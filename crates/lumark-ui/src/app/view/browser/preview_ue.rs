//! 游戏容器 UE 资产预览：IoStore/PAK 读取与纹理/模型/骨骼渲染。

use std::path::PathBuf;

use crate::i18n::tr;
use lumark_archive::{ArchiveService, ExtractionOptions, IoStoreContainer};
use lumark_unreal::{
    AssetKind, AssetMetadata, Mappings, ZenScriptObjects, is_legacy_package, parse_legacy_summary,
    parse_package, render_skeleton_orbit, render_static_mesh,
};
use uuid::Uuid;

/// UE 资产预览结果。
pub(super) enum UePreviewResult {
    /// 已解码的纹理缩略图。
    Texture {
        path: PathBuf,
        name: String,
        subtitle: String,
    },
    /// 可识别但不是纹理的资产。
    Other { name: String, class_name: String },
    /// 已渲染的静态模型缩略图。
    Model {
        path: PathBuf,
        name: String,
        subtitle: String,
        mesh: lumark_unreal::MeshLod,
    },
    /// 已渲染的骨骼线条图。
    Skeleton {
        path: PathBuf,
        name: String,
        subtitle: String,
        skeleton: lumark_unreal::Skeleton,
    },
    /// 可展示的资产元数据。
    Metadata {
        name: String,
        class_name: String,
        rows: Vec<(String, String)>,
        timeline_seconds: Option<f32>,
    },
    /// 功能/配置尚不具备。
    Unavailable(String),
    /// 解析失败。
    Error(String),
}

/// 后台加载并解码 UE 资产。
pub(super) fn load_ue_asset_preview(
    archive: &std::path::Path,
    global: &std::path::Path,
    relative: &str,
    mappings_path: Option<&std::path::Path>,
    oodle: Option<PathBuf>,
    aes_key: Option<&str>,
    output: &std::path::Path,
) -> UePreviewResult {
    if let Some(oodle) = oodle {
        let _ = lumark_archive::configure_oodle_library(Some(oodle));
    }
    let (data, ubulk, scripts) = match read_ue_asset_inputs(archive, global, relative, aes_key) {
        Ok(input) => input,
        Err(error) => return UePreviewResult::Error(error),
    };
    // UE4 旧式 unversioned 包：先展示摘要/类名，字段级解析需要映射。
    if is_legacy_package(&data)
        && let Ok(summary) = parse_legacy_summary(&data)
    {
        let rows = vec![
            ("包".to_string(), summary.package_name),
            ("导出数".to_string(), summary.export_count.to_string()),
            ("导入数".to_string(), summary.import_count.to_string()),
            ("名称数".to_string(), summary.name_count.to_string()),
            (
                "说明".to_string(),
                tr(
                    "UE4 旧式 unversioned 包；字段级预览需要该游戏的 .usmap",
                    "UE4 legacy unversioned package; field-level previews need the game .usmap",
                )
                .to_string(),
            ),
        ];
        return UePreviewResult::Metadata {
            name: summary.export_name,
            class_name: summary.class_name,
            rows,
            timeline_seconds: None,
        };
    }
    let package = match parse_package(data, scripts.as_ref(), None) {
        Ok(package) => package,
        Err(error) => {
            return UePreviewResult::Error(format!(
                "解析 UE 包失败（{}）：{error}",
                archive.display()
            ));
        }
    };
    load_ue_asset_from_package(package, mappings_path, ubulk, output)
}

