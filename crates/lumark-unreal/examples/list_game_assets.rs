//! 手工诊断：扫描游戏、提取密钥并列出容器内匹配的资产路径。
//!
//! 用法：`list_game_assets <game-root> <container> <filter>`；不打印密钥。

use std::error::Error;
use std::path::Path;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{extract_key_for_game, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let filter = args.next().unwrap_or_default().to_ascii_lowercase();
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
    let container = IoStoreContainer::open(&container_path, Some(&key))?;
    let mut shown = 0usize;
    for file in container.files() {
        if filter.is_empty() || file.path.to_ascii_lowercase().contains(&filter) {
            println!("{} ({} bytes)", file.path, file.size);
            shown += 1;
            if shown >= 80 {
                break;
            }
        }
    }
    println!("matches>= {shown}");
    Ok(())
}
