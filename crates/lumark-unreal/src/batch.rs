//! 整包批量导出与每游戏导出配置。
//!
//! 遍历容器内 `.uasset`，自动识别 StaticMesh/SkeletalMesh，按配置写出
//! PSK / GLB / UEMODEL / USDA，并保留资产在容器中的相对目录结构。
//! 单个资产失败不会中断整批；报告包含成功、跳过与失败明细。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use lumark_export::{
    ExportOptions, GltfExport, write_glb, write_psk, write_ueformat_model, write_usda,
};
use serde::{Deserialize, Serialize};

use crate::export_input::build_mesh_export;
use crate::{GameAssetSource, Mappings};

/// 批量导出的目标格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchExportFormat {
    /// glTF 2.0 binary。
    Glb,
    /// UEFormat UEMODEL。
    UeModel,
    /// USD ASCII。
    Usda,
    /// ActorX PSK。
    Psk,
}

impl BatchExportFormat {
    /// 全部格式。
    pub fn all() -> [Self; 4] {
        [Self::Glb, Self::UeModel, Self::Usda, Self::Psk]
    }

    /// 输出扩展名。
    pub fn extension(self) -> &'static str {
        match self {
            Self::Glb => "glb",
            Self::UeModel => "uemodel",
            Self::Usda => "usda",
            Self::Psk => "psk",
        }
    }

    /// 界面标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Glb => "glTF",
            Self::UeModel => "UEFormat",
            Self::Usda => "USDA",
            Self::Psk => "ActorX PSK",
        }
    }
}

/// 每游戏导出配置（可序列化为 JSON）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchExportConfig {
    /// 输出根目录。
    pub output_dir: PathBuf,
    /// 目标格式。
    pub formats: Vec<BatchExportFormat>,
    /// 位置缩放（UE 厘米 → 米用 0.01；PSK/UEMODEL 通常保持 1.0）。
    pub scale: f32,
    /// 是否解析并导出材质/贴图。
    pub include_materials: bool,
    /// 贴图最大边长。
    pub max_texture_size: u32,
    /// 是否写顶点色。
    pub vertex_colors: bool,
    /// 是否写额外 UV。
    pub extra_uvs: bool,
    /// 资产路径子串过滤（不区分大小写）。
    pub asset_filter: Option<String>,
    /// 最多处理资产数。
    pub limit: Option<usize>,
    /// 是否覆盖已存在文件。
    pub overwrite: bool,
}

impl Default for BatchExportConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("export"),
            formats: vec![BatchExportFormat::Glb],
            scale: 1.0,
            include_materials: true,
            max_texture_size: 512,
            vertex_colors: true,
            extra_uvs: true,
            asset_filter: None,
            limit: None,
            overwrite: false,
        }
    }
}

impl BatchExportConfig {
    /// 从 JSON 文件加载；缺失字段使用默认值。
    pub fn load(path: &Path) -> Result<Self, String> {
        let data =
            std::fs::read_to_string(path).map_err(|error| format!("读取导出配置失败：{error}"))?;
        serde_json::from_str(&data).map_err(|error| format!("解析导出配置失败：{error}"))
    }

    /// 保存为 JSON 文件。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建配置目录失败：{error}"))?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|error| format!("序列化导出配置失败：{error}"))?;
        std::fs::write(path, data).map_err(|error| format!("写入导出配置失败：{error}"))
    }

    /// 规范化：格式为空时回退 GLB，缩放限制在合理范围。
    pub fn normalized(mut self) -> Self {
        if self.formats.is_empty() {
            self.formats = vec![BatchExportFormat::Glb];
        }
        self.formats.sort_by_key(|format| format.extension());
        self.formats.dedup();
        self.scale = self.scale.clamp(0.0001, 100.0);
        self.max_texture_size = self.max_texture_size.clamp(16, 8192);
        self
    }

    /// 导出选项。
    pub fn export_options(&self) -> ExportOptions {
        ExportOptions {
            scale: self.scale,
            vertex_colors: self.vertex_colors,
            extra_uvs: self.extra_uvs,
        }
    }
}

/// 单个资产的导出失败。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchFailure {
    /// 资产路径。
    pub asset: String,
    /// 错误信息。
    pub message: String,
}

