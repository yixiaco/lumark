//! 诊断：打印 StaticMesh 的法线/切线/UV/顶点颜色/材质槽统计。
//!
//! 用法：`inspect_mesh_attrs <game-root> <container> <entry> <mapping.usmap>`

use std::error::Error;

use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, MeshLod, extract_key_for_game,
    parse_package, read_static_mesh, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;

    let game = scan_game(std::path::Path::new(&root))?;
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
    let mut source = GameAssetSource::open(
        &container_path,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(mapping_path.clone().into()),
        },
    )?;
    let inputs = source.read_package(&entry)?;
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let package = parse_package(inputs.uasset, source.scripts(), None)?;
    if let Some(mesh_index) = package
        .info()
        .exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = read_static_mesh(&package, mesh_index, &mappings)?;
        println!(
            "mesh={} lod_count={} parsed_lods={} material_slots={:?}",
            mesh.name,
            mesh.lod_count,
            mesh.lods.len(),
            mesh.material_slots
                .iter()
                .map(|slot| slot.name.as_str())
                .collect::<Vec<_>>()
        );
        print_lod_attributes(&mesh.lods);
        return Ok(());
    }
    if let Some(index) = package
        .info()
        .exports
        .iter()
        .position(|export| export.kind == AssetKind::SkeletalMesh)
    {
        let preview =
            package.read_skeletal_mesh_preview(index, &mappings, inputs.ubulk.as_deref())?;
        println!(
            "skeletal_mesh={} bones={} lods={} material_names={:?}",
            preview.skeleton.name,
            preview.skeleton.bones.len(),
            preview.lods.len(),
            preview.material_names
        );
        print_lod_attributes(&preview.lods);
        return Ok(());
    }
    Err("资产中没有 StaticMesh/SkeletalMesh 导出".into())
}

/// 打印每个 LOD 的顶点属性统计。
fn print_lod_attributes(lods: &[MeshLod]) {
    for (index, lod) in lods.iter().enumerate() {
        let attributes = &lod.attributes;
        println!(
            "lod{index} verts={} tris={} sections={} normals={} tangents={} uvs={:?} colors={}",
            lod.positions.len(),
            lod.triangle_count(),
            lod.sections.len(),
            attributes.normals.len(),
            attributes.tangents.len(),
            attributes
                .uvs
                .iter()
                .map(|channel| channel.len())
                .collect::<Vec<_>>(),
            attributes.colors.len()
        );
        if let Some(normal) = attributes.normals.first() {
            println!("  normal0={normal:?}");
        }
        if let Some(tangent) = attributes.tangents.first() {
            println!("  tangent0={tangent:?}");
        }
        if let Some(uv) = attributes.uvs.first().and_then(|channel| channel.first()) {
            println!("  uv0={uv:?}");
        }
        if let Some(color) = attributes.colors.first() {
            println!("  color0={color:?}");
        }
    }
}
