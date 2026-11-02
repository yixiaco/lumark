//! 诊断：把 StaticMesh/SkeletalMesh 导出为 USDA + 自动材质/贴图旁挂。
//!
//! 用法：`export_usda <game-root> <container> <mesh-entry> <mapping.usmap> <out.usda>`

use std::error::Error;
use std::path::Path;

use lumark_export::{
    ExportBone, ExportMaterial, ExportMesh, ExportOptions, ExportTexture, MeshSection, MeshVertex,
    write_usda,
};
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot,
    extract_key_for_game, parse_package, resolve_material_asset, resolve_material_slots, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少网格资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出 .usda")?;
    let material_override = args.next();

    let game = scan_game(Path::new(&root))?;
    let container_path = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == container_name)
        })
        .map(|container| container.path.clone())
        .ok_or("未找到指定容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到 AES 密钥")?;
    let global = container_path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;
    let mut source = GameAssetSource::open(
        &container_path,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(mapping_path.into()),
        },
    )?;
    let inputs = source.read_package(&entry)?;
    let package = parse_package(inputs.uasset, source.scripts(), None)?;
    let exports = &package.info().exports;

    let (mut mesh, mut materials) = if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = package.read_static_mesh(index, &mappings)?;
        let export = export_mesh_from_lod(
            &mesh.name,
            &mesh.lod,
            mesh.material_slots
                .iter()
                .map(|slot| slot.name.clone())
                .collect(),
        );
        let resolved = resolve_material_slots(
            &mut source,
            &entry,
            &mesh.material_slots,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        (export, export_materials(&resolved))
    } else if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::SkeletalMesh)
    {
        let preview =
            package.read_skeletal_mesh_preview(index, &mappings, inputs.ubulk.as_deref())?;
        let lod = preview
            .geometry
            .as_ref()
            .ok_or("SkeletalMesh 缺少可导出几何")?;
        let mut export =
            export_mesh_from_lod(&preview.skeleton.name, lod, preview.material_names.clone());
        export.bones = preview
            .skeleton
            .bones
            .iter()
            .map(|bone| ExportBone {
                name: bone.name.clone(),
                parent_index: bone.parent_index,
                position: [
                    bone.translation[0] as f32,
                    bone.translation[1] as f32,
                    bone.translation[2] as f32,
                ],
                rotation: [
                    bone.rotation[0] as f32,
                    bone.rotation[1] as f32,
                    bone.rotation[2] as f32,
                    bone.rotation[3] as f32,
                ],
            })
            .collect();
        if let Some(skin) = &preview.skin {
            let mut indices = Vec::with_capacity(export.vertices.len());
            let mut weights = Vec::with_capacity(export.vertices.len());
            for vertex in 0..export.vertices.len() {
                let mut vertex_indices = [0u16; 4];
                let mut vertex_weights = [0f32; 4];
                if let (Some(influences), Some(raw_weights)) =
                    (skin.influences.get(vertex), skin.weights.get(vertex))
                {
                    let total: f32 = raw_weights.iter().map(|weight| f32::from(*weight)).sum();
                    let mut slot = 0;
                    for influence in 0..raw_weights.len().min(4) {
                        let weight = f32::from(raw_weights[influence]);
                        let bone = influences[influence];
                        if weight <= 0.0 || bone as usize >= export.bones.len() || slot >= 4 {
                            continue;
                        }
                        vertex_indices[slot] = bone;
                        vertex_weights[slot] = if total > 0.0 { weight / total } else { 0.0 };
                        slot += 1;
                    }
                }
                indices.push(vertex_indices);
                weights.push(vertex_weights);
            }
            export.bone_indices = indices;
            export.bone_weights = weights;
        }
        let slots: Vec<MaterialSlot> = preview
            .material_names
            .iter()
            .cloned()
            .map(|name| MaterialSlot {
                name,
                package: None,
            })
            .collect();
        let resolved = resolve_material_slots(
            &mut source,
            &entry,
            &slots,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        (export, export_materials(&resolved))
    } else {
        return Err("资产中没有 StaticMesh/SkeletalMesh 导出".into());
    };

    // 可选：直接用指定材质资产覆盖材质槽（用于贴图旁挂验证）。
    if let Some(material_path) = material_override {
        let resolved = resolve_material_asset(
            &mut source,
            &material_path,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        materials = export_materials(&[resolved]);
    }
    mesh.object_path = entry.clone();
    let usda = write_usda(
        &mesh,
        &materials,
        &ExportOptions {
            scale: 1.0,
            ..ExportOptions::default()
        },
    )?;
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, usda.text.as_bytes())?;
    for resource in &usda.resources {
        let path = output_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&resource.relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &resource.bytes)?;
    }
    println!(
        "usda={} bytes={} resources={} verts={} tris={} bones={} materials={}",
        output,
        usda.text.len(),
        usda.resources.len(),
        mesh.vertices.len(),
        mesh.triangle_count(),
        mesh.bones.len(),
        materials.len()
    );
    Ok(())
}

