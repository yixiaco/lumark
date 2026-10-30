//! 手工诊断游戏目录：扫描容器并尝试提取 AES 密钥。
//!
//! 用法：`keyscan <game-root>`。只读取本地文件，不打印完整密钥。

use std::error::Error;

use lumark_unreal::{extract_key_for_game, scan_game};

/// 扫描游戏并输出检测结果。
fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::args().nth(1).ok_or("用法：keyscan <game-root>")?;
    let game = scan_game(std::path::Path::new(&root))?;
    println!("name: {}", game.name);
    println!("project: {}", game.project_dir.display());
    println!("engine: {}", game.engine.label());
    println!(
        "shipping exe: {}",
        game.shipping_exe
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "missing".to_string())
    );
    for container in &game.containers {
        println!(
            "container: {} ({}, {} bytes, encrypted={:?})",
            container.path.display(),
            container.kind.label(),
            container.size,
            container.encrypted
        );
    }
    for oodle in &game.oodle_candidates {
        println!("oodle candidate: {}", oodle.display());
    }
    for mapping in &game.mapping_candidates {
        println!("mapping candidate: {}", mapping.display());
    }
    let result = extract_key_for_game(&game, None, |_, _, _| {});
    match result.key {
        Some(key) => {
            let text = key.to_string();
            let masked = format!("{}…{}", &text[..8], &text[text.len() - 8..]);
            println!(
                "AES key: {masked} (candidates={}, validated={})",
                result.candidate_count,
                result
                    .validated_against
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            );
        }
        None => println!(
            "AES key: not found (candidates={}, {})",
            result.candidate_count,
            result.message.unwrap_or_default()
        ),
    }
    Ok(())
}
