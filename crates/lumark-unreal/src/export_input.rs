//! 从 Unreal 资产构建引擎无关导出输入（批量导出与示例共用）。

use crate::{
    AssetKind, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot, ResolvedMaterial,
    SkeletalMeshPreview, parse_package, resolve_material_slots, resolve_static_materials,
};
use lumark_export::{
    ExportBone, ExportMaterial, ExportMesh, ExportTexture, MeshSection, MeshVertex,
};

/// 一个资产的导出输入：网格 + 自动解析的材质。
#[derive(Debug, Clone)]
pub struct MeshExportBundle {
    /// 网格（含骨骼/蒙皮）。
    pub mesh: ExportMesh,
    /// 材质槽解析结果；关闭材质加载时为空。
    pub materials: Vec<ExportMaterial>,
}

/// 读取资产并构建导出输入。
pub fn build_mesh_export(
    source: &mut GameAssetSource,
    entry: &str,
    mappings: &Mappings,
    include_materials: bool,
    max_texture_size: u32,
) -> Result<MeshExportBundle, String> {
    let inputs = source
        .read_package(entry)
        .map_err(|error| format!("读取资产失败：{error}"))?;
    let package = parse_package(inputs.uasset, source.scripts(), None)
        .map_err(|error| format!("解析包失败：{error}"))?;
    let exports = &package.info().exports;
    let material_options = MaterialResolveOptions {
        load_textures: include_materials,
        max_texture_size,
        ..MaterialResolveOptions::default()
    };

    if let Some(index) = exports.iter().position(|export| {
        export.kind == AssetKind::StaticMesh && export.class_name == "StaticMesh"
    }) {
        let static_mesh = package
            .read_static_mesh_with_bulk(index, mappings, inputs.ubulk.as_deref())
            .map_err(|error| format!("读取 StaticMesh 失败：{error}"))?;
        let mut mesh = mesh_from_lod(
            &static_mesh.name,
            &static_mesh.lod,
            static_mesh
                .material_slots
                .iter()
                .map(|slot| slot.name.clone())
                .collect(),
        );
        mesh.object_path = entry.to_string();
        let materials = if include_materials {
            let resolved = resolve_static_materials(
                source,
                entry,
                &static_mesh.material_slots,
                &static_mesh.material_imports,
                Some(mappings),
                &material_options,
            );
            export_materials(&resolved)
        } else {
            Vec::new()
        };
        return Ok(MeshExportBundle { mesh, materials });
    }

    if let Some(index) = exports.iter().position(|export| {
        export.kind == AssetKind::SkeletalMesh && export.class_name == "SkeletalMesh"
    }) {
        let preview = package
            .read_skeletal_mesh_preview(index, mappings, inputs.ubulk.as_deref())
            .map_err(|error| format!("读取 SkeletalMesh 失败：{error}"))?;
        let geometry = preview
            .geometry
            .as_ref()
            .ok_or_else(|| "SkeletalMesh 缺少可导出几何".to_string())?;
        let mut mesh = mesh_from_lod(
            &preview.skeleton.name,
            geometry,
            preview.material_names.clone(),
        );
        mesh.object_path = entry.to_string();
        fill_skeleton(&mut mesh, &preview);
        let materials = if include_materials {
            let slots: Vec<MaterialSlot> = preview
                .material_names
                .iter()
                .cloned()
                .map(|name| MaterialSlot {
                    name,
                    package: None,
                })
                .collect();
            let resolved =
                resolve_material_slots(source, entry, &slots, Some(mappings), &material_options);
            export_materials(&resolved)
        } else {
            Vec::new()
        };
        return Ok(MeshExportBundle { mesh, materials });
    }

    Err("资产中没有 StaticMesh/SkeletalMesh 导出".to_string())
}

/// MeshLod → 导出网格。
fn mesh_from_lod(name: &str, lod: &crate::MeshLod, material_names: Vec<String>) -> ExportMesh {
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

/// 填充骨骼与蒙皮权重。
fn fill_skeleton(mesh: &mut ExportMesh, preview: &SkeletalMeshPreview) {
    mesh.bones = preview
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
    let Some(skin) = &preview.skin else {
        return;
    };
    let mut indices = Vec::with_capacity(mesh.vertices.len());
    let mut weights = Vec::with_capacity(mesh.vertices.len());
    for vertex in 0..mesh.vertices.len() {
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
                if weight <= 0.0 || bone as usize >= mesh.bones.len() || slot >= 4 {
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
    mesh.bone_indices = indices;
    mesh.bone_weights = weights;
}

/// 材质解析结果 → 导出材质（含贴图）。
pub fn export_materials(resolved: &[ResolvedMaterial]) -> Vec<ExportMaterial> {
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