/// 从已解析包渲染纹理/模型/元数据。
fn load_ue_asset_from_package(
    package: lumark_unreal::UnrealPackage,
    mappings_path: Option<&std::path::Path>,
    ubulk: Option<Vec<u8>>,
    output: &std::path::Path,
) -> UePreviewResult {
    let Some(mappings_path) = mappings_path else {
        let primary = package
            .info()
            .exports
            .iter()
            .position(|export| {
                matches!(
                    export.class_name.as_str(),
                    "AnimSequence"
                        | "AnimMontage"
                        | "Skeleton"
                        | "StaticMesh"
                        | "SkeletalMesh"
                        | "Texture2D"
                )
            })
            .unwrap_or(0);
        let Some(export) = package.info().exports.get(primary) else {
            return UePreviewResult::Unavailable("包中没有可展示的资产".to_string());
        };
        let rows = vec![
            ("包".to_string(), package.info().package_name.clone()),
            (
                "导出数".to_string(),
                package.info().exports.len().to_string(),
            ),
            ("主类".to_string(), export.class_name.clone()),
        ];
        return UePreviewResult::Metadata {
            name: export.name.clone(),
            class_name: export.class_name.clone(),
            rows,
            timeline_seconds: None,
        };
    };
    let mappings = match Mappings::parse_file(mappings_path) {
        Ok(mappings) => mappings,
        Err(error) => return UePreviewResult::Error(error.to_string()),
    };
    let Some(texture_index) = package
        .info()
        .exports
        .iter()
        .position(|export| export.kind == AssetKind::Texture)
    else {
        let primary = package
            .info()
            .exports
            .iter()
            .position(|export| {
                matches!(
                    export.class_name.as_str(),
                    "AnimSequence" | "AnimMontage" | "Skeleton" | "StaticMesh" | "SkeletalMesh"
                )
            })
            .unwrap_or(0);
        let Some(export) = package.info().exports.get(primary) else {
            return UePreviewResult::Unavailable("包中没有可展示的资产".to_string());
        };
        if export.class_name == "StaticMesh"
            && let Ok(mesh) = package.read_static_mesh(primary, &mappings)
        {
            let rgba = render_static_mesh(&mesh.lod, 480, 360);
            let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) else {
                return UePreviewResult::Error("模型渲染尺寸不匹配".to_string());
            };
            if let Some(parent) = output.parent()
                && let Err(error) = std::fs::create_dir_all(parent)
            {
                return UePreviewResult::Error(error.to_string());
            }
            if let Err(error) = image.save(output) {
                return UePreviewResult::Error(error.to_string());
            }
            return UePreviewResult::Model {
                path: output.to_path_buf(),
                name: export.name.clone(),
                subtitle: format!(
                    "StaticMesh · {} 顶点 · {} 三角形",
                    mesh.lod.positions.len(),
                    mesh.lod.triangle_count()
                ),
                mesh: mesh.lod,
            };
        }
        if matches!(export.class_name.as_str(), "Skeleton" | "SkeletalMesh") {
            let preview = if export.class_name == "Skeleton" {
                package
                    .read_skeleton(primary, &mappings)
                    .map(|skeleton| (skeleton, 0, 0, 0, 0, 0, None))
            } else {
                package
                    .read_skeletal_mesh_preview(primary, &mappings, ubulk.as_deref())
                    .map(|preview| {
                        (
                            preview.skeleton,
                            preview.lod_count,
                            preview.material_slots,
                            preview.lod_sections,
                            preview.lod_vertices,
                            preview.lod_triangles,
                            preview.geometry,
                        )
                    })
            };
            if let Ok((
                skeleton,
                lod_count,
                material_slots,
                lod_sections,
                lod_vertices,
                lod_triangles,
                geometry,
            )) = preview
            {
                let mut subtitle = format!(
                    "{} · {} 根参考骨骼",
                    export.class_name,
                    skeleton.bones.len()
                );
                if export.class_name == "SkeletalMesh" {
                    subtitle.push_str(&format!(
                        " · {lod_count} LOD · {material_slots} 材质 · {lod_sections} section"
                    ));
                    if lod_vertices > 0 {
                        subtitle
                            .push_str(&format!(" · {lod_vertices} 顶点 · {lod_triangles} 三角形"));
                    }
                }
                if let Some(mesh) = geometry {
                    let rgba = render_static_mesh(&mesh, 480, 360);
                    let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) else {
                        return UePreviewResult::Error("骨骼网格渲染尺寸不匹配".to_string());
                    };
                    if let Some(parent) = output.parent()
                        && let Err(error) = std::fs::create_dir_all(parent)
                    {
                        return UePreviewResult::Error(error.to_string());
                    }
                    if let Err(error) = image.save(output) {
                        return UePreviewResult::Error(error.to_string());
                    }
                    return UePreviewResult::Model {
                        path: output.to_path_buf(),
                        name: export.name.clone(),
                        subtitle,
                        mesh,
                    };
                }
                let rgba = render_skeleton_orbit(&skeleton, 480, 360, 35.0, 25.0, 1.0);
                let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) else {
                    return UePreviewResult::Error("骨骼渲染尺寸不匹配".to_string());
                };
                if let Some(parent) = output.parent()
                    && let Err(error) = std::fs::create_dir_all(parent)
                {
                    return UePreviewResult::Error(error.to_string());
                }
                if let Err(error) = image.save(output) {
                    return UePreviewResult::Error(error.to_string());
                }
                return UePreviewResult::Skeleton {
                    path: output.to_path_buf(),
                    name: export.name.clone(),
                    subtitle,
                    skeleton,
                };
            }
        }
        let metadata = package
            .read_asset_metadata(primary, &mappings)
            .unwrap_or(AssetMetadata::Other);
        let timeline_seconds = match &metadata {
            AssetMetadata::Animation { length, .. } => Some(*length),
            _ => None,
        };
        let rows = metadata.rows();
        let name = export.name.clone();
        let class_name = export.class_name.clone();
        return if rows.is_empty() {
            UePreviewResult::Other { name, class_name }
        } else {
            UePreviewResult::Metadata {
                name,
                class_name,
                rows,
                timeline_seconds,
            }
        };
    };
    let texture = match package.read_texture2d(texture_index, &mappings, ubulk.as_deref()) {
        Ok(texture) => texture,
        Err(error) => return UePreviewResult::Error(error.to_string()),
    };
    let rgba = match texture.decode_mip_rgba(0) {
        Ok(rgba) => rgba,
        Err(error) => return UePreviewResult::Error(error.to_string()),
    };
    let Some(image) = image::RgbaImage::from_raw(texture.width as u32, texture.height as u32, rgba)
    else {
        return UePreviewResult::Error("纹理 RGBA 尺寸不匹配".to_string());
    };
    if let Some(parent) = output.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return UePreviewResult::Error(error.to_string());
    }
    if let Err(error) = image.save(output) {
        return UePreviewResult::Error(error.to_string());
    }
    UePreviewResult::Texture {
        path: output.to_path_buf(),
        name: texture.name,
        subtitle: format!(
            "{} · {}×{}",
            texture.pixel_format, texture.width, texture.height
        ),
    }
}

