//! 诊断：打印骨骼网格每个材质槽精确解析到的材质包与贴图。
//!
//! 用法：`inspect_material_slots <game-root> <container> <mapping.usmap> <mesh-entry>`
//!
//! 输出每槽的 `loaded_assets`（材质包与贴图名）以及基础色/法线/ORM 是否加载，
//! 用于排查“材质解析成功但颜色不对”的槽位。

use std::error::Error;
use std::path::Path;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot,
    extract_key_for_game, parse_package, resolve_material_slots_exact, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let entry = args.next().ok_or("缺少 SkeletalMesh 资产路径")?;

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
    let index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "SkeletalMesh")
        .ok_or("未找到 SkeletalMesh 导出")?;
    let preview = package.read_skeletal_mesh_preview(index, &mappings, inputs.ubulk.as_deref())?;
    let slots: Vec<MaterialSlot> = preview
        .material_names
        .iter()
        .map(|name| MaterialSlot {
            name: name.clone(),
            package: None,
        })
        .collect();
    let imports: Vec<Option<usize>> = preview
        .material_imports
        .iter()
        .map(|index| (*index < 0).then(|| (-*index - 1) as usize))
        .collect();
    let resolved = resolve_material_slots_exact(
        &mut source,
        &entry,
        &slots,
        &imports,
        Some(&mappings),
        &MaterialResolveOptions::default(),
    );
    for (index, material) in resolved.iter().enumerate() {
        let appearance = material.appearance.as_ref();
        println!(
            "slot[{index}] name={} pkg={:?} base_tex={} normal_t={} orm_t={} ao_t={} tint={:?} masked={:?}",
            material.slot.name,
            material.package_path,
            material
                .viewer
                .as_ref()
                .is_some_and(|viewer| viewer.base_color.is_some()),
            material
                .viewer
                .as_ref()
                .is_some_and(|viewer| viewer.normal.is_some()),
            material
                .viewer
                .as_ref()
                .is_some_and(|viewer| viewer.metallic_roughness.is_some()),
            material
                .viewer
                .as_ref()
                .is_some_and(|viewer| viewer.occlusion.is_some()),
            appearance.map(|appearance| appearance.base_color),
            appearance.map(|appearance| appearance.masked),
        );
        for asset in &material.loaded_assets {
            println!("    loaded={asset}");
        }
        for warning in &material.warnings {
            println!("    warning={warning}");
        }
    }
    Ok(())
}
