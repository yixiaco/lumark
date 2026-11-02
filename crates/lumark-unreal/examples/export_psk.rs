//! 诊断：把 StaticMesh/SkeletalMesh 导出为 ActorX PSK。
//!
//! 用法：`export_psk <game-root> <container> <mesh-entry> <mapping.usmap> <out.psk>`

use std::error::Error;
use std::path::Path;

use lumark_export::{ExportBone, ExportMesh, ExportOptions, MeshSection, MeshVertex, write_psk};
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, extract_key_for_game, parse_package,
    scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少网格资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出 .psk")?;
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

    let export = if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = package.read_static_mesh(index, &mappings)?;
        export_mesh_from_lod(
            &mesh.name,
            &mesh.lod,
            mesh.material_slots
                .iter()
                .map(|slot| slot.name.clone())
                .collect(),
        )
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
        export
    } else {
        return Err("资产中没有 StaticMesh/SkeletalMesh 导出".into());
    };

    let data = write_psk(&export, &ExportOptions::default())?;
    if let Some(parent) = Path::new(&output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, &data)?;
    println!(
        "psk={} bytes={} verts={} tris={} bones={} sections={} chunks={}",
        output,
        data.len(),
        export.vertices.len(),
        export.triangle_count(),
        export.bones.len(),
        export.sections.len(),
        chunk_names(&data).join(",")
    );
    Ok(())
}

/// 从 MeshLod 生成导出网格（静态部分）。
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
    let extra_uvs = lod.attributes.uvs.iter().skip(1).cloned().collect();
    ExportMesh {
        name: name.to_string(),
        vertices,
        indices: lod.indices.clone(),
        sections,
        material_names,
        extra_uvs,
        ..ExportMesh::default()
    }
}

/// 列出 PSK chunk 名称。
fn chunk_names(data: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut position = 0usize;
    while position + 32 <= data.len() {
        let name = String::from_utf8_lossy(&data[position..position + 20])
            .trim_end_matches('\0')
            .to_string();
        let data_size = i32::from_le_bytes(
            data[position + 24..position + 28]
                .try_into()
                .unwrap_or_default(),
        );
        let count = i32::from_le_bytes(
            data[position + 28..position + 32]
                .try_into()
                .unwrap_or_default(),
        );
        names.push(name);
        position += 32 + data_size.max(0) as usize * count.max(0) as usize;
    }
    names
}