/// 资产读取结果：包字节、`.ubulk` 侧载和脚本对象。
type UeAssetInputs = (Vec<u8>, Option<Vec<u8>>, Option<ZenScriptObjects>);

/// 读取资产字节、`.ubulk` 侧载和脚本对象；支持 IoStore 和普通 PAK。
fn read_ue_asset_inputs(
    archive: &std::path::Path,
    global: &std::path::Path,
    relative: &str,
    aes_key: Option<&str>,
) -> Result<UeAssetInputs, String> {
    let extension = archive
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let scripts = global
        .is_file()
        .then(|| ZenScriptObjects::from_container(global, aes_key).ok())
        .flatten();
    let ubulk_relative = relative
        .strip_suffix(".uasset")
        .map(|stem| format!("{stem}.ubulk"));
    if extension == "utoc" {
        let container =
            IoStoreContainer::open(archive, aes_key).map_err(|error| error.to_string())?;
        let data = container
            .read_file(relative)
            .map_err(|error| error.to_string())?;
        let ubulk = ubulk_relative.and_then(|path| container.read_file(&path).ok());
        return Ok((data, ubulk, scripts));
    }
    if extension == "pak" {
        let temp = std::env::temp_dir().join(format!("lumark-pak-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
        let mut selected = vec![relative.to_string()];
        if let Some(ubulk) = &ubulk_relative {
            selected.push(ubulk.clone());
        }
        let mut options = ExtractionOptions {
            output_dir: temp.clone(),
            selected_paths: selected,
            overwrite: true,
            ..Default::default()
        };
        if ArchiveService::extract(archive, &options, |_| {}).is_err() {
            options.selected_paths = vec![relative.to_string()];
            if let Err(error) = ArchiveService::extract(archive, &options, |_| {}) {
                let _ = std::fs::remove_dir_all(&temp);
                return Err(error.to_string());
            }
        }
        let data = std::fs::read(temp.join(relative)).map_err(|error| error.to_string());
        let ubulk = ubulk_relative
            .as_ref()
            .and_then(|path| std::fs::read(temp.join(path)).ok());
        let _ = std::fs::remove_dir_all(&temp);
        return data.map(|data| (data, ubulk, scripts));
    }
    Err(format!("资产查看暂不支持该容器类型：{}", archive.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_archive::{ArchiveFormat, CompressionOptions};

    /// 使用 CUE4Parse UE5.8 fixture 验证完整纹理预览链路。
    ///
    /// 需要把 `LUMARK_UE_FIXTURE_ROOT` 指向 `CUE4Parse.Tests/Fixtures/UE5_8`；
    /// 未配置时跳过，避免测试依赖外部仓库。
    #[test]
    fn loads_fixture_assets_when_available() {
        let Ok(root) = std::env::var("LUMARK_UE_FIXTURE_ROOT") else {
            return;
        };
        let base = std::path::PathBuf::from(root);
        let toc = base.join(
            "IoStore/Unversioned/Uncompressed/CUE4ParseFixtures-Minimal-Uncompressed-Windows.utoc",
        );
        let global = base.join("IoStore/Unversioned/global.utoc");
        let mapping = base.join("Mappings/CUE4ParseFixtures-Uncompressed.usmap");
        if !toc.is_file() || !global.is_file() || !mapping.is_file() {
            return;
        }
        let temp = tempfile::tempdir().expect("temp dir");
        let output = temp.path().join("preview.png");
        let result = load_ue_asset_preview(
            &toc,
            &global,
            "CUE4ParseFixtures/Content/Fixtures/Textures/T_BC1.uasset",
            Some(&mapping),
            None,
            None,
            &output,
        );
        match result {
            UePreviewResult::Texture {
                path,
                name,
                subtitle,
            } => {
                assert_eq!(name, "T_BC1");
                assert!(subtitle.contains("PF_DXT1"));
                assert!(path.is_file());
            }
            _ => panic!("fixture texture preview failed"),
        }

        // 未选择映射时仍展示包级元数据，而不是直接失败。
        match load_ue_asset_preview(
            &toc,
            &global,
            "CUE4ParseFixtures/Content/Fixtures/Textures/T_BC1.uasset",
            None,
            None,
            None,
            &temp.path().join("metadata.png"),
        ) {
            UePreviewResult::Metadata {
                class_name, rows, ..
            } => {
                assert_eq!(class_name, "Texture2D");
                assert!(rows.iter().any(|(label, _)| label == "包"));
            }
            _ => panic!("fixture no-mapping metadata failed"),
        }

        // 普通 PAK（ZIP 容器）中的同一 Zen 包也应能预览。
        let package_bytes = IoStoreContainer::open(&toc, None)
            .expect("fixture container")
            .read_file("CUE4ParseFixtures/Content/Fixtures/Textures/T_BC1.uasset")
            .expect("fixture texture entry");
        let source_dir = temp.path().join("pak-source");
        std::fs::create_dir_all(&source_dir).expect("pak source dir");
        let source = source_dir.join("T_BC1.uasset");
        std::fs::write(&source, &package_bytes).expect("write texture package");
        let pak = temp.path().join("fixture.pak");
        ArchiveService::create(
            &pak,
            &[source],
            &CompressionOptions {
                format: ArchiveFormat::Zip,
                ..Default::default()
            },
            |_| {},
        )
        .expect("create fixture pak");
        let pak_output = temp.path().join("pak-preview.png");
        match load_ue_asset_preview(
            &pak,
            &global,
            "T_BC1.uasset",
            Some(&mapping),
            None,
            None,
            &pak_output,
        ) {
            UePreviewResult::Texture { name, path, .. } => {
                assert_eq!(name, "T_BC1");
                assert!(path.is_file());
            }
            _ => panic!("fixture pak texture preview failed"),
        }

        let model = load_ue_asset_preview(
            &toc,
            &global,
            "CUE4ParseFixtures/Content/Fixtures/Meshes/SM_Fixture.uasset",
            Some(&mapping),
            None,
            None,
            &temp.path().join("model.png"),
        );
        match model {
            UePreviewResult::Model {
                path,
                name,
                subtitle,
                ..
            } => {
                assert_eq!(name, "SM_Fixture");
                assert!(subtitle.contains("6 顶点"));
                assert!(subtitle.contains("2 三角形"));
                assert!(path.is_file());
            }
            _ => panic!("fixture static mesh preview failed"),
        }

        let skeleton = load_ue_asset_preview(
            &toc,
            &global,
            "CUE4ParseFixtures/Content/Fixtures/Animations/SKEL_Fixture.uasset",
            Some(&mapping),
            None,
            None,
            &temp.path().join("skeleton.png"),
        );
        match skeleton {
            UePreviewResult::Skeleton {
                path,
                subtitle,
                skeleton,
                ..
            } => {
                assert_eq!(skeleton.bones.len(), 3);
                assert!(subtitle.contains("3 根参考骨骼"));
                assert!(path.is_file());
            }
            _ => panic!("fixture skeleton preview failed"),
        }

        let animation = load_ue_asset_preview(
            &toc,
            &global,
            "CUE4ParseFixtures/Content/Fixtures/Animations/AS_Fixture.uasset",
            Some(&mapping),
            None,
            None,
            &temp.path().join("animation.png"),
        );
        match animation {
            UePreviewResult::Metadata {
                class_name,
                timeline_seconds,
                ..
            } => {
                assert_eq!(class_name, "AnimSequence");
                assert!(timeline_seconds.is_some_and(|length| (length - 1.0).abs() < 1.0e-4));
            }
            _ => panic!("fixture animation metadata failed"),
        }

        let skeletal = load_ue_asset_preview(
            &toc,
            &global,
            "CUE4ParseFixtures/Content/Fixtures/Meshes/SK_Fixture.uasset",
            Some(&mapping),
            None,
            None,
            &temp.path().join("skeletal.png"),
        );
        match skeletal {
            UePreviewResult::Model {
                path,
                subtitle,
                mesh,
                ..
            } => {
                assert_eq!(mesh.positions.len(), 6);
                assert!(mesh.triangle_count() > 0);
                assert!(!mesh.sections.is_empty(), "SkeletalMesh section palette");
                assert!(subtitle.contains("SkeletalMesh"));
                assert!(subtitle.contains("6 顶点"), "subtitle={subtitle}");
                assert!(path.is_file());
            }
            _ => panic!("fixture skeletal mesh geometry preview failed"),
        }
    }
}
