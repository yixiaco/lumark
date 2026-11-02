//! 诊断：把 Unreal 资产转换为引擎无关 `lumark_scene::Scene` 并输出统计。
//!
//! 用法：`build_scene <game-root> <container> <mesh-entry> <mapping.usmap>`

use std::error::Error;

use lumark_scene::scene_inspector;
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot,
    extract_key_for_game, parse_package, resolve_material_slots, scan_game, skeletal_mesh_scene,
    static_mesh_scene,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少网格资产路径")?;
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
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
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

    let scene = if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = package.read_static_mesh(index, &mappings)?;
        let slots = mesh.material_slots.clone();
        let resolved = resolve_material_slots(
            &mut source,
            &entry,
            &slots,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        static_mesh_scene(&mesh.name, &mesh, &resolved)
    } else if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::SkeletalMesh)
    {
        let preview =
            package.read_skeletal_mesh_preview(index, &mappings, inputs.ubulk.as_deref())?;
        let slots: Vec<MaterialSlot> = preview
            .material_names
            .iter()
            .map(|name| MaterialSlot {
                name: name.clone(),
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
        skeletal_mesh_scene(&preview.skeleton.name, &preview, &resolved)
    } else {
        return Err("资产中没有 StaticMesh/SkeletalMesh 导出".into());
    };

    println!(
        "scene={} nodes={} meshes={} materials={} vertices={} triangles={} textures={} bounds={:?}",
        scene.name,
        scene.stats().nodes,
        scene.stats().meshes,
        scene.stats().materials,
        scene.stats().vertices,
        scene.stats().triangles,
        scene.stats().textures,
        scene.compute_bounds()
    );
    let inspector = scene_inspector(&scene);
    println!(
        "inspector title={} rows={}",
        inspector.title,
        inspector.row_count()
    );
    for section in &inspector.sections {
        println!("[{}]", section.title);
        for row in section.rows.iter().take(6) {
            println!("  {} = {}", row.label, row.value);
        }
    }
    Ok(())
}
