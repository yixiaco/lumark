//! 诊断：读取 IoStore ContainerHeader、包 ID 与按包 ID 读取包数据。
//!
//! 用法：`iostore_zen_probe <container.utoc> [package-name]`。

use std::error::Error;

use lumark_archive::{IoStoreContainer, package_id};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("缺少 .utoc 路径")?;
    let package_name = args.next();
    let container = IoStoreContainer::open(&path, None)?;
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
            println!("store entry: not found（包名大小写/挂载路径可能不同）");
        }
        let data_chunks = container.find_package_chunks(id, 1);
        println!("ExportBundleData chunks: {}", data_chunks.len());
        if !data_chunks.is_empty() {
            let data = container.read_package_by_id(id)?;
            println!("package bytes: {}", data.len());
        }
        let bulk = container.find_package_chunks(id, 2);
        println!("BulkData chunks: {}", bulk.len());
    }
    Ok(())
}
