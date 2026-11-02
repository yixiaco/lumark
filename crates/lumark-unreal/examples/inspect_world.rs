//! 诊断：读取 `.umap` 的 World/Level 摘要与 Actor 类别统计。
//!
//! 用法：`inspect_world <game-root> <container> <umap-entry> <mapping.usmap>`

use std::error::Error;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, extract_key_for_game, parse_package, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少 .umap 路径")?;
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
    let world = package.read_world_summary(&mappings)?;
    println!(
        "world={} export={} persistent_level={:?} actors={} streaming_levels={}",
        world.name,
        world.world_export,
        world.persistent_level_name,
        world.actor_count,
        world.streaming_level_count
    );
    for (class_name, count) in world.categories.iter().take(20) {
        println!("category {class_name} = {count}");
    }
    for actor in world.actors.iter().take(10) {
        println!(
            "actor {} ({}) export={}",
            actor.name, actor.class_name, actor.export_index
        );
    }
    for warning in &world.warnings {
        println!("warning={warning}");
    }
    Ok(())
}