/// 已导出的资产。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedAsset {
    /// 资产路径。
    pub asset: String,
    /// 写出的文件。
    pub files: Vec<PathBuf>,
    /// 顶点数。
    pub vertices: usize,
    /// 三角形数。
    pub triangles: usize,
    /// 骨骼数。
    pub bones: usize,
}

/// 批量导出报告。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchExportReport {
    /// 成功导出的资产。
    pub exported: Vec<ExportedAsset>,
    /// 跳过的资产（非网格/已存在/过滤）。
    pub skipped: usize,
    /// 失败明细。
    pub failures: Vec<BatchFailure>,
    /// 是否因取消而提前结束。
    pub cancelled: bool,
    /// 总耗时（秒）。
    pub elapsed_seconds: f64,
}

impl BatchExportReport {
    /// 写出的文件总数。
    pub fn file_count(&self) -> usize {
        self.exported.iter().map(|asset| asset.files.len()).sum()
    }

    /// 序列化为 JSON 报告。
    pub fn to_json_pretty(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|error| format!("序列化导出报告失败：{error}"))
    }
}

/// 进度事件。
#[derive(Debug, Clone)]
pub enum BatchProgress {
    /// 开始分析某个资产。
    Analyzing {
        /// 当前序号（1 起）。
        index: usize,
        /// 总数。
        total: usize,
        /// 资产路径。
        asset: String,
    },
    /// 资产已导出。
    Exported {
        /// 资产路径。
        asset: String,
        /// 写出的文件。
        files: Vec<PathBuf>,
    },
    /// 资产被跳过。
    Skipped {
        /// 资产路径。
        asset: String,
        /// 原因。
        reason: String,
    },
    /// 资产失败。
    Failed {
        /// 资产路径。
        asset: String,
        /// 错误信息。
        message: String,
    },
    /// 批量任务被取消。
    Cancelled {
        /// 取消时正在处理的资产。
        asset: String,
    },
}

/// 批量导出（不可取消）。
pub fn export_archive(
    source: &mut GameAssetSource,
    mappings: &Mappings,
    config: &BatchExportConfig,
    progress: impl FnMut(BatchProgress),
) -> Result<BatchExportReport, String> {
    export_archive_cancellable(source, mappings, config, None, progress)
}

/// 批量导出，可传入取消标志（任务面板/UI 使用）。
pub fn export_archive_cancellable(
    source: &mut GameAssetSource,
    mappings: &Mappings,
    config: &BatchExportConfig,
    cancel: Option<&AtomicBool>,
    mut progress: impl FnMut(BatchProgress),
) -> Result<BatchExportReport, String> {
    let started = Instant::now();
    let config = config.clone().normalized();
    std::fs::create_dir_all(&config.output_dir)
        .map_err(|error| format!("创建输出目录失败：{error}"))?;

    let filter = config
        .asset_filter
        .as_ref()
        .map(|value| value.to_ascii_lowercase());
    let mut candidates: Vec<String> = source
        .list()
        .into_iter()
        .map(|entry| entry.path)
        .filter(|path| path.to_ascii_lowercase().ends_with(".uasset"))
        .filter(|path| {
            filter
                .as_ref()
                .is_none_or(|filter| path.to_ascii_lowercase().contains(filter))
        })
        .collect();
    candidates.sort();
    if let Some(limit) = config.limit {
        candidates.truncate(limit);
    }
    let total = candidates.len();
    let mut report = BatchExportReport::default();

    for (position, asset) in candidates.into_iter().enumerate() {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            report.cancelled = true;
            progress(BatchProgress::Cancelled {
                asset: asset.clone(),
            });
            break;
        }
        progress(BatchProgress::Analyzing {
            index: position + 1,
            total,
            asset: asset.clone(),
        });
        let targets: Vec<PathBuf> = config
            .formats
            .iter()
            .map(|format| {
                config
                    .output_dir
                    .join(relative_output_path(&asset, format.extension()))
            })
            .collect();
        if !config.overwrite && targets.iter().all(|path| path.is_file()) {
            report.skipped += 1;
            progress(BatchProgress::Skipped {
                asset,
                reason: "输出文件已存在".to_string(),
            });
            continue;
        }

        let bundle = match build_mesh_export(
            source,
            &asset,
            mappings,
            config.include_materials,
            config.max_texture_size,
        ) {
            Ok(bundle) => bundle,
            Err(message) => {
                if message.contains("没有 StaticMesh/SkeletalMesh") {
                    report.skipped += 1;
                    progress(BatchProgress::Skipped {
                        asset,
                        reason: message,
                    });
                } else {
                    progress(BatchProgress::Failed {
                        asset: asset.clone(),
                        message: message.clone(),
                    });
                    report.failures.push(BatchFailure { asset, message });
                }
                continue;
            }
        };

        let options = config.export_options();
        let mut files = Vec::new();
        let mut failure: Option<String> = None;
        for format in &config.formats {
            let relative = relative_output_path(&asset, format.extension());
            let path = config.output_dir.join(&relative);
            let result = write_format(*format, &bundle.mesh, &bundle.materials, &options, &path);
            match result {
                Ok(()) => files.push(path),
                Err(message) => {
                    failure = Some(format!("导出 {} 失败：{message}", format.label()));
                    break;
                }
            }
        }
        if let Some(message) = failure {
            progress(BatchProgress::Failed {
                asset: asset.clone(),
                message: message.clone(),
            });
            report.failures.push(BatchFailure { asset, message });
            continue;
        }
        progress(BatchProgress::Exported {
            asset: asset.clone(),
            files: files.clone(),
        });
        report.exported.push(ExportedAsset {
            asset,
            files,
            vertices: bundle.mesh.vertices.len(),
            triangles: bundle.mesh.triangle_count(),
            bones: bundle.mesh.bones.len(),
        });
    }
    report.elapsed_seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

