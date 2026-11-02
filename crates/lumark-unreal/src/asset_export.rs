//! 单资产导出编排（E-11）：解析资产 → 材质/贴图 → 多格式文件 + 材质旁挂。
//!
//! UI、命令行与批量导出共用；输出直接落在目标目录（不做容器目录镜像）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use lumark_export::{
    ExportOptions, MaterialSidecarOptions, TextureExportFormat, material_sidecars, write_resources,
};
use serde::{Deserialize, Serialize};

use crate::batch::{BatchExportFormat, write_format};
use crate::export_input::build_mesh_export;
use crate::{GameAssetSource, Mappings};

/// 单资产导出配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetExportConfig {
    /// 输出目录。
    pub output_dir: PathBuf,
    /// 目标格式。
    pub formats: Vec<BatchExportFormat>,
    /// 位置缩放。
    pub scale: f32,
    /// 是否写顶点色。
    pub vertex_colors: bool,
    /// 是否写额外 UV。
    pub extra_uvs: bool,
    /// 是否覆盖已有文件。
    pub overwrite: bool,
    /// 是否写材质贴图/JSON 旁挂。
    pub write_sidecars: bool,
    /// 贴图最大边长。
    pub max_texture_size: u32,
    /// 贴图格式。
    pub texture_format: TextureExportFormat,
    /// JPEG 质量。
    pub jpeg_quality: u8,
}

impl Default for AssetExportConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("export"),
            formats: vec![BatchExportFormat::Glb],
            scale: 1.0,
            vertex_colors: true,
            extra_uvs: true,
            overwrite: true,
            write_sidecars: true,
            max_texture_size: 512,
            texture_format: TextureExportFormat::Png,
            jpeg_quality: 90,
        }
    }
}

impl AssetExportConfig {
    /// 规范化：格式为空时回退 GLB，缩放/质量为合理值。
    pub fn normalized(mut self) -> Self {
        if self.formats.is_empty() {
            self.formats = vec![BatchExportFormat::Glb];
        }
        self.formats.sort_by_key(|format| format.extension());
        self.formats.dedup();
        self.scale = self.scale.clamp(0.0001, 100.0);
        self.jpeg_quality = self.jpeg_quality.clamp(1, 100);
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

/// 单资产导出报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetExportReport {
    /// 资产路径。
    pub asset_path: String,
    /// 写出的模型文件。
    pub files: Vec<PathBuf>,
    /// 写出的材质旁挂文件。
    pub sidecar_files: Vec<PathBuf>,
    /// 顶点数。
    pub vertices: usize,
    /// 三角形数。
    pub triangles: usize,
    /// 骨骼数。
    pub bones: usize,
    /// 是否取消。
    pub cancelled: bool,
    /// 耗时（秒）。
    pub elapsed_seconds: f64,
}

impl AssetExportReport {
    /// 序列化为 JSON。
    pub fn to_json_pretty(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|error| format!("序列化导出报告失败：{error}"))
    }
}

/// 导出进度。
#[derive(Debug, Clone)]
pub enum AssetExportProgress {
    /// 正在写某个格式。
    Writing {
        /// 格式标签。
        format: &'static str,
        /// 输出路径。
        path: PathBuf,
    },
    /// 正在写材质旁挂。
    Sidecars,
    /// 完成。
    Finished,
}

