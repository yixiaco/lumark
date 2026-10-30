//! 扫描游戏安装目录、定位 UE 项目、引擎版本和容器清单。
//!
//! 扫描只读取目录名、文件大小和 `.pak/.utoc` 头部，不解压、不修改游戏
//! 文件；结果用于界面展示和后续密钥提取。

use std::path::{Path, PathBuf};

use memchr::memmem;
use serde::{Deserialize, Serialize};

use lumark_archive::ArchiveService;

/// 已识别的 Unreal Engine 版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UnrealEngine {
    /// UE 4.27。
    Ue4_27,
    /// UE 5.0。
    Ue5_0,
    /// UE 5.1。
    Ue5_1,
    /// UE 5.2。
    Ue5_2,
    /// UE 5.3。
    Ue5_3,
    /// UE 5.4。
    Ue5_4,
    /// UE 5.5。
    Ue5_5,
    /// UE 5.6。
    Ue5_6,
    /// 未识别。
    #[default]
    Unknown,
}

impl UnrealEngine {
    /// 返回界面展示名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Ue4_27 => "UE 4.27",
            Self::Ue5_0 => "UE 5.0",
            Self::Ue5_1 => "UE 5.1",
            Self::Ue5_2 => "UE 5.2",
            Self::Ue5_3 => "UE 5.3",
            Self::Ue5_4 => "UE 5.4",
            Self::Ue5_5 => "UE 5.5",
            Self::Ue5_6 => "UE 5.6",
            Self::Unknown => "未知版本",
        }
    }

    /// 返回设置和资产解析器使用的版本文本，例如 `5.6`。
    pub fn setting(self) -> &'static str {
        match self {
            Self::Ue4_27 => "4.27",
            Self::Ue5_0 => "5.0",
            Self::Ue5_1 => "5.1",
            Self::Ue5_2 => "5.2",
            Self::Ue5_3 => "5.3",
            Self::Ue5_4 => "5.4",
            Self::Ue5_5 => "5.5",
            Self::Ue5_6 => "5.6",
            Self::Unknown => "",
        }
    }

    /// 从 `major.minor` 文本解析版本；不支持时返回 `None`。
    pub fn from_setting(value: &str) -> Option<Self> {
        match value.trim() {
            "4.27" => Some(Self::Ue4_27),
            "5.0" => Some(Self::Ue5_0),
            "5.1" => Some(Self::Ue5_1),
            "5.2" => Some(Self::Ue5_2),
            "5.3" => Some(Self::Ue5_3),
            "5.4" => Some(Self::Ue5_4),
            "5.5" => Some(Self::Ue5_5),
            "5.6" => Some(Self::Ue5_6),
            _ => None,
        }
    }

    /// 按主版本和次版本构造版本。
    fn from_major_minor(major: u32, minor: u32) -> Option<Self> {
        match (major, minor) {
            (4, 27) => Some(Self::Ue4_27),
            (5, 0) => Some(Self::Ue5_0),
            (5, 1) => Some(Self::Ue5_1),
            (5, 2) => Some(Self::Ue5_2),
            (5, 3) => Some(Self::Ue5_3),
            (5, 4) => Some(Self::Ue5_4),
            (5, 5) => Some(Self::Ue5_5),
            (5, 6) => Some(Self::Ue5_6),
            _ => None,
        }
    }
}

/// 容器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerKind {
    /// Unreal `.pak`。
    Pak,
    /// IoStore `.utoc`（配套 `.ucas`）。
    IoStore,
}

impl ContainerKind {
    /// 返回界面展示名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Pak => "Unreal PAK",
            Self::IoStore => "IoStore",
        }
    }
}

/// 一个游戏容器的初步信息。
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    /// `.pak` 或 `.utoc` 路径。
    pub path: PathBuf,
    /// 容器类型。
    pub kind: ContainerKind,
    /// 容器文件大小。
    pub size: u64,
    /// 是否加密；探测失败时为 `None`。
    pub encrypted: Option<bool>,
}

/// 扫描结果。
#[derive(Debug, Clone)]
pub struct DetectedGame {
    /// 用户选择的安装根目录。
    pub root: PathBuf,
    /// 包含 `Content` 和 `Binaries` 的项目目录。
    pub project_dir: PathBuf,
    /// 默认游戏名（项目目录名）。
    pub name: String,
    /// 从 Shipping.exe 探测到的引擎版本。
    pub engine: UnrealEngine,
    /// 探测到的 Shipping.exe。
    pub shipping_exe: Option<PathBuf>,
    /// 容器列表，`global.utoc` 在前。
    pub containers: Vec<ContainerInfo>,
    /// 游戏目录内可用的 Oodle 动态库候选。
    pub oodle_candidates: Vec<PathBuf>,
    /// 游戏目录内发现的可覆盖 `.usmap` 映射文件。
    pub mapping_candidates: Vec<PathBuf>,
}