/// 从 MeshLod 生成导出网格。
fn export_mesh_from_lod(
    name: &str,
    lod: &lumark_unreal::MeshLod,
    material_names: Vec<String>,
) -> ExportMesh {
    let vertices = lod
        .positions
        .iter()
        .enumerate()
        .map(|(index, position)| MeshVertex {
            position: [position[0] as f32, position[1] as f32, position[2] as f32],
            normal: lod
                .attributes
                .normals
                .get(index)
                .copied()
                .unwrap_or([0.0, 0.0, 1.0]),
            uv: lod
                .attributes
                .uvs
                .first()
                .and_then(|channel| channel.get(index))
                .copied()
                .unwrap_or([0.0, 0.0]),
            color: lod
                .attributes
                .colors
                .get(index)
                .copied()
                .unwrap_or([255; 4]),
        })
        .collect();
    let sections = lod
        .sections
        .iter()
        .map(|section| MeshSection {
            material_index: usize::try_from(section.material_index.max(0)).unwrap_or(0),
            first_index: usize::try_from(section.first_index.max(0)).unwrap_or(0),
            triangle_count: usize::try_from(section.triangle_count.max(0)).unwrap_or(0),
        })
        .collect();
    ExportMesh {
        name: name.to_string(),
        vertices,
        indices: lod.indices.clone(),
        sections,
        material_names,
        extra_uvs: lod.attributes.uvs.iter().skip(1).cloned().collect(),
        ..ExportMesh::default()
    }
}

/// 把解析后的材质转换为导出材质。
fn export_materials(resolved: &[lumark_unreal::ResolvedMaterial]) -> Vec<ExportMaterial> {
    resolved
        .iter()
        .map(|material| {
            let appearance = material
                .viewer
                .as_ref()
                .map(|viewer| &viewer.appearance)
                .or(material.appearance.as_ref());
            let mut export = ExportMaterial {
                name: material.slot.name.clone(),
                ..ExportMaterial::default()
            };
            if let Some(appearance) = appearance {
                export.base_color = [
                    appearance.base_color[0],
                    appearance.base_color[1],
                    appearance.base_color[2],
                    1.0,
                ];
                export.metallic = appearance.metallic;
                export.roughness = appearance.roughness;
                export.emissive = appearance.emissive;
            }
            if let Some(viewer) = &material.viewer {
                export.base_color_texture = viewer.base_color.as_ref().map(|image| ExportTexture {
                    name: format!("{}_BaseColor", material.slot.name),
                    width: image.width,
                    height: image.height,
                    rgba: image.rgba.clone(),
                });
                export.normal_texture = viewer.normal.as_ref().map(|image| ExportTexture {
                    name: format!("{}_Normal", material.slot.name),
                    width: image.width,
                    height: image.height,
                    rgba: image.rgba.clone(),
                });
            }
            export
        })
        .collect()
}