/// 导出单个资产。
pub fn export_asset(
    source: &mut GameAssetSource,
    mappings: &Mappings,
    asset_path: &str,
    config: &AssetExportConfig,
    cancel: Option<&AtomicBool>,
    mut progress: impl FnMut(AssetExportProgress),
) -> Result<AssetExportReport, String> {
    let started = Instant::now();
    let config = config.clone().normalized();
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err("导出已取消".to_string());
    }
    std::fs::create_dir_all(&config.output_dir)
        .map_err(|error| format!("创建输出目录失败：{error}"))?;
    let bundle = build_mesh_export(
        source,
        asset_path,
        mappings,
        config.write_sidecars,
        config.max_texture_size,
    )?;
    let options = config.export_options();
    let stem = asset_stem(asset_path);
    let mut files = Vec::with_capacity(config.formats.len());
    for format in &config.formats {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            return Ok(AssetExportReport {
                asset_path: asset_path.to_string(),
                files,
                sidecar_files: Vec::new(),
                vertices: bundle.mesh.vertices.len(),
                triangles: bundle.mesh.triangle_count(),
                bones: bundle.mesh.bones.len(),
                cancelled: true,
                elapsed_seconds: started.elapsed().as_secs_f64(),
            });
        }
        let path = config
            .output_dir
            .join(format!("{stem}.{}", format.extension()));
        if !config.overwrite && path.is_file() {
            return Err(format!("文件已存在：{}", path.display()));
        }
        progress(AssetExportProgress::Writing {
            format: format.label(),
            path: path.clone(),
        });
        write_format(*format, &bundle.mesh, &bundle.materials, &options, &path)?;
        files.push(path);
    }

    let mut sidecar_files = Vec::new();
    if config.write_sidecars {
        progress(AssetExportProgress::Sidecars);
        let sidecar_options = MaterialSidecarOptions {
            format: config.texture_format,
            quality: config.jpeg_quality,
        };
        let resources = material_sidecars(&bundle.materials, &sidecar_options)
            .map_err(|error| error.to_string())?;
        sidecar_files =
            write_resources(&config.output_dir, &resources).map_err(|error| error.to_string())?;
    }
    progress(AssetExportProgress::Finished);
    Ok(AssetExportReport {
        asset_path: asset_path.to_string(),
        files,
        sidecar_files,
        vertices: bundle.mesh.vertices.len(),
        triangles: bundle.mesh.triangle_count(),
        bones: bundle.mesh.bones.len(),
        cancelled: false,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    })
}

/// 资产路径 → 文件名主干。
fn asset_stem(asset_path: &str) -> String {
    asset_path
        .rsplit(['/', '\\'])
        .next()
        .and_then(|file| file.strip_suffix(".uasset").or(Some(file)))
        .map(sanitize_file_stem)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "asset".to_string())
}

/// 文件名清洗。
fn sanitize_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// 报告写入 JSON 文件。
pub fn write_report(path: &Path, report: &AssetExportReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建报告目录失败：{error}"))?;
    }
    std::fs::write(path, report.to_json_pretty()?)
        .map_err(|error| format!("写入导出报告失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 配置默认/规范化与导出选项。
    #[test]
    fn config_defaults_and_normalization() {
        let config = AssetExportConfig {
            formats: vec![
                BatchExportFormat::Psk,
                BatchExportFormat::Glb,
                BatchExportFormat::Glb,
            ],
            scale: 0.0,
            jpeg_quality: 250,
            ..AssetExportConfig::default()
        }
        .normalized();
        assert_eq!(
            config.formats,
            vec![BatchExportFormat::Glb, BatchExportFormat::Psk]
        );
        assert!(config.scale > 0.0);
        assert_eq!(config.jpeg_quality, 100);
        assert_eq!(config.export_options().scale, config.scale);
    }

    /// 资产名主干与报告 JSON。
    #[test]
    fn asset_stem_and_report_json() {
        assert_eq!(asset_stem("Game/Content/SM_A.uasset"), "SM_A");
        assert_eq!(asset_stem("Game\\SM B.uasset"), "SM_B");
        let report = AssetExportReport {
            asset_path: "Game/SM_A.uasset".to_string(),
            files: vec![PathBuf::from("out/SM_A.glb")],
            sidecar_files: vec![PathBuf::from("out/materials.json")],
            vertices: 3,
            triangles: 1,
            bones: 0,
            cancelled: false,
            elapsed_seconds: 0.5,
        };
        let json: serde_json::Value =
            serde_json::from_str(&report.to_json_pretty().expect("JSON")).expect("解析");
        assert_eq!(json["vertices"], 3);
        assert_eq!(json["sidecar_files"][0], "out/materials.json");
    }
}
