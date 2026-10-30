//! 手工诊断：列出归档条目，或解压指定条目。
//!
//! 用法：`archive_entry <archive> [entry]`；指定 entry 时解压到当前目录。

use std::error::Error;
use std::path::Path;

use lumark_archive::{ArchiveService, ExtractionOptions};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let archive = args.next().ok_or("缺少归档路径")?;
    let entry = args.next();
    let info = ArchiveService::list(Path::new(&archive), None)?;
    match entry {
        Some(entry) => {
            let output = std::env::current_dir()?.join("target/archive-entry");
            let options = ExtractionOptions {
                output_dir: output.clone(),
                selected_paths: vec![entry.clone()],
                overwrite: true,
                ..Default::default()
            };
            ArchiveService::extract(Path::new(&archive), &options, |_| {})?;
            println!("extracted {} -> {}", entry, output.display());
        }
        None => {
            for entry in info.entries.iter().take(30) {
                println!("{} ({} bytes)", entry.path, entry.size);
            }
            println!("entries={}", info.entries.len());
        }
    }
    Ok(())
}
