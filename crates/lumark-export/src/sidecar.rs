//! 材质旁挂：贴图文件 + `materials.json`。
//!
//! 供批量导出/单资产导出在模型文件旁生成可被 DCC 读取的材质描述；
//! 贴图格式可配置（PNG/JPEG/TGA/WebP）。

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::ExportError;
use crate::gltf::ExportMaterial;
use crate::texture::{TextureExportFormat, TextureExportOptions, encode_texture};
use crate::usda::ExportResource;

/// 材质旁挂选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialSidecarOptions {
    /// 贴图格式。
    pub format: TextureExportFormat,
    /// JPEG 质量。
    pub quality: u8,
}

impl Default for MaterialSidecarOptions {
    fn default() -> Self {
        Self {
            format: TextureExportFormat::Png,
            quality: 90,
        }
    }
}

/// 生成材质旁挂文件（贴图 + materials.json）。
pub fn material_sidecars(
    materials: &[ExportMaterial],
    options: &MaterialSidecarOptions,
) -> Result<Vec<ExportResource>, ExportError> {
    let texture_options = TextureExportOptions {
        format: options.format,
        quality: options.quality,
    };
    let mut resources = Vec::new();
    let mut entries = Vec::with_capacity(materials.len());
    for material in materials {
        let name = sanitize_file_name(&material.name);
        let mut textures = serde_json::Map::new();
        if let Some(texture) = &material.base_color_texture {
            let path = format!("textures/{name}_BaseColor.{}", options.format.extension());
            resources.push(ExportResource {
                relative_path: path.clone(),
                bytes: encode_texture(texture, &texture_options)?,
            });
            textures.insert("baseColor".to_string(), json!(path));
        }
        if let Some(texture) = &material.normal_texture {
            let path = format!("textures/{name}_Normal.{}", options.format.extension());
            resources.push(ExportResource {
                relative_path: path.clone(),
                bytes: encode_texture(texture, &texture_options)?,
            });
            textures.insert("normal".to_string(), json!(path));
        }
        entries.push(json!({
            "name": material.name,
            "baseColor": material.base_color,
            "metallic": material.metallic,
            "roughness": material.roughness,
            "emissive": material.emissive,
            "twoSided": material.two_sided,
            "textures": textures,
        }));
    }
    let document = json!({
        "version": 1,
        "generator": "Lumark",
        "textureFormat": options.format.extension(),
        "materials": entries,
    });
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| ExportError::Serialization(error.to_string()))?;
    resources.push(ExportResource {
        relative_path: "materials.json".to_string(),
        bytes,
    });
    Ok(resources)
}

/// 把旁挂资源写到目录，返回实际文件路径。
///
/// 拒绝绝对路径与 `..`，避免写出目标目录。
pub fn write_resources(
    base_dir: &Path,
    resources: &[ExportResource],
) -> Result<Vec<PathBuf>, ExportError> {
    let mut written = Vec::with_capacity(resources.len());
    for resource in resources {
        let mut path = base_dir.to_path_buf();
        for component in resource.relative_path.split(['/', '\\']) {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." || component.contains(':') || Path::new(component).is_absolute() {
                return Err(ExportError::Serialization(format!(
                    "非法旁挂路径：{}",
                    resource.relative_path
                )));
            }
            path.push(component);
        }
        crate::write_atomic(&path, &resource.bytes)?;
        written.push(path);
    }
    Ok(written)
}

/// 文件名清洗（保留字母/数字/下划线/连字符/空格）。
fn sanitize_file_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim().is_empty() {
        "Material".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf::{ExportMaterial, ExportTexture};

    /// 材质旁挂包含贴图与 materials.json。
    #[test]
    fn writes_material_sidecars() {
        let material = ExportMaterial {
            name: "M-Test".to_string(),
            base_color_texture: Some(ExportTexture {
                name: "T_Base".to_string(),
                width: 1,
                height: 1,
                rgba: vec![255, 0, 0, 255],
            }),
            ..ExportMaterial::default()
        };
        let resources = material_sidecars(
            &[material],
            &MaterialSidecarOptions {
                format: TextureExportFormat::Png,
                quality: 90,
            },
        )
        .expect("旁挂");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].relative_path, "textures/M-Test_BaseColor.png");
        assert_eq!(&resources[0].bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(resources[1].relative_path, "materials.json");
        let document: serde_json::Value =
            serde_json::from_slice(&resources[1].bytes).expect("JSON");
        assert_eq!(document["version"], 1);
        assert_eq!(document["textures"].as_object(), None);
        assert_eq!(
            document["materials"][0]["textures"]["baseColor"],
            "textures/M-Test_BaseColor.png"
        );
    }

    /// 写出旁挂文件并拒绝路径穿越。
    #[test]
    fn writes_resources_and_rejects_escape() {
        let directory = tempfile::tempdir().expect("临时目录");
        let resources = vec![ExportResource {
            relative_path: "textures/T.png".to_string(),
            bytes: vec![1, 2, 3],
        }];
        let written = write_resources(directory.path(), &resources).expect("写入");
        assert_eq!(written.len(), 1);
        assert!(written[0].is_file());
        assert!(
            write_resources(
                directory.path(),
                &[ExportResource {
                    relative_path: "../evil.txt".to_string(),
                    bytes: vec![0],
                }]
            )
            .is_err()
        );
    }

    /// 无贴图时仍写 materials.json。
    #[test]
    fn writes_plain_materials_json() {
        let resources = material_sidecars(
            &[ExportMaterial {
                name: "M_Plain".to_string(),
                ..ExportMaterial::default()
            }],
            &MaterialSidecarOptions::default(),
        )
        .expect("旁挂");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].relative_path, "materials.json");
    }
}
