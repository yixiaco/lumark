//! 手工诊断：从游戏 PAK 容器中提取单个条目。
//!
//! 用法：`extract_entry <game-root> <container-name> <entry-path> <output-dir>`

use std::error::Error;
use std::path::Path;

use lumark_archive::{ArchiveService, ExtractionOptions};
use lumark_unreal::{extract_key_for_game, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器")?;
    let entry = args.next().ok_or("缺少条目路径")?;
    let output = args.next().ok_or("缺少输出目录")?;
    let game = scan_game(Path::new(&root))?;
    let container = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name == container_name)
        })
        .map(|container| container.path.clone())
        .ok_or("未找到容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到密钥")?;
    let output = Path::new(&output).to_path_buf();
    std::fs::create_dir_all(&output)?;
    ArchiveService::extract(
        &container,
        &ExtractionOptions {
            output_dir: output.clone(),
            selected_paths: vec![entry.replace('\\', "/")],
            overwrite: true,
            password: Some(key),
            ..Default::default()
        },
        |_| {},
    )?;
    println!("extracted to {}", output.display());
    Ok(())
}