/// 游戏扫描错误。
#[derive(Debug, thiserror::Error)]
pub enum GameScanError {
    /// 路径不存在或不是目录。
    #[error("游戏目录不存在：{0}")]
    NotFound(String),
    /// 没有找到包含 Content/Binaries 的 UE 项目。
    #[error("在目录中没有找到 Unreal 项目（Content/Paks 或 Binaries/Win64）：{0}")]
    NoProject(String),
    /// 读取目录失败。
    #[error("读取游戏目录失败：{0}")]
    Io(#[from] std::io::Error),
}

/// 扫描游戏目录并返回项目、引擎版本、容器和 Oodle 候选。
pub fn scan_game(root: &Path) -> Result<DetectedGame, GameScanError> {
    if !root.is_dir() {
        return Err(GameScanError::NotFound(root.display().to_string()));
    }
    let project_dir = find_project_dir(root)
        .ok_or_else(|| GameScanError::NoProject(root.display().to_string()))?;
    let shipping_exe = find_shipping_exe(&project_dir);
    let engine = shipping_exe
        .as_deref()
        .and_then(detect_engine_from_file)
        .unwrap_or_default();
    let name = project_dir
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Unreal Game")
        .to_string();
    let containers = discover_containers(root, &project_dir)?;
    let oodle_candidates = discover_oodle_candidates(root, &project_dir);
    let mapping_candidates = discover_mapping_candidates(root, &project_dir);
    Ok(DetectedGame {
        root: root.to_path_buf(),
        project_dir,
        name,
        engine,
        shipping_exe,
        containers,
        oodle_candidates,
        mapping_candidates,
    })
}

/// 判断目录是否像 UE 项目根。
fn has_project_layout(directory: &Path) -> bool {
    directory.join("Content").is_dir() || directory.join("Binaries").is_dir()
}

/// 在安装根和一级项目目录中选择项目根。
fn find_project_dir(root: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if has_project_layout(root) {
        candidates.push(root.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(name.as_str(), "engine" | "content" | "binaries" | "saved") {
                continue;
            }
            if has_project_layout(&path) {
                candidates.push(path);
            }
        }
    }
    candidates.into_iter().max_by_key(|path| {
        (
            u8::from(find_shipping_exe(path).is_some()),
            u8::from(path.join("Content/Paks").is_dir()),
            std::cmp::Reverse(path.as_os_str().len()),
        )
    })
}

/// 在项目的 `Binaries/Win64` 中查找 `*Shipping.exe`。
fn find_shipping_exe(project: &Path) -> Option<PathBuf> {
    let directory = project.join("Binaries/Win64");
    let mut candidates = Vec::new();
    let mut any_exe = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".exe") {
                any_exe.push(path.clone());
            }
            if name.ends_with("shipping.exe") || name.ends_with("-win64-shipping.exe") {
                candidates.push(path);
            }
        }
    }
    // 部分游戏（例如 FF7 Remake）没有 `*-Win64-Shipping.exe` 命名，
    // 但 Binaries/Win64 下最大的可执行文件就是游戏主程序。
    if candidates.is_empty() {
        any_exe.sort_by_key(|path| {
            std::cmp::Reverse(std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0))
        });
        return any_exe.into_iter().next();
    }
    candidates.sort_by_key(|path| std::cmp::Reverse(path.as_os_str().len()));
    candidates.into_iter().next()
}

/// 探测 Shipping.exe 内的 `++UE5+Release-5.6` 版本字符串。
fn detect_engine_from_file(path: &Path) -> Option<UnrealEngine> {
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() == 0 {
        return None;
    }
    // 只映射只读文件；游戏 exe 通常较大，映射避免一次性复制。
    let map = unsafe { memmap2::Mmap::map(&file).ok()? };
    detect_engine_bytes(&map)
}

/// 从二进制数据中识别 UE 版本，兼容 ASCII 与 UTF-16LE 版本资源。
pub fn detect_engine_bytes(data: &[u8]) -> Option<UnrealEngine> {
    if let Some(version) = find_ascii_version(data) {
        return Some(version);
    }
    find_utf16_version(data)
}

/// 扫描 ASCII `++UE4+Release-4.27` / `++UE5+Release-5.6`。
fn find_ascii_version(data: &[u8]) -> Option<UnrealEngine> {
    for index in memmem::find_iter(data, b"++UE") {
        let end = (index + 64).min(data.len());
        let text = String::from_utf8_lossy(&data[index..end]);
        if let Some(version) = parse_release_text(&text) {
            return Some(version);
        }
    }
    None
}

