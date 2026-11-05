//! 游戏资产来源：统一 IoStore 与 PAK 的列举、读取和侧车文件解析。
//!
//! 查看器、导出和媒体播放器都通过该类型读取容器，避免各自重复处理
//! `.utoc/.ucas` 与 `.pak` 的差异。密钥只保存在容器对象内部，本模块不记录、
//! 不打印明文密钥。

use std::path::{Path, PathBuf};

use lumark_archive::{ArchiveError, IoStoreContainer, PakAssetReader};

use crate::package::{
    AssetKind, ZenScriptObjects, is_legacy_package, parse_legacy_summary, parse_package,
};

/// 资产类型摘要：类名与推断出的资产种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetClassSummary {
    /// UE 类名，例如 `SkeletalMesh`。
    pub class_name: String,
    /// 推断的资产种类，供文件表图标和详情展示使用。
    pub kind: AssetKind,
}

/// 容器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetContainerKind {
    /// Unreal `.pak`。
    Pak,
    /// IoStore `.utoc`（配套 `.ucas`）。
    IoStore,
}

impl AssetContainerKind {
    /// 返回界面展示名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Pak => "Unreal PAK",
            Self::IoStore => "IoStore",
        }
    }
}

/// 容器中的一个资产条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameAssetEntry {
    /// 容器内路径。
    pub path: String,
    /// 解压后大小。
    pub size: u64,
}

/// 一个资产包及其侧车文件。
#[derive(Debug, Clone, Default)]
pub struct AssetInputs {
    /// `.uasset` / `.umap` 主体字节。
    pub uasset: Vec<u8>,
    /// `.uexp` 侧车（无版本属性或旧式导出数据）。
    pub uexp: Option<Vec<u8>>,
    /// `.ubulk` 侧车（纹理 mip、网格顶点等内联块）。
    pub ubulk: Option<Vec<u8>>,
}

/// 打开游戏资产来源所需的配置。
#[derive(Debug, Clone, Default)]
pub struct GameAssetOptions<'a> {
    /// AES 密钥；加密容器必须提供。
    pub key: Option<&'a str>,
    /// `global.utoc`，用于读取 UE5 脚本对象表。
    pub global: Option<PathBuf>,
    /// 可选的 Oodle DLL；`None` 表示使用默认后端（纯 Rust 优先）。
    pub oodle: Option<PathBuf>,
    /// 可选的 `.usmap` 映射路径，仅作为配置透传。
    pub mappings: Option<PathBuf>,
}

/// 资产来源错误。
#[derive(Debug, thiserror::Error)]
pub enum AssetSourceError {
    /// 文件或目录 IO 错误。
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    /// 容器需要密码。
    #[error("需要密码")]
    PasswordRequired,
    /// 容器密码错误。
    #[error("密码错误")]
    WrongPassword,
    /// 容器解析或读取失败。
    #[error("归档错误：{0}")]
    Archive(String),
    /// 暂不支持的容器类型。
    #[error("不支持的容器：{0}")]
    Unsupported(String),
}

impl From<ArchiveError> for AssetSourceError {
    /// 把统一归档错误折叠为资产来源可处理的分类。
    fn from(error: ArchiveError) -> Self {
        match error {
            ArchiveError::PasswordRequired => Self::PasswordRequired,
            ArchiveError::WrongPassword => Self::WrongPassword,
            other => Self::Archive(other.to_string()),
        }
    }
}

/// Oodle 预检结果（O-2）。
#[derive(Debug, Clone)]
pub struct OodlePreflight {
    /// 容器是否需要 Oodle。
    pub needed: bool,
    /// 当前配置下能否解压所需块。
    pub ready: bool,
    /// 容器声明的压缩方法。
    pub methods: Vec<String>,
    /// 当前后端状态。
    pub status: lumark_archive::OodleStatus,
}

impl OodlePreflight {
    /// 构造并计算可用性。
    pub fn new(methods: Vec<String>, status: lumark_archive::OodleStatus) -> Self {
        let needed = methods
            .iter()
            .any(|method| method.eq_ignore_ascii_case("oodle"));
        let ready = !needed || status.usable();
        Self {
            needed,
            ready,
            methods,
            status,
        }
    }

    /// 界面展示。
    pub fn summary(&self) -> String {
        if !self.needed {
            return format!(
                "无需 Oodle；压缩方法：{}",
                if self.methods.is_empty() {
                    "无".to_string()
                } else {
                    self.methods.join(", ")
                }
            );
        }
        format!(
            "需要 Oodle（{}）；后端：{}",
            if self.ready {
                "可解压"
            } else {
                "缺少后端"
            },
            self.status.summary()
        )
    }
}

