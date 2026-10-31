//! 手工诊断：列出 FF7R PAK 中匹配的条目。
//!
//! 用法：`list_pak <game-root> <container> <filter>`

use std::error::Error;
use std::path::Path;

use lumark_archive::ArchiveService;
use lumark_unreal::{extract_key_for_game, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器")?;
    let filter = args.next().unwrap_or_default().to_ascii_lowercase();
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
        .map(|container| container.path.clone())
        .ok_or("未找到容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到密钥")?;
    let info = ArchiveService::list(&container, Some(key.as_str()))?;
    let mut shown = 0;
    for entry in info.entries {
        if entry.path.to_ascii_lowercase().contains(&filter) {
            println!("{} ({} bytes)", entry.path, entry.size);
            shown += 1;
            if shown >= 40 {
                break;
            }
        }
    }
    println!("shown={shown}");
    Ok(())
}