/// 扫描 UTF-16LE 版本资源字符串。
fn find_utf16_version(data: &[u8]) -> Option<UnrealEngine> {
    let needle = [b'+', 0, b'+', 0, b'U', 0, b'E', 0];
    for index in memmem::find_iter(data, &needle) {
        let end = (index + 128).min(data.len());
        let units = data[index..end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|value| *value != 0)
            .collect::<Vec<_>>();
        let text = String::from_utf16_lossy(&units);
        if let Some(version) = parse_release_text(&text) {
            return Some(version);
        }
    }
    None
}

/// 从包含 `UE5+Release-5.6` 的文本解析版本。
fn parse_release_text(text: &str) -> Option<UnrealEngine> {
    let release = text.find("Release-")?;
    let rest = &text[release + "Release-".len()..];
    let mut parts = rest.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor_text = parts.next()?;
    let minor = minor_text
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u32>()
        .ok()?;
    UnrealEngine::from_major_minor(major, minor)
}

/// 收集项目与安装根 `Content/Paks` 下的容器。
fn discover_containers(root: &Path, project: &Path) -> Result<Vec<ContainerInfo>, GameScanError> {
    let mut directories = vec![project.join("Content/Paks")];
    if root != project {
        directories.push(root.join("Content/Paks"));
    }
    let mut paths = Vec::new();
    for directory in directories {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            match extension.as_deref() {
                Some("utoc") | Some("pak") => paths.push(path),
                _ => {}
            }
        }
    }
    paths.sort_by_key(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        (name != "global.utoc", name)
    });
    paths.dedup();
    let mut containers = Vec::with_capacity(paths.len());
    for path in paths {
        let kind = if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("utoc"))
        {
            ContainerKind::IoStore
        } else {
            ContainerKind::Pak
        };
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        let encrypted = ArchiveService::probe_encryption(&path)
            .ok()
            .map(|info| info.encrypted);
        containers.push(ContainerInfo {
            path,
            kind,
            size,
            encrypted,
        });
    }
    Ok(containers)
}

/// 收集游戏目录内常见的 `.usmap` 映射候选。
fn discover_mapping_candidates(root: &Path, project: &Path) -> Vec<PathBuf> {
    let mut directories = vec![
        root.to_path_buf(),
        project.to_path_buf(),
        root.join("Mappings"),
        project.join("Mappings"),
        project.join("Content/Paks"),
    ];
    // 部分整合包把映射放在 `5.6.1-.../Mappings` 这类构建目录下，
    // 只扫描游戏根目录的一级子目录，避免遍历整个安装。
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path.join("Mappings"));
                directories.push(path);
            }
        }
    }
    let mut candidates = Vec::new();
    for directory in directories {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("usmap"))
            {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates.truncate(8);
    candidates
}