/// 写一个格式到路径，并落盘 USDA 旁挂资源。
pub(crate) fn write_format(
    format: BatchExportFormat,
    mesh: &lumark_export::ExportMesh,
    materials: &[lumark_export::ExportMaterial],
    options: &ExportOptions,
    path: &Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
    }
    match format {
        BatchExportFormat::Glb => {
            let data = write_glb(&GltfExport::new(mesh.clone(), materials.to_vec()), options)
                .map_err(|error| error.to_string())?;
            write_bytes(path, &data)
        }
        BatchExportFormat::UeModel => {
            let data = write_ueformat_model(mesh, options).map_err(|error| error.to_string())?;
            write_bytes(path, &data)
        }
        BatchExportFormat::Psk => {
            let data = write_psk(mesh, options).map_err(|error| error.to_string())?;
            write_bytes(path, &data)
        }
        BatchExportFormat::Usda => {
            let output = write_usda(mesh, materials, options).map_err(|error| error.to_string())?;
            write_bytes(path, output.text.as_bytes())?;
            let base = path.parent().unwrap_or_else(|| Path::new("."));
            for resource in &output.resources {
                let target = safe_join(base, &resource.relative_path)?;
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("创建贴图目录失败：{error}"))?;
                }
                write_bytes(&target, &resource.bytes)?;
            }
            Ok(())
        }
    }
}

/// 写字节到文件。
fn write_bytes(path: &Path, data: &[u8]) -> Result<(), String> {
    std::fs::write(path, data).map_err(|error| format!("写入 {} 失败：{error}", path.display()))
}

/// 资产路径 → 输出相对路径（保留目录结构、替换扩展名、拒绝路径穿越）。
fn relative_output_path(asset: &str, extension: &str) -> PathBuf {
    let without_extension = {
        let lower = asset.to_ascii_lowercase();
        if lower.ends_with(".uasset") {
            &asset[..asset.len() - ".uasset".len()]
        } else {
            asset
        }
    };
    let mut path = PathBuf::new();
    for component in without_extension.split(['/', '\\']) {
        if component.is_empty() || component == "." || component == ".." {
            continue;
        }
        path.push(sanitize_component(component));
    }
    if path.as_os_str().is_empty() {
        path.push("asset");
    }
    path.set_extension(extension);
    path
}

