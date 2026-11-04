//! 诊断：解析网格材质槽 → 材质包 → 父材质链 → 贴图。
//!
//! 用法：`resolve_materials <game-root> <container> <mesh-entry> <mapping.usmap>`

use std::error::Error;

use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot,
    extract_key_for_game, parse_package, read_static_mesh, resolve_static_materials, scan_game,
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
    let mut slots: Vec<MaterialSlot> = Vec::new();
    let mut material_imports: Vec<i32> = Vec::new();
    if let Some(index) = package
        .info()
        .exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = read_static_mesh(&package, index, &mappings)?;
        slots = mesh.material_slots;
        material_imports = mesh.material_imports;
    } else if let Some(index) = package
        .info()
        .exports
        .iter()
        .position(|export| export.kind == AssetKind::SkeletalMesh)
    {
        let preview =
            package.read_skeletal_mesh_preview(index, &mappings, inputs.ubulk.as_deref())?;
        slots = preview
            .material_names
            .into_iter()
            .map(|name| MaterialSlot {
                name,
                package: None,
            })
            .collect();
        material_imports = preview.material_imports;
    }
    println!("mesh={entry} slots={}", slots.len());
    let resolved = resolve_static_materials(
        &mut source,
        &entry,
        &slots,
        &material_imports,
        Some(&mappings),
        &MaterialResolveOptions::default(),
    );
    for material in resolved {
        println!(
            "slot={:?} material={:?} appearance={:?}",
            material.slot.name,
            material.package_path,
            material.appearance.as_ref().map(|appearance| {
                (
                    appearance.base_color,
                    appearance.metallic,
                    appearance.roughness,
                    appearance.textures.len(),
                )
            })
        );
        if let Some(viewer) = &material.viewer {
            println!(
                "  textures base={} normal={} orm={}",
                viewer.base_color.is_some(),
                viewer.normal.is_some(),
                viewer.metallic_roughness.is_some()
            );
        }
        for asset in &material.loaded_assets {
            println!("  loaded={asset}");
        }
        for warning in &material.warnings {
            println!("  warning={warning}");
        }
    }
    Ok(())
}