/// 容器句柄。
enum Container {
    /// Unreal PAK 按条目读取器。
    Pak(PakAssetReader),
    /// IoStore 容器。
    IoStore(IoStoreContainer),
}

/// 游戏资产来源：一个已打开的容器加上可选的全局脚本对象和映射路径。
pub struct GameAssetSource {
    archive: PathBuf,
    container: Container,
    global: Option<PathBuf>,
    mappings: Option<PathBuf>,
    scripts: Option<ZenScriptObjects>,
    warnings: Vec<String>,
}

impl GameAssetSource {
    /// 打开一个游戏容器。
    ///
    /// Oodle DLL 配置失败只记录告警而不阻止打开：纯 Rust 后端可独立工作，
    /// 真正的解压失败会在读取阶段以可读错误返回。
    pub fn open(
        archive: impl Into<PathBuf>,
        options: GameAssetOptions<'_>,
    ) -> Result<Self, AssetSourceError> {
        let archive = archive.into();
        if !archive.is_file() {
            return Err(AssetSourceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("容器不存在：{}", archive.display()),
            )));
        }
        let mut warnings = Vec::new();
        // 只在显式提供 DLL 时改动进程级 Oodle 配置；None 保留上层（游戏会话）
        // 已配置的后端，避免一次预览把游戏设置里的 DLL 清掉。
        if let Some(oodle) = options.oodle.clone()
            && let Err(error) = lumark_archive::configure_oodle_library(Some(oodle))
        {
            warnings.push(format!(
                "Oodle DLL 配置失败，将继续使用内置 Rust 解码器：{error}"
            ));
        }
        let container = match archive
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("utoc") => Container::IoStore(IoStoreContainer::open(&archive, options.key)?),
            Some("pak") => Container::Pak(PakAssetReader::open(&archive, options.key)?),
            _ => {
                return Err(AssetSourceError::Unsupported(format!(
                    "仅支持 .utoc 与 .pak 容器：{}",
                    archive.display()
                )));
            }
        };
        let global = options.global.filter(|path| path.is_file());
        let scripts = global
            .as_ref()
            .and_then(|path| ZenScriptObjects::from_container(path, options.key).ok());
        if global.is_some() && scripts.is_none() {
            warnings.push("全局脚本对象表读取失败，部分资产将缺少类名解析".to_string());
        }
        Ok(Self {
            archive,
            container,
            global,
            mappings: options.mappings.filter(|path| path.is_file()),
            scripts,
            warnings,
        })
    }

    /// 返回容器类型。
    pub fn kind(&self) -> AssetContainerKind {
        match &self.container {
            Container::Pak(_) => AssetContainerKind::Pak,
            Container::IoStore(_) => AssetContainerKind::IoStore,
        }
    }

    /// 返回容器路径。
    pub fn archive(&self) -> &Path {
        &self.archive
    }

    /// 返回全局 `global.utoc` 路径。
    pub fn global(&self) -> Option<&Path> {
        self.global.as_deref()
    }

    /// 返回配置的 `.usmap` 路径。
    pub fn mappings(&self) -> Option<&Path> {
        self.mappings.as_deref()
    }

    /// 返回已加载的全局脚本对象表。
    pub fn scripts(&self) -> Option<&ZenScriptObjects> {
        self.scripts.as_ref()
    }

    /// 返回目录索引路径对应的 IoStore 包 ID。
    pub fn package_id_for_path(&self, path: &str) -> Option<u64> {
        match &self.container {
            Container::IoStore(container) => container.package_id_for_path(path),
            Container::Pak(_) => None,
        }
    }

    /// 按 IoStore 包 ID 反查容器内资产路径。
    pub fn package_path_for_id(&self, package_id: u64) -> Option<String> {
        match &self.container {
            Container::IoStore(container) => container.package_path_for_id(package_id),
            Container::Pak(_) => None,
        }
    }

    /// 返回 IoStore 包的直接导入包 ID（来自 ContainerHeader store entry）。
    pub fn imported_packages(&self, package_id: u64) -> Result<Vec<u64>, ArchiveError> {
        match &self.container {
            Container::IoStore(container) => container.imported_packages(package_id),
            Container::Pak(_) => Ok(Vec::new()),
        }
    }

    /// 按包 ID 读取 IoStore 包的 uasset 与可选 ubulk。
    pub fn read_package_by_id(
        &self,
        package_id: u64,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), ArchiveError> {
        match &self.container {
            Container::IoStore(container) => {
                let uasset = container.read_package_by_id(package_id)?;
                let bulk = container.find_package_chunks(package_id, 2);
                let ubulk = if bulk.is_empty() {
                    None
                } else {
                    Some(container.read_bulk_by_id(package_id)?)
                };
                Ok((uasset, ubulk))
            }
            Container::Pak(_) => Err(ArchiveError::Corrupt(
                "PAK 容器不支持按包 ID 读取".to_string(),
            )),
        }
    }

    /// 取出全局脚本对象表，供一次性读取调用转移所有权。
    pub fn take_scripts(&mut self) -> Option<ZenScriptObjects> {
        self.scripts.take()
    }

    /// 返回打开阶段的非致命告警。
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 返回是否加密。
    pub fn encrypted(&self) -> bool {
        match &self.container {
            Container::Pak(reader) => reader.encrypted(),
            Container::IoStore(container) => container.encrypted(),
        }
    }

    /// 返回容器是否需要 Oodle 解压后端。
    pub fn needs_oodle(&self) -> bool {
        match &self.container {
            Container::Pak(reader) => reader.needs_oodle(),
            Container::IoStore(container) => container
                .compression_methods()
                .iter()
                .any(|method| method.eq_ignore_ascii_case("Oodle")),
        }
    }

    /// 返回压缩方法名（去重）。
    pub fn compression_methods(&self) -> Vec<String> {
        let raw = match &self.container {
            Container::Pak(reader) => reader.compression_methods(),
            Container::IoStore(container) => container.compression_methods().to_vec(),
        };
        let mut methods: Vec<String> = Vec::new();
        for method in raw {
            if !methods.iter().any(|existing| existing == &method) {
                methods.push(method);
            }
        }
        methods
    }

    /// 容器 Oodle 预检：是否需要、当前后端能否解压（O-2）。
    pub fn oodle_preflight(&self) -> OodlePreflight {
        OodlePreflight::new(self.compression_methods(), lumark_archive::oodle_status())
    }

    /// 列出容器内的全部条目。
    pub fn list(&self) -> Vec<GameAssetEntry> {
        match &self.container {
            Container::Pak(reader) => reader
                .entries()
                .into_iter()
                .map(|entry| GameAssetEntry {
                    path: entry.path,
                    size: entry.size,
                })
                .collect(),
            Container::IoStore(container) => container
                .files()
                .iter()
                .map(|file| GameAssetEntry {
                    path: file.path.clone(),
                    size: file.size,
                })
                .collect(),
        }
    }

    /// 返回条目数量。
    pub fn entry_count(&self) -> usize {
        match &self.container {
            Container::Pak(reader) => reader.entry_count(),
            Container::IoStore(container) => container.files().len(),
        }
    }

    /// 判断容器内是否存在指定路径。
    pub fn contains(&self, path: &str) -> bool {
        let path = normalize_container_path(path);
        match &self.container {
            Container::Pak(reader) => reader.contains(&path),
            Container::IoStore(container) => container.contains_file(&path),
        }
    }

    /// 读取单个条目的解压后字节。
    pub fn read_bytes(&mut self, path: &str) -> Result<Vec<u8>, AssetSourceError> {
        let path = normalize_container_path(path);
        match &mut self.container {
            Container::Pak(reader) => Ok(reader.read_entry(&path)?),
            Container::IoStore(container) => Ok(container.read_file(&path)?),
        }
    }

    /// 读取资产包主体和 `.uexp`/`.ubulk` 侧车文件。
    ///
    /// 侧车是否存在通过容器索引判断，因此缺失侧车不会产生错误；其余读取失败
    /// 仍原样返回，避免把损坏文件误判为“没有侧车”。
    pub fn read_package(&mut self, path: &str) -> Result<AssetInputs, AssetSourceError> {
        let path = normalize_container_path(path);
        let uasset = self.read_bytes(&path)?;
        let stem = path
            .strip_suffix(".uasset")
            .or_else(|| path.strip_suffix(".umap"));
        let mut inputs = AssetInputs {
            uasset,
            ..Default::default()
        };
        let Some(stem) = stem else {
            return Ok(inputs);
        };
        let uexp = format!("{stem}.uexp");
        if self.contains(&uexp) {
            inputs.uexp = Some(self.read_bytes(&uexp)?);
        }
        let ubulk = format!("{stem}.ubulk");
        if self.contains(&ubulk) {
            inputs.ubulk = Some(self.read_bytes(&ubulk)?);
        } else if inputs.ubulk.is_none() {
            // 目录索引里没有独立 `.ubulk` 条目时，按包 ID 取该包的 BulkData chunk。
            inputs.ubulk = self.read_bulk_by_path(&path)?;
        }
        Ok(inputs)
    }

    /// 按资产路径读取同一 IoStore 包的 BulkData chunk。
    ///
    /// 部分游戏的 bulk 数据只以包 ID 关联、不在目录索引里单独列出（FModel 也从
    /// 包侧解析），因此路径侧车缺失时再用包 ID 兜底。
    fn read_bulk_by_path(&self, path: &str) -> Result<Option<Vec<u8>>, AssetSourceError> {
        let Container::IoStore(container) = &self.container else {
            return Ok(None);
        };
        let Some(package_id) = container.package_id_for_path(path) else {
            return Ok(None);
        };
        if container.find_package_chunks(package_id, 2).is_empty() {
            return Ok(None);
        }
        Ok(Some(container.read_bulk_by_id(package_id)?))
    }

    /// 只解析包头部，识别第一个导出对象的类名与资产种类。
    ///
    /// 文件表类型图标使用该接口：旧式包只需 `uasset` 头，Zen 包解析
    /// 名称表/导出表；不读取 `.uexp` 属性，也不解码贴图或网格。
    pub fn read_asset_class(&mut self, path: &str) -> Option<AssetClassSummary> {
        let inputs = self.read_package(path).ok()?;
        if is_legacy_package(&inputs.uasset) {
            let summary = parse_legacy_summary(&inputs.uasset).ok()?;
            let kind = AssetKind::classify(&summary.class_name, &summary.export_name);
            return Some(AssetClassSummary {
                class_name: summary.class_name,
                kind,
            });
        }
        let package = parse_package(inputs.uasset, self.scripts(), None).ok()?;
        let export = package
            .info()
            .exports
            .iter()
            .find(|export| export.outer_export.is_none())?;
        let kind = AssetKind::classify(&export.class_name, &export.name);
        Some(AssetClassSummary {
            class_name: export.class_name.clone(),
            kind,
        })
    }
}