/// 相对路径安全拼接（拒绝绝对路径与 `..`）。
fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = base.to_path_buf();
    for component in relative.split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." || component.contains(':') || Path::new(component).is_absolute() {
            return Err(format!("非法资源路径：{relative}"));
        }
        path.push(component);
    }
    Ok(path)
}

/// 文件名/目录名清洗。
fn sanitize_component(component: &str) -> String {
    let sanitized: String = component
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "_".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 相对路径保留目录结构并替换扩展名。
    #[test]
    fn maps_asset_paths_to_output() {
        assert_eq!(
            relative_output_path("Game/Content/Meshes/SM_A.uasset", "glb"),
            PathBuf::from("Game/Content/Meshes/SM_A.glb")
        );
        assert_eq!(
            relative_output_path("Game\\Content\\SM_B.UASSET", "uemodel"),
            PathBuf::from("Game/Content/SM_B.uemodel")
        );
        assert_eq!(
            relative_output_path("../../etc/passwd.uasset", "psk"),
            PathBuf::from("etc/passwd.psk")
        );
    }

    /// 非法相对资源路径被拒绝。
    #[test]
    fn rejects_unsafe_resource_paths() {
        assert!(safe_join(Path::new("out"), "../evil.png").is_err());
        assert!(safe_join(Path::new("out"), "C:/evil.png").is_err());
        assert_eq!(
            safe_join(Path::new("out"), "textures/a.png").unwrap(),
            PathBuf::from("out/textures/a.png")
        );
    }

    /// 配置 JSON 往返与默认值补齐。
    #[test]
    fn config_round_trip() {
        let config = BatchExportConfig {
            output_dir: PathBuf::from("D:/export"),
            formats: vec![BatchExportFormat::Glb, BatchExportFormat::Usda],
            scale: 0.01,
            include_materials: false,
            asset_filter: Some("props".to_string()),
            limit: Some(10),
            overwrite: true,
            ..BatchExportConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: BatchExportConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, config);

        let partial: BatchExportConfig =
            serde_json::from_str(r#"{"output_dir":"out","formats":["glb","psk"]}"#).unwrap();
        assert_eq!(partial.output_dir, PathBuf::from("out"));
        assert_eq!(
            partial.formats,
            vec![BatchExportFormat::Glb, BatchExportFormat::Psk]
        );
        assert!(partial.include_materials);
    }

    /// 格式扩展名/标签与规范化排序去重。
    #[test]
    fn formats_and_normalization() {
        assert_eq!(BatchExportFormat::UeModel.extension(), "uemodel");
        assert_eq!(BatchExportFormat::Psk.label(), "ActorX PSK");
        let config = BatchExportConfig {
            formats: vec![
                BatchExportFormat::Psk,
                BatchExportFormat::Glb,
                BatchExportFormat::Glb,
            ],
            scale: 0.0,
            ..BatchExportConfig::default()
        }
        .normalized();
        assert_eq!(
            config.formats,
            vec![BatchExportFormat::Glb, BatchExportFormat::Psk]
        );
        assert!(config.scale > 0.0);
    }

    /// 报告可序列化为 JSON，包含取消/耗时/失败。
    #[test]
    fn report_serializes_to_json() {
        let report = BatchExportReport {
            exported: vec![ExportedAsset {
                asset: "Game/SM_A.uasset".to_string(),
                files: vec![PathBuf::from("out/Game/SM_A.glb")],
                vertices: 3,
                triangles: 1,
                bones: 0,
            }],
            skipped: 2,
            failures: vec![BatchFailure {
                asset: "Game/SM_B.uasset".to_string(),
                message: "读取失败".to_string(),
            }],
            cancelled: true,
            elapsed_seconds: 1.25,
        };
        let json: serde_json::Value =
            serde_json::from_str(&report.to_json_pretty().expect("JSON")).expect("解析 JSON");
        assert_eq!(json["skipped"], 2);
        assert_eq!(json["cancelled"], true);
        assert_eq!(json["exported"][0]["vertices"], 3);
        assert_eq!(json["failures"][0]["asset"], "Game/SM_B.uasset");
        assert!((json["elapsed_seconds"].as_f64().unwrap() - 1.25).abs() < 1e-6);
    }
}
