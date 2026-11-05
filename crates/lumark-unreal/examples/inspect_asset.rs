//! 手工诊断 UE Zen 包：读取 IoStore 条目并输出包/导出元数据。
//!
//! 用法：`inspect_asset <utoc> <aes-key|-> <global.utoc> <entry-path>`。

use std::error::Error;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{ZenScriptObjects, parse_package};

/// 读取并打印一个包。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let toc = args.next().ok_or("缺少 .utoc 路径")?;
    let key = args.next().unwrap_or_else(|| "-".to_string());
    let global = args.next().ok_or("缺少 global.utoc 路径")?;
    let entry = args.next().ok_or("缺少包路径")?;
    let key = (key != "-").then_some(key.as_str());
    let container = IoStoreContainer::open(&toc, key)?;
    let scripts = ZenScriptObjects::from_container(std::path::Path::new(&global), key)?;
    println!("script objects: {}", scripts.len());
    let data = container.read_file(&entry)?;
    println!("package bytes: {}", data.len());
    // 诊断 Zen 导出尾部的 cooked header 区域。
    if data.len() > 2634 {
        let tail = &data[data.len() - 2634..];
        println!("tail header first: {:02X?}", &tail[..tail.len().min(64)]);
        let printable = tail.iter().filter(|b| (32..=126).contains(*b)).count();
        println!("tail printable bytes: {printable}");
    }
    let package = parse_package(data, Some(&scripts), None)?;
    let info = package.info();
    println!(
        "package: {} header_version={} header_size={} cooked_header_size={} flags=0x{:08X} ue4={} ue5={} exports={} bulk={}",
        info.package_name,
        info.header_version,
        info.header_size,
        info.cooked_header_size,
        info.package_flags,
        info.file_version_ue4,
        info.file_version_ue5,
        info.exports.len(),
        info.bulk_data.len()
    );
    for (index, entry) in info.bulk_data.iter().enumerate() {
        println!(
            "bulk {index}: offset={} size={} flags=0x{:X} cooked={}",
            entry.serial_offset, entry.serial_size, entry.flags, entry.cooked_index
        );
    }
    for (index, name) in info.names.iter().enumerate() {
        println!("name {index}: {name}");
    }
    for (index, export) in info.exports.iter().enumerate() {
        println!(
            "export {index}: {} class={} kind={} data={}+{} outer={:?}",
            export.name,
            export.class_name,
            export.kind.label(),
            export.data_offset,
            export.data_size,
            export.outer_export
        );
        if let Some(bytes) = package.export_data(index) {
            println!("  first: {:02X?}", &bytes[..bytes.len().min(48)]);
        }
    }
    Ok(())
}