/// 一次性读取资产包的结果，供无法长期持有 `GameAssetSource` 的界面预览调用。
#[derive(Debug)]
pub struct AssetPackageRead {
    /// 资产包字节与 `.uexp`/`.ubulk` 侧车。
    pub inputs: AssetInputs,
    /// `global.utoc` 的脚本对象表；读取失败时为 `None`。
    pub scripts: Option<ZenScriptObjects>,
}

/// 打开容器并读取一个资产包。
///
/// 每次调用都会重新打开容器；需要连续读取多个资产的调用方应直接持有
/// [`GameAssetSource`] 并复用，避免重复解析索引。
pub fn read_asset_package(
    archive: &Path,
    global: &Path,
    relative: &str,
    key: Option<&str>,
) -> Result<AssetPackageRead, AssetSourceError> {
    let options = GameAssetOptions {
        key,
        global: Some(global.to_path_buf()),
        ..Default::default()
    };
    let mut source = GameAssetSource::open(archive, options)?;
    let inputs = source.read_package(relative)?;
    let scripts = source.take_scripts();
    Ok(AssetPackageRead { inputs, scripts })
}

/// 规范化容器内路径：去掉前导 `/`，统一为 `/` 分隔。
fn normalize_container_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/').replace('\\', "/");
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::{OodlePreflight, normalize_container_path};

    /// 容器路径应统一为 `/` 分隔并去掉前导斜杠。
    #[test]
    fn normalizes_container_paths() {
        assert_eq!(
            normalize_container_path("/Game/A/B.uasset"),
            "Game/A/B.uasset"
        );
        assert_eq!(
            normalize_container_path("Game\\A\\B.uasset"),
            "Game/A/B.uasset"
        );
        assert_eq!(normalize_container_path("/"), "/");
    }

    /// Oodle 预检：无需 Oodle 时 ready；需要时依赖后端状态。
    #[test]
    fn oodle_preflight_reports_readiness() {
        let status = lumark_archive::OodleStatus {
            setting: "auto",
            rust_available: true,
            dll_supported: true,
            dll_configured: None,
            dll_loaded: false,
        };
        let plain = OodlePreflight::new(vec!["Zlib".to_string()], status.clone());
        assert!(!plain.needed);
        assert!(plain.ready);
        assert!(plain.summary().contains("无需 Oodle"));
        let oodle = OodlePreflight::new(vec!["Oodle".to_string()], status.clone());
        assert!(oodle.needed);
        assert!(oodle.ready);
        let dll_only = OodlePreflight::new(
            vec!["Oodle".to_string()],
            lumark_archive::OodleStatus {
                setting: "dll",
                rust_available: false,
                dll_supported: true,
                dll_configured: None,
                dll_loaded: false,
            },
        );
        assert!(!dll_only.ready);
        assert!(dll_only.summary().contains("缺少后端"));
    }
}
