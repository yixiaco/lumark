//! 诊断：加载 UE5 World `.umap` 为可渲染 Scene（V-14）。
//!
//! 用法：`world_scene <game-root> <container-file> <world-entry> [mappings.usmap]`。

use std::error::Error;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, WorldSceneOptions, extract_key_for_game,
    load_world_scene, scan_game,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少 World 条目路径")?;
    let mapping_arg = args.next();
    let game = scan_game(std::path::Path::new(&root))?;
    let container = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == container_name)
        })
        .ok_or("未找到指定容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到 AES 密钥")?;
    let mapping_path = mapping_arg
        .map(std::path::PathBuf::from)
        .or_else(|| game.mapping_candidates.first().cloned())
        .ok_or("缺少 .usmap")?;
    let mappings = Mappings::parse_file(&mapping_path)?;
    let global = container
        .path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let mut source = GameAssetSource::open(
        &container.path,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(mapping_path),
        },
    )?;
    let start = std::time::Instant::now();
    let loaded = load_world_scene(
        &mut source,
        &entry,
        &mappings,
        &WorldSceneOptions::default(),
    )?;
    println!(
        "world scene: actors={} meshes={} triangles={} skipped={} elapsed={:.1}ms",
        loaded.report.actor_count,
        loaded.report.mesh_count,
        loaded.report.triangle_count,
        loaded.report.skipped,
        start.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "scene: name={} meshes={} nodes={} bounds={:?}",
        loaded.scene.name,
        loaded.scene.meshes.len(),
        loaded.scene.nodes.len(),
        loaded.scene.compute_bounds()
    );
    for warning in loaded.report.warnings.iter().take(10) {
        println!("warning={warning}");
    }
    Ok(())
}