/// 收集游戏目录内的 Oodle 动态库候选。
fn discover_oodle_candidates(root: &Path, project: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let roots = [
        root.join("Engine/Binaries/ThirdParty/Oodle"),
        project.join("Binaries/Win64"),
    ];
    for directory in roots {
        collect_oodle_libraries(&directory, 0, &mut candidates);
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

/// 递归收集 Oodle DLL，深度上限避免扫描整个安装。
fn collect_oodle_libraries(directory: &Path, depth: usize, output: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_oodle_libraries(&path, depth + 1, output);
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".dll") && (name.contains("oo2core") || name.contains("oodle")) {
            output.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 构造包含版本字符串的 Shipping.exe 内容。
    fn engine_exe(text: &str) -> Vec<u8> {
        let mut bytes = vec![0u8; 16];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    /// 验证 ASCII 与 UTF-16 版本字符串都识别为 UE 5.6。
    #[test]
    fn engine_version_is_detected_in_both_encodings() {
        let ascii = b"..++UE5+Release-5.6-CL-44394996..";
        assert_eq!(detect_engine_bytes(ascii), Some(UnrealEngine::Ue5_6));
        let utf16 = engine_exe("++UE5+Release-5.6-CL-44394996");
        assert_eq!(detect_engine_bytes(&utf16), Some(UnrealEngine::Ue5_6));
        assert_eq!(
            parse_release_text("++UE4+Release-4.27-CL-1"),
            Some(UnrealEngine::Ue4_27)
        );
        assert_eq!(parse_release_text("no version"), None);
    }

    /// 验证安装根下的一级项目目录、容器顺序和 Shipping.exe。
    #[test]
    fn scans_install_root_with_project_subdirectory() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("TogetherBNB");
        let paks = project.join("Content/Paks");
        let binaries = project.join("Binaries/Win64");
        fs::create_dir_all(&paks).unwrap();
        fs::create_dir_all(&binaries).unwrap();
        fs::write(paks.join("pakchunk0-Windows.utoc"), b"not a real utoc").unwrap();
        fs::write(paks.join("global.utoc"), b"not a real utoc").unwrap();
        fs::write(
            binaries.join("TogetherBNB-Win64-Shipping.exe"),
            engine_exe("++UE5+Release-5.6-CL-1"),
        )
        .unwrap();
        let game = scan_game(directory.path()).unwrap();
        assert_eq!(game.project_dir, project);
        assert_eq!(game.name, "TogetherBNB");
        assert_eq!(game.engine, UnrealEngine::Ue5_6);
        assert_eq!(
            game.shipping_exe.as_deref(),
            Some(binaries.join("TogetherBNB-Win64-Shipping.exe").as_path())
        );
        assert_eq!(game.containers.len(), 2);
        assert!(game.containers[0].path.ends_with("global.utoc"));
        assert_eq!(game.containers[0].kind, ContainerKind::IoStore);
        assert!(
            game.containers
                .iter()
                .all(|container| container.encrypted.is_none())
        );
    }

    /// 验证项目直接位于用户选择目录时也能扫描。
    #[test]
    fn scans_project_root_layout() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("Content/Paks")).unwrap();
        fs::write(
            directory.path().join("Content/Paks/pakchunk0-Windows.pak"),
            b"not a real pak",
        )
        .unwrap();
        let game = scan_game(directory.path()).unwrap();
        assert_eq!(game.project_dir, directory.path());
        assert_eq!(game.engine, UnrealEngine::Unknown);
        assert_eq!(game.containers.len(), 1);
        assert_eq!(game.containers[0].kind, ContainerKind::Pak);
    }

    /// 缺少 UE 项目结构时返回可读错误。
    #[test]
    fn missing_project_reports_error() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            scan_game(directory.path()),
            Err(GameScanError::NoProject(_))
        ));
        assert!(matches!(
            scan_game(&directory.path().join("missing")),
            Err(GameScanError::NotFound(_))
        ));
    }

    /// 没有 `-Win64-Shipping.exe` 命名时回退到最大的可执行文件。
    #[test]
    fn finds_largest_exe_without_shipping_suffix() {
        let directory = tempfile::tempdir().unwrap();
        let binaries = directory.path().join("Binaries/Win64");
        fs::create_dir_all(&binaries).unwrap();
        fs::write(binaries.join("Game-Win64-Shipping.exe"), b"x").unwrap();
        fs::write(binaries.join("CrashReporter.exe"), b"xx").unwrap();
        assert!(
            find_shipping_exe(directory.path())
                .unwrap()
                .ends_with("Game-Win64-Shipping.exe")
        );

        let fallback = tempfile::tempdir().unwrap();
        let fallback_binaries = fallback.path().join("Binaries/Win64");
        fs::create_dir_all(&fallback_binaries).unwrap();
        fs::write(fallback_binaries.join("game_.exe"), b"main").unwrap();
        fs::write(fallback_binaries.join("helper.exe"), b"x").unwrap();
        let found = find_shipping_exe(fallback.path()).unwrap();
        assert!(found.ends_with("game_.exe"), "found={found:?}");
    }

    /// 验证 Oodle DLL 候选只匹配库文件名。
    #[test]
    fn finds_oodle_candidates_in_engine_tree() {
        let directory = tempfile::tempdir().unwrap();
        let oodle = directory
            .path()
            .join("Engine/Binaries/ThirdParty/Oodle/Win64");
        fs::create_dir_all(&oodle).unwrap();
        fs::write(oodle.join("oo2core_7_win64.dll"), b"dll").unwrap();
        fs::write(oodle.join("readme.txt"), b"text").unwrap();
        let candidates = discover_oodle_candidates(directory.path(), directory.path());
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].ends_with("oo2core_7_win64.dll"));
    }

    /// 验证游戏目录及 Mappings/ 下的 `.usmap` 会被发现。
    #[test]
    fn discovers_mapping_candidates() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("Mappings")).unwrap();
        fs::create_dir_all(directory.path().join("Build/Mappings")).unwrap();
        fs::write(directory.path().join("game.usmap"), b"map").unwrap();
        fs::write(directory.path().join("Mappings/extra.usmap"), b"map").unwrap();
        fs::write(directory.path().join("Build/Mappings/build.usmap"), b"map").unwrap();
        fs::write(directory.path().join("readme.txt"), b"text").unwrap();
        let candidates = discover_mapping_candidates(directory.path(), directory.path());
        assert_eq!(candidates.len(), 3);
        assert!(candidates.iter().all(|path| path.extension().is_some()));
    }
}
