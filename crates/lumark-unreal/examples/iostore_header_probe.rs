//! 诊断：在加密 IoStore 容器上读取 ContainerHeader 与包 ID（密钥仅内存）。
//!
//! 用法：`iostore_header_probe <game-root> <container-file> [package-name]`。

use std::error::Error;

use lumark_archive::{IoStoreContainer, package_id};
use lumark_unreal::{extract_key_for_game, scan_game};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let package_name = args.next();
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
    let container = IoStoreContainer::open(&container.path, Some(key.as_str()))?;
    println!(
        "toc: entries={} encrypted={} compression={:?}",
        container.entry_count(),
        container.encrypted(),
        container.compression_methods()
    );
    let header = container.container_header()?;
    println!(
        "container header: version={} id=0x{:016X} packages={}",
        header.version,
        header.container_id,
        header.packages.len()
    );
    if let Some(name) = package_name {
        let id = package_id(&name);
        println!("package name: {name}");
        println!("package id: 0x{id:016X}");
        if let Some(package) = header.package(id) {
            println!("imported packages: {}", package.imported_packages.len());
        } else {
            println!("store entry: not found");
        }
        println!(
            "ExportBundleData chunks: {}",
            container.find_package_chunks(id, 1).len()
        );
        println!(
            "BulkData chunks: {}",
            container.find_package_chunks(id, 2).len()
        );
        // 用目录索引反查真实包 ID，帮助确认 UE 包名。
        let file = container
            .files()
            .iter()
            .find(|file| file.path.ends_with(&name) || file.path.contains(&name));
        if let Some(file) = file {
            let actual = container
                .package_id(file.toc_index)
                .ok_or("目录条目没有 package id")?;
            println!(
                "directory: {} toc={} package_id=0x{actual:016X}",
                file.path, file.toc_index
            );
            println!("header has actual id: {}", header.package(actual).is_some());
            let data = container.read_package_by_id(actual)?;
            println!("package data bytes: {}", data.len());
            let bulk = container.find_package_chunks(actual, 2);
            println!("actual BulkData chunks: {}", bulk.len());
            for candidate in [
                file.path.clone(),
                file.path
                    .strip_suffix(".umap")
                    .or_else(|| file.path.strip_suffix(".uasset"))
                    .unwrap_or(&file.path)
                    .to_string(),
                "/Game/KingS/GinkgoTree".to_string(),
                "Game/KingS/GinkgoTree".to_string(),
                "TogetherBNB/Content/KingS/GinkgoTree".to_string(),
            ] {
                let hash = package_id(&candidate);
                println!(
                    "candidate {candidate:?} -> 0x{hash:016X}{}",
                    if hash == actual { " MATCH" } else { "" }
                );
            }
        } else {
            println!("directory: no matching file");
        }
    }
    Ok(())
}
