//! 游戏容器 UE 资产预览：IoStore/PAK 读取与纹理/模型/骨骼渲染。

use std::path::PathBuf;

use crate::i18n::tr;
use lumark_archive::{ArchiveService, ExtractionOptions, IoStoreContainer};
use lumark_unreal::{
    AssetKind, AssetMetadata, LegacyPackage, Mappings, PropValue, ZenScriptObjects,
    is_legacy_package, parse_legacy_summary, parse_package, read_legacy_anim_sequence,
    read_legacy_skeletal_mesh_geometry, read_legacy_skeletal_mesh_summary, read_legacy_skeleton,
    read_legacy_texture, render_skeletal_mesh_orbit, render_skeleton_orbit, render_static_mesh,
};
use uuid::Uuid;

/// UE 资产预览结果。
pub(super) enum UePreviewResult {
    /// 已解码的纹理缩略图。
    Texture {
        path: PathBuf,
        name: String,
        subtitle: String,
        /// 旧式包无映射/有映射时的只读字段行。
        rows: Vec<(String, String)>,
    },
    /// 可识别但不是纹理的资产。
    Other { name: String, class_name: String },
    /// 已渲染的静态模型缩略图。
    Model {
        path: PathBuf,
        name: String,
        subtitle: String,
        mesh: lumark_unreal::MeshLod,
        /// StaticMesh 的全部可切换 LOD。
        lods: Vec<lumark_unreal::MeshLod>,
        /// SkeletalMesh 对应的参考骨骼。
        skeleton: Option<lumark_unreal::Skeleton>,
        /// SkeletalMesh 对应的逐 LOD 蒙皮权重。
        skins: Vec<Option<lumark_unreal::SkinWeights>>,
        /// 只读字段行（无版本属性）。
        rows: Vec<(String, String)>,
    },
    /// 已渲染的骨骼线条图。
    Skeleton {
        path: PathBuf,
        name: String,
        subtitle: String,
        skeleton: lumark_unreal::Skeleton,
        /// 只读字段行（无版本属性）。
        rows: Vec<(String, String)>,
    },
    /// 可展示的资产元数据。
    Metadata {
        name: String,
        class_name: String,
        rows: Vec<(String, String)>,
        timeline_seconds: Option<f32>,
        /// 若当前资产是 AnimSequence，携带可播放的压缩动画数据。
        animation: Option<lumark_unreal::AnimSequenceData>,
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
    let (data, ubulk, uexp, scripts) =
        match read_ue_asset_inputs(archive, global, relative, aes_key) {
            Ok(input) => input,
            Err(error) => return UePreviewResult::Error(error),
        };
    // UE4 旧式纹理：先用原生平台数据启发式解码基础 mip；失败再展示包级元数据。
    if is_legacy_package(&data) {
        if let Some(uexp) = &uexp
            && let Ok(texture) = read_legacy_texture(&data, uexp, ubulk.as_deref())
        {
            let rgba = match texture.decode_mip_rgba(0) {
                Ok(rgba) => rgba,
                Err(error) => return UePreviewResult::Error(error.to_string()),
            };
            let Some(image) =
                image::RgbaImage::from_raw(texture.width as u32, texture.height as u32, rgba)
            else {
                return UePreviewResult::Error("旧式纹理 RGBA 尺寸不匹配".to_string());
            };
            if let Some(parent) = output.parent()
                && let Err(error) = std::fs::create_dir_all(parent)
            {
                return UePreviewResult::Error(error.to_string());
            }
            if let Err(error) = image.save(output) {
                return UePreviewResult::Error(error.to_string());
            }
            return UePreviewResult::Texture {
                path: output.to_path_buf(),
                name: texture.name,
                subtitle: {
                    let fields = lumark_unreal::read_legacy_property_header(uexp)
                        .map(|header| {
                            format!(
                                " · {} 属性/{} 字节",
                                header.property_values, header.header_bytes
                            )
                        })
                        .unwrap_or_default();
                    format!(
                        "{} · {}×{} · legacy{}",
                        texture.pixel_format, texture.width, texture.height, fields
                    )
                },
                rows: legacy_property_rows(&data, uexp, mappings_path),
            };
        }
        if let (Some(uexp), Some(mappings_path)) = (&uexp, mappings_path)
            && let Ok(package) = LegacyPackage::parse(&data, uexp)
            && package
                .exports()
                .first()
                .is_some_and(|export| export.class_name == "Skeleton")
            && let Ok(mappings) = Mappings::parse_file(mappings_path)
            && let Ok(skeleton) = read_legacy_skeleton(&package, 0, &mappings)
        {
            let rgba = render_skeleton_orbit(&skeleton, 480, 360, 35.0, 25.0, 1.0);
            if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
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
                    name: skeleton.name.clone(),
                    subtitle: format!("Skeleton · {} 根参考骨骼", skeleton.bones.len()),
                    rows: legacy_property_rows(&data, uexp, Some(mappings_path)),
                    skeleton,
                };
            }
        }
        if let (Some(uexp), Some(mappings_path)) = (&uexp, mappings_path)
            && let Ok(package) = LegacyPackage::parse(&data, uexp)
            && package
                .exports()
                .first()
                .is_some_and(|export| export.class_name == "SkeletalMesh")
            && let Ok(mappings) = Mappings::parse_file(mappings_path)
        {
            if let Ok(geometry) = read_legacy_skeletal_mesh_geometry(&package, 0, &mappings) {
                let rgba = render_skeletal_mesh_orbit(
                    &geometry.mesh,
                    &geometry.skeleton,
                    480,
                    360,
                    35.0,
                    25.0,
                    1.0,
                );
                if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
                    if let Some(parent) = output.parent()
                        && let Err(error) = std::fs::create_dir_all(parent)
                    {
                        return UePreviewResult::Error(error.to_string());
                    }
                    if let Err(error) = image.save(output) {
                        return UePreviewResult::Error(error.to_string());
                    }
                    let subtitle = format!(
                        "SkeletalMesh · {} 根参考骨骼 · {} LOD · {} section · {} 顶点 · {} 三角形",
                        geometry.skeleton.bones.len(),
                        geometry.lod_count,
                        geometry.section_count,
                        geometry.mesh.positions.len(),
                        geometry.mesh.triangle_count()
                    );
                    return UePreviewResult::Model {
                        path: output.to_path_buf(),
                        name: parse_legacy_summary(&data)
                            .map(|summary| summary.export_name)
                            .unwrap_or_else(|_| "SkeletalMesh".to_string()),
                        subtitle,
                        lods: vec![geometry.mesh.clone()],
                        mesh: geometry.mesh,
                        skeleton: Some(geometry.skeleton),
                        skins: vec![geometry.skin],
                        rows: legacy_property_rows(&data, uexp, Some(mappings_path)),
                    };
                }
            }
            if let Ok(summary) = read_legacy_skeletal_mesh_summary(&package, 0, &mappings) {
                let skeleton = summary.skeleton;
                let rgba = render_skeleton_orbit(&skeleton, 480, 360, 35.0, 25.0, 1.0);
                if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
                    if let Some(parent) = output.parent()
                        && let Err(error) = std::fs::create_dir_all(parent)
                    {
                        return UePreviewResult::Error(error.to_string());
                    }
                    if let Err(error) = image.save(output) {
                        return UePreviewResult::Error(error.to_string());
                    }
                    let sections = summary.section_counts.first().copied().unwrap_or_default();
                    return UePreviewResult::Skeleton {
                        path: output.to_path_buf(),
                        name: skeleton.name.clone(),
                        subtitle: format!(
                            "SkeletalMesh · {} 根参考骨骼 · {} LOD · {} section",
                            skeleton.bones.len(),
                            summary.lod_count,
                            sections
                        ),
                        rows: legacy_property_rows(&data, uexp, Some(mappings_path)),
                        skeleton,
                    };
                }
            }
        }
        if let (Some(uexp), Some(mappings_path)) = (&uexp, mappings_path)
            && let Ok(package) = LegacyPackage::parse(&data, uexp)
            && let Some(export) = package
                .exports()
                .first()
                .filter(|export| export.class_name == "AnimSequence")
            && let Ok(mappings) = Mappings::parse_file(mappings_path)
            && let Ok(properties) = package.read_export_properties(0, &mappings)
        {
            let timeline_seconds = properties.properties.iter().find_map(|(name, value)| {
                match (name.as_str(), value) {
                    ("SequenceLength", PropValue::Float(seconds)) => Some(*seconds as f32),
                    _ => None,
                }
            });
            // FF7R 等旧式包使用 AnimEncoding_ConstantKeyLerp 压缩；解码失败时仍
            // 保留元数据，只是查看器里不能播放。
            let animation = read_legacy_anim_sequence(&package, 0, &mappings).ok();
            return UePreviewResult::Metadata {
                name: export.name.clone(),
                class_name: export.class_name.clone(),
                rows: legacy_property_rows(&data, uexp, Some(mappings_path)),
                timeline_seconds,
                animation,
            };
        }
        if let (Some(uexp), Some(mappings_path)) = (&uexp, mappings_path)
            && let Ok(package) = LegacyPackage::parse(&data, uexp)
            && let Some(export) = package
                .exports()
                .first()
                .filter(|export| lumark_unreal::is_material_class(&export.class_name))
        {
            return UePreviewResult::Metadata {
                name: export.name.clone(),
                class_name: export.class_name.clone(),
                rows: legacy_material_rows(
                    &data,
                    uexp,
                    Some(mappings_path),
                    &export.class_name,
                    &export.name,
                ),
                timeline_seconds: None,
                animation: None,
            };
        }
        if let Ok(summary) = parse_legacy_summary(&data) {
            let mut rows = vec![
                ("包".to_string(), summary.package_name),
                ("导出数".to_string(), summary.export_count.to_string()),
                ("导入数".to_string(), summary.import_count.to_string()),
                ("名称数".to_string(), summary.name_count.to_string()),
                (
                    "说明".to_string(),
                    tr(
                        "UE4 旧式 unversioned 包；字段名需要该游戏的 .usmap",
                        "UE4 legacy unversioned package; property names need the game .usmap",
                    )
                    .to_string(),
                ),
            ];
            if let Some(uexp) = &uexp {
                let fields = legacy_property_rows(&data, uexp, mappings_path);
                if fields.is_empty() {
                    if mappings_path.is_some() {
                        rows.push((
                            "字段".to_string(),
                            tr(
                                "当前映射未能解析该资产的属性表",
                                "The current mapping could not parse this asset's properties",
                            )
                            .to_string(),
                        ));
                    }
                } else {
                    rows.push(("字段".to_string(), format!("{} 项", fields.len())));
                    rows.extend(fields);
                }
            }
            return UePreviewResult::Metadata {
                name: summary.export_name,
                class_name: summary.class_name,
                rows,
                timeline_seconds: None,
                animation: None,
            };
        }
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
            animation: None,
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
                    "StaticMesh · {} LOD · {} 顶点 · {} 三角形",
                    mesh.lod_count,
                    mesh.lod.positions.len(),
                    mesh.lod.triangle_count()
                ),
                mesh: mesh.lod,
                lods: mesh.lods,
                skeleton: None,
                skins: Vec::new(),
                rows: zen_property_rows(&package, primary, &mappings),
            };
        }
        if matches!(export.class_name.as_str(), "Skeleton" | "SkeletalMesh") {
            let preview = if export.class_name == "Skeleton" {
                package.read_skeleton(primary, &mappings).map(|skeleton| {
                    (
                        skeleton,
                        0,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                    )
                })
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
                            preview.skin,
                            preview.lods,
                            preview.lod_skins,
                            preview.material_names,
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
                skin,
                lods,
                skins,
                material_names,
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
                    if !material_names.is_empty() {
                        subtitle.push_str(&format!(
                            " · 材质 {}",
                            material_names
                                .iter()
                                .take(4)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    if lod_vertices > 0 {
                        subtitle
                            .push_str(&format!(" · {lod_vertices} 顶点 · {lod_triangles} 三角形"));
                    }
                    if let Some(skin) = &skin {
                        subtitle.push_str(&format!(
                            " · 蒙皮 {} 顶点/{} 影响",
                            skin.vertices, skin.max_influences
                        ));
                    }
                }
                if let Some(mesh) = geometry {
                    let rgba =
                        render_skeletal_mesh_orbit(&mesh, &skeleton, 480, 360, 35.0, 25.0, 1.0);
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
                        lods,
                        skeleton: Some(skeleton),
                        skins,
                        rows: zen_property_rows(&package, primary, &mappings),
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
                    rows: zen_property_rows(&package, primary, &mappings),
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
        let mut rows = rows;
        rows.extend(zen_property_rows(&package, primary, &mappings));
        let name = export.name.clone();
        let class_name = export.class_name.clone();
        let animation = if class_name == "AnimSequence" {
            package.read_anim_sequence(primary, &mappings).ok()
        } else {
            None
        };
        // 材质（含材质函数/材质表达式）没有纹理或网格预览，但可以展示类、
        // 引用资源和可解析属性，避免只显示“暂不支持”。
        if lumark_unreal::is_material_class(&class_name) {
            let mut rows = vec![
                ("材质类".to_string(), class_name.clone()),
                ("对象".to_string(), name.clone()),
                (
                    "说明".to_string(),
                    tr(
                        "材质着色器/参数为引擎自定义布局，暂不渲染材质预览；可查看引用资源与可解析属性",
                        "Material shaders/parameters use an engine-specific layout; material preview is not rendered yet",
                    )
                    .to_string(),
                ),
            ];
            rows.extend(zen_property_rows(&package, primary, &mappings));
            return UePreviewResult::Metadata {
                name,
                class_name,
                rows,
                timeline_seconds: None,
                animation: None,
            };
        }
        return if rows.is_empty() {
            UePreviewResult::Other { name, class_name }
        } else {
            UePreviewResult::Metadata {
                name,
                class_name,
                rows,
                timeline_seconds,
                animation,
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
        rows: zen_property_rows(&package, texture_index, &mappings),
    }
}

/// 读取 Zen 导出的无版本属性并格式化为详情行。
fn zen_property_rows(
    package: &lumark_unreal::UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Vec<(String, String)> {
    let Ok(properties) = package.read_export_properties(export_index, mappings) else {
        return Vec::new();
    };
    properties
        .properties
        .iter()
        .take(40)
        .map(|(name, value)| (name.clone(), format_prop_value(value)))
        .collect()
}

/// 资产读取结果：包字节、`.ubulk` 侧载和脚本对象。
type UeAssetInputs = (
    Vec<u8>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<ZenScriptObjects>,
);

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
    let uexp_relative = relative
        .strip_suffix(".uasset")
        .map(|stem| format!("{stem}.uexp"));
    if extension == "utoc" {
        let container =
            IoStoreContainer::open(archive, aes_key).map_err(|error| error.to_string())?;
        let data = container
            .read_file(relative)
            .map_err(|error| error.to_string())?;
        let ubulk = ubulk_relative.and_then(|path| container.read_file(&path).ok());
        let uexp = uexp_relative.and_then(|path| container.read_file(&path).ok());
        return Ok((data, ubulk, uexp, scripts));
    }
    if extension == "pak" {
        let temp = std::env::temp_dir().join(format!("lumark-pak-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
        let mut selected = vec![relative.to_string()];
        if let Some(ubulk) = &ubulk_relative {
            selected.push(ubulk.clone());
        }
        if let Some(uexp) = &uexp_relative {
            selected.push(uexp.clone());
        }
        // 加密 PAK 的目录索引和条目共用同一个 AES 密钥；列表阶段已经用它，
        // 预览解压也必须带上，否则每个资产都会误报“需要密码”。
        let password = aes_key.map(|key| zeroize::Zeroizing::new(key.to_string()));
        let mut options = ExtractionOptions {
            output_dir: temp.clone(),
            selected_paths: selected,
            overwrite: true,
            password,
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
        let uexp = uexp_relative
            .as_ref()
            .and_then(|path| std::fs::read(temp.join(path)).ok());
        let _ = std::fs::remove_dir_all(&temp);
        return data.map(|data| (data, ubulk, uexp, scripts));
    }
    Err(format!("资产查看暂不支持该容器类型：{}", archive.display()))
}

/// 读取旧式包属性并格式化为详情行；无映射或解析失败时返回空。
/// 读取旧式包属性并格式化为详情行；无映射或解析失败时返回空。
fn legacy_property_rows(
    asset: &[u8],
    uexp: &[u8],
    mappings_path: Option<&std::path::Path>,
) -> Vec<(String, String)> {
    let Some((package, mappings)) = parse_legacy_asset(asset, uexp, mappings_path) else {
        return Vec::new();
    };
    let Ok(properties) = package.read_export_properties(0, &mappings) else {
        return Vec::new();
    };
    let resolve = |index: i32| package.resolve_object_name(index);
    let mut rows: Vec<(String, String)> = properties
        .properties
        .iter()
        .map(|(name, value)| (name.clone(), format_prop_value_with(value, &resolve)))
        .collect();
    if let Some(header) = lumark_unreal::read_legacy_property_header(uexp) {
        rows.push((
            "属性头".to_string(),
            format!("{} 值/{} 字节", header.property_values, header.header_bytes),
        ));
    }
    rows
}

/// 解析旧式包与映射；材质预览等需要同时使用两者。
fn parse_legacy_asset(
    asset: &[u8],
    uexp: &[u8],
    mappings_path: Option<&std::path::Path>,
) -> Option<(LegacyPackage, Mappings)> {
    let mappings_path = mappings_path?;
    let mappings = Mappings::parse_file(mappings_path).ok()?;
    let package = LegacyPackage::parse(asset, uexp).ok()?;
    Some((package, mappings))
}

/// 旧式材质资产的预览行：引用资源、可解析属性与说明。
fn legacy_material_rows(
    asset: &[u8],
    uexp: &[u8],
    mappings_path: Option<&std::path::Path>,
    class_name: &str,
    name: &str,
) -> Vec<(String, String)> {
    let mut rows = vec![
        ("材质类".to_string(), class_name.to_string()),
        ("对象".to_string(), name.to_string()),
    ];
    let note = tr(
        "材质着色器/参数为引擎自定义布局，暂不渲染材质预览；这里展示引用资源与可解析属性",
        "Material shaders/parameters use an engine-specific layout; preview is not rendered; references and parsed properties are shown",
    )
    .to_string();
    let Some((package, mappings)) = parse_legacy_asset(asset, uexp, mappings_path) else {
        rows.push(("说明".to_string(), note));
        return rows;
    };
    // 材质依赖：父材质/材质实例/纹理引用。
    let mut parents: Vec<String> = Vec::new();
    let mut textures: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    for (class_package, import_class, object_name) in package.imports() {
        if import_class == "Package" {
            if object_name.starts_with("/Game/") {
                packages.push(object_name.clone());
            }
            continue;
        }
        if object_name.starts_with("Default__") || object_name.starts_with('/') {
            continue;
        }
        if import_class.contains("Material") {
            parents.push(format!("{object_name} ({import_class})"));
        } else if import_class == "Texture2D" || import_class.ends_with("Texture2D") {
            textures.push(object_name.clone());
        }
        let _ = class_package;
    }
    parents.sort();
    parents.dedup();
    textures.sort();
    textures.dedup();
    packages.sort();
    packages.dedup();
    if !parents.is_empty() {
        rows.push(("材质依赖".to_string(), parents.len().to_string()));
        rows.extend(
            parents
                .iter()
                .take(8)
                .map(|value| ("依赖".to_string(), value.clone())),
        );
    }
    if !textures.is_empty() {
        rows.push(("纹理引用".to_string(), textures.len().to_string()));
        rows.extend(
            textures
                .iter()
                .take(12)
                .map(|value| ("纹理".to_string(), value.clone())),
        );
    }
    if !packages.is_empty() {
        rows.push(("资源引用".to_string(), packages.len().to_string()));
        rows.extend(packages.iter().take(12).map(|value| {
            let short = value.rsplit('/').next().unwrap_or(value).to_string();
            ("资源".to_string(), short)
        }));
    }
    match package.read_export_properties(0, &mappings) {
        Ok(properties) if !properties.properties.is_empty() => {
            let resolve = |index: i32| package.resolve_object_name(index);
            rows.push((
                "可解析字段".to_string(),
                format!("{} 项", properties.properties.len()),
            ));
            rows.extend(
                properties
                    .properties
                    .iter()
                    .take(24)
                    .map(|(name, value)| (name.clone(), format_prop_value_with(value, &resolve))),
            );
        }
        Ok(_) => rows.push((
            "可解析字段".to_string(),
            tr(
                "属性表为空或全部为默认值",
                "Properties are empty or all defaults",
            )
            .to_string(),
        )),
        Err(_) => rows.push((
            "可解析字段".to_string(),
            tr(
                "该材质布局与映射不一致，暂未能解析属性值",
                "This material layout does not match the mapping; properties could not be parsed",
            )
            .to_string(),
        )),
    }
    rows.push(("说明".to_string(), note));
    rows
}

/// 属性值转成一行可读文本；小型 struct/数组展开到下一层。
fn format_prop_value(value: &PropValue) -> String {
    format_prop_value_with(value, &|_| None)
}

/// 属性值转成一行可读文本，并用 `resolve` 把对象索引解析成资产名。
fn format_prop_value_with(value: &PropValue, resolve: &dyn Fn(i32) -> Option<String>) -> String {
    let text = format_prop_value_at(value, 0, resolve);
    if text.chars().count() > 160 {
        format!("{}…", text.chars().take(160).collect::<String>())
    } else {
        text
    }
}

/// 递归格式化属性值；深度超过 2 后只显示数量。
fn format_prop_value_at(
    value: &PropValue,
    depth: usize,
    resolve: &dyn Fn(i32) -> Option<String>,
) -> String {
    match value {
        PropValue::Int(value) => value.to_string(),
        PropValue::UInt(value) => value.to_string(),
        PropValue::Float(value) => format!("{value:.6}"),
        PropValue::Bool(value) => value.to_string(),
        PropValue::Name(value) | PropValue::String(value) => value.clone(),
        PropValue::Object(index) => match resolve(*index) {
            Some(name) => format!("Object({index}) {name}"),
            None if *index == 0 => "None".to_string(),
            None => format!("Object({index})"),
        },
        PropValue::Array(values) | PropValue::Set(values) => {
            if depth >= 2 || values.is_empty() {
                return format!("[{} 项]", values.len());
            }
            let shown = values
                .iter()
                .take(4)
                .map(|value| format_prop_value_at(value, depth + 1, resolve))
                .collect::<Vec<_>>();
            let suffix = if values.len() > shown.len() {
                format!(" …+{}", values.len() - shown.len())
            } else {
                String::new()
            };
            format!("[{}]", shown.join(", ") + &suffix)
        }
        PropValue::Map(values) => {
            if depth >= 2 || values.is_empty() {
                return format!("{{{} 项}}", values.len());
            }
            let shown = values
                .iter()
                .take(3)
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        format_prop_value_at(key, depth + 1, resolve),
                        format_prop_value_at(value, depth + 1, resolve)
                    )
                })
                .collect::<Vec<_>>();
            let suffix = if values.len() > shown.len() {
                format!(" …+{}", values.len() - shown.len())
            } else {
                String::new()
            };
            format!("{{{}}}", shown.join(", ") + &suffix)
        }
        PropValue::Struct(fields) => {
            if depth >= 2 || fields.is_empty() {
                return format!("{{{} 字段}}", fields.len());
            }
            let shown = fields
                .iter()
                .take(6)
                .map(|(name, value)| {
                    format!("{name}={}", format_prop_value_at(value, depth + 1, resolve))
                })
                .collect::<Vec<_>>();
            let suffix = if fields.len() > shown.len() {
                format!(" …+{}", fields.len() - shown.len())
            } else {
                String::new()
            };
            format!("{{{}}}", shown.join(", ") + &suffix)
        }
        PropValue::Zero => "0".to_string(),
        PropValue::Other => "<原始值>".to_string(),
    }
}
#[cfg(test)]
#[path = "preview_ue_tests.rs"]
mod tests;
