//! 诊断：打开游戏容器并打印 Oodle 预检结果。
//!
//! 用法：`oodle_preflight <game-root> <container>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{GameAssetOptions, GameAssetSource, extract_key_for_game, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let game = scan_game(Path::new(&root))?;
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
        .ok_or("未找到容器")?
        .path
        .clone();
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到密钥")?;
    let global = container
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let mut source = GameAssetSource::open(
        &container,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: None,
        },
    )?;
    let preflight = source.oodle_preflight();
    println!(
        "kind={:?} entries={} methods={:?} needed={} ready={}",
        source.kind(),
        source.entry_count(),
        preflight.methods,
        preflight.needed,
        preflight.ready
    );
    println!("summary={}", preflight.summary());
    println!("backend={}", preflight.status.summary());
    Ok(())
}
