//! 诊断：通过 `load_scene` Provider 加载资产并打印报告/警告。
//!
//! 用法：`provider_scene <game-root> <container> <mapping.usmap> <entry> [entry...]`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, LodPolicy, Mappings, SceneLoadOptions, SceneLoadStage,
    SceneRequest, extract_key_for_game, load_scene, scan_game, texture_cache,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let mut entries = Vec::new();
    let mut animation = None;
    while let Some(argument) = args.next() {
        if argument == "--anim" {
            animation = args.next();
        } else {
            entries.push(argument);
        }
    }
    if entries.is_empty() {
        return Err("缺少资产路径".into());
    }

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
    let options = SceneLoadOptions {
        load_materials: true,
        max_texture_size: 512,
        texture_cache: Some(texture_cache()),
        animation,
        animation_time: 0.5,
    };
    for entry in &entries {
        let request = SceneRequest::new(entry.as_str()).with_lod(LodPolicy::First);
        let loaded = load_scene(&mut source, &mappings, &request, &options, &mut |stage| {
            println!("  stage={stage:?}");
        })?;
        println!(
            "asset={} class={} lods={} used={} verts={} tris={} materials={} warnings={}",
            loaded.report.asset_path,
            loaded.report.asset_class,
            loaded.report.lod_count,
            loaded.report.lod_used,
            loaded.report.vertices,
            loaded.report.triangles,
            loaded.report.materials,
            loaded.report.warnings.len()
        );
        for warning in loaded.report.warnings.iter().take(5) {
            println!("  warning={warning}");
        }
        if let Some(animation) = &loaded.report.animation {
            println!(
                "  animation={} length={:.3}s frames={} tracks={} time={:.3}s",
                animation.name,
                animation.length,
                animation.frames,
                animation.tracks,
                animation.time
            );
        }
        println!("  bones={}", loaded.report.bones);
        assert!(scene_stage_reached(&loaded));
    }
    Ok(())
}

/// 报告已完成（避免 unused import 警告并断言阶段语义）。
fn scene_stage_reached(loaded: &lumark_unreal::LoadedScene) -> bool {
    let _ = SceneLoadStage::Finished {
        vertices: loaded.report.vertices,
        triangles: loaded.report.triangles,
    };
    !loaded.scene.meshes.is_empty()
}
