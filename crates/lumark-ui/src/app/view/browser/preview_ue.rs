//! 游戏容器 UE 资产预览：通过 `GameAssetSource` 读取资产并渲染纹理/模型/骨骼。

use std::path::PathBuf;

use crate::i18n::tr;
use lumark_unreal::{
    AssetKind, AssetMetadata, LegacyPackage, Mappings, PropValue, ZenScriptObjects,
    is_legacy_package, parse_legacy_summary, parse_package, read_asset_package,
    read_legacy_anim_sequence, read_legacy_skeletal_mesh_geometry,
    read_legacy_skeletal_mesh_summary, read_legacy_skeleton, read_legacy_texture,
    render_skeletal_mesh_orbit, render_skeleton_orbit,
};

mod material;

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
    /// 已渲染的近似 PBR 材质球。
    Material {
        path: PathBuf,
        name: String,
        subtitle: String,
        /// 材质依赖、字段与外观参数说明。
        rows: Vec<(String, String)>,
        /// 交互式材质查看器数据。
        viewer: Option<lumark_unreal::MaterialViewerData>,
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

/// 把单个网格 LOD 包成最小场景，供缩略图渲染。
fn mesh_preview_scene(
    name: &str,
    source: &str,
    mesh: &lumark_unreal::MeshLod,
    kind: lumark_scene::NodeKind,
) -> lumark_scene::Scene {
    use lumark_scene::{Axis, Scene, SceneNode, Transform};
    let mut scene = Scene::new(name);
    scene.source = source.to_string();
    scene.up_axis = Axis::Z;
    scene.unit_scale = lumark_unreal::UE_UNIT_SCALE;
    let handle = scene.add_mesh(lumark_unreal::convert_mesh(mesh, name));
    scene.add_node(SceneNode {
        name: name.to_string(),
        parent: None,
        transform: Transform::IDENTITY,
        mesh: Some(handle),
        materials: Vec::new(),
        visible: true,
        kind,
        metadata: Vec::new(),
    });
    scene
}

/// 用共享渲染服务渲染缩略图到 `output`，返回后端标签。
fn save_scene_thumbnail(
    scene: &lumark_scene::Scene,
    output: &std::path::Path,
) -> Result<String, String> {
    let settings = lumark_render::RenderSettings {
        width: 480,
        height: 360,
        supersample: 2,
        yaw_degrees: 35.0,
        pitch_degrees: 25.0,
        ..Default::default()
    };
    let service = crate::render_service::render_service();
    let target = service.render_target(scene, &settings)?;
    let image = image::RgbaImage::from_raw(target.width, target.height, target.rgba)
        .ok_or_else(|| "模型渲染尺寸不匹配".to_string())?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    image.save(output).map_err(|error| error.to_string())?;
    Ok(service.backend_label())
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
            let mut rows = material::legacy_material_rows(
                &data,
                uexp,
                Some(mappings_path),
                &export.class_name,
                &export.name,
            );
            if let Some(rendered) = material::load_legacy_material_preview(
                archive,
                global,
                relative,
                &data,
                uexp,
                Some(mappings_path),
                aes_key,
                output,
            ) {
                let appearance = &rendered.appearance;
                rows.push((
                    "基础色".to_string(),
                    material::format_color(appearance.base_color),
                ));
                rows.push(("金属度".to_string(), format!("{:.2}", appearance.metallic)));
                rows.push(("粗糙度".to_string(), format!("{:.2}", appearance.roughness)));
                if appearance.inherited {
                    rows.push((
                        "参数来源".to_string(),
                        tr("继承自父材质", "Inherited from parent material").to_string(),
                    ));
                }
                for (label, name) in &rendered.textures {
                    rows.push((label.clone(), name.clone()));
                }
                rows.push((
                    "外观预览".to_string(),
                    tr(
                        "近似 PBR 材质球：基础色/贴图/粗糙度来自可解析参数，未还原完整 shader 图",
                        "Approximate PBR material ball from parsed parameters/textures; the full shader graph is not reproduced",
                    )
                    .to_string(),
                ));
                return UePreviewResult::Material {
                    path: rendered.path,
                    name: export.name.clone(),
                    subtitle: tr("近似 PBR 材质球", "Approximate PBR material ball").to_string(),
                    rows,
                    viewer: Some(rendered.viewer),
                };
            }
            return UePreviewResult::Metadata {
                name: export.name.clone(),
                class_name: export.class_name.clone(),
                rows,
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
    // UE5 材质：尝试渲染近似 PBR 材质球；Zen 包不暴露贴图路径，按同目录
    // 命名约定定位贴图，失败时回退为信息预览。
    if let Some(export) = package
        .info()
        .exports
        .first()
        .filter(|export| lumark_unreal::is_material_class(&export.class_name))
    {
        let name = export.name.clone();
        let class_name = export.class_name.clone();
        let mut rows = vec![
            ("材质类".to_string(), class_name.clone()),
            ("对象".to_string(), name.clone()),
        ];
        if let Some(mappings_path) = mappings_path
            && let Ok(mappings) = Mappings::parse_file(mappings_path)
        {
            rows.extend(zen_property_rows(&package, 0, &mappings));
        }
        if let Some(rendered) = material::load_zen_material_preview(
            archive,
            global,
            relative,
            &package,
            mappings_path,
            aes_key,
            output,
        ) {
            let appearance = &rendered.appearance;
            rows.push((
                "基础色".to_string(),
                material::format_color(appearance.base_color),
            ));
            rows.push(("金属度".to_string(), format!("{:.2}", appearance.metallic)));
            rows.push(("粗糙度".to_string(), format!("{:.2}", appearance.roughness)));
            for (label, texture) in &rendered.textures {
                rows.push((label.clone(), texture.clone()));
            }
            rows.push((
                "外观预览".to_string(),
                tr(
                    "近似 PBR 材质球：基础色/贴图来自可解析参数，贴图按同目录命名推断",
                    "Approximate PBR material ball from parsed parameters; textures are guessed from sibling names",
                )
                .to_string(),
            ));
            return UePreviewResult::Material {
                path: rendered.path,
                name,
                subtitle: tr("近似 PBR 材质球", "Approximate PBR material ball").to_string(),
                rows,
                viewer: Some(rendered.viewer),
            };
        }
        rows.push((
            "说明".to_string(),
            tr(
                "材质着色器/参数为引擎自定义布局，暂未生成材质球；可查看可解析属性",
                "Material shaders/parameters use an engine-specific layout; material ball was not generated",
            )
            .to_string(),
        ));
        return UePreviewResult::Metadata {
            name,
            class_name,
            rows,
            timeline_seconds: None,
            animation: None,
        };
    }
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
            // 复用引擎无关场景 + 共享渲染服务（GPU 优先，CPU 回退）。
            let scene = mesh_preview_scene(
                &mesh.name,
                &export.name,
                &mesh.lod,
                lumark_scene::NodeKind::StaticMesh,
            );
            let backend = match save_scene_thumbnail(&scene, output) {
                Ok(backend) => backend,
                Err(error) => return UePreviewResult::Error(error),
            };
            return UePreviewResult::Model {
                path: output.to_path_buf(),
                name: export.name.clone(),
                subtitle: format!(
                    "StaticMesh · {} LOD · {} 顶点 · {} 三角形 · {}",
                    mesh.lod_count,
                    mesh.lod.positions.len(),
                    mesh.lod.triangle_count(),
                    backend
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
                    // 复用引擎无关场景 + 共享渲染服务。
                    let scene = mesh_preview_scene(
                        &skeleton.name,
                        &export.name,
                        &mesh,
                        lumark_scene::NodeKind::SkeletalMesh,
                    );
                    let backend = match save_scene_thumbnail(&scene, output) {
                        Ok(backend) => backend,
                        Err(error) => return UePreviewResult::Error(error),
                    };
                    let subtitle = format!("{subtitle} · {backend}");
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
///
/// 容器读取统一走 `lumark_unreal::read_asset_package`：PAK 不再逐资产解压到
/// 临时目录，IoStore 也不在 UI 层重复解析索引。
fn read_ue_asset_inputs(
    archive: &std::path::Path,
    global: &std::path::Path,
    relative: &str,
    aes_key: Option<&str>,
) -> Result<UeAssetInputs, String> {
    let read = read_asset_package(archive, global, relative, aes_key)
        .map_err(|error| error.to_string())?;
    Ok((
        read.inputs.uasset,
        read.inputs.ubulk,
        read.inputs.uexp,
        read.scripts,
    ))
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
