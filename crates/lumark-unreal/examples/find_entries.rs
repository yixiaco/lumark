//! 手工诊断：在游戏的所有 PAK 容器里按路径片段搜索条目。
//!
//! 用法：`find_entries <game-root> <filter>`

use std::error::Error;
use std::path::Path;

use lumark_archive::ArchiveService;
use lumark_unreal::{extract_key_for_game, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let filter = args.next().unwrap_or_default().to_ascii_lowercase();
    let game = scan_game(Path::new(&root))?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到密钥")?;
    let mut shown = 0usize;
    for container in &game.containers {
        let Ok(info) = ArchiveService::list(&container.path, Some(key.as_str())) else {
            continue;
        };
        let name = container
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("container");
        for entry in &info.entries {
            if entry.path.to_ascii_lowercase().contains(&filter) {
                println!(
                    "{name} | {} | {} bytes",
                    entry.path.replace('\\', "/"),
                    entry.size
                );
                shown += 1;
                if shown >= 200 {
                    return Ok(());
                }
            }
        }
    }
    println!("shown={shown}");
    Ok(())
}
