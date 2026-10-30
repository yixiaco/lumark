//! 手工诊断无版本属性：读取 IoStore 条目、`.usmap` 并打印属性。
//!
//! 用法：`inspect_unversioned <utoc> <key|-> <global.utoc> <entry> <mapping.usmap> [export]`。

use std::error::Error;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{Mappings, ZenScriptObjects, parse_package};

/// 读取并打印属性。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let toc = args.next().ok_or("缺少 .utoc")?;
    let key = args.next().unwrap_or_else(|| "-".to_string());
    let global = args.next().ok_or("缺少 global.utoc")?;
    let entry = args.next().ok_or("缺少包路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let export_index = args
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let key = (key != "-").then_some(key.as_str());
    let container = IoStoreContainer::open(&toc, key)?;
    let scripts = ZenScriptObjects::from_container(std::path::Path::new(&global), key)?;
    let data = container.read_file(&entry)?;
    let package = parse_package(data, Some(&scripts), None)?;
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or("导出索引越界")?;
    println!(
        "export {export_index}: {} class={} data={}",
        export.name, export.class_name, export.data_size
    );
    let properties = package.read_export_properties(export_index, &mappings)?;
    println!("properties bytes: {}", properties.bytes_read);
    for (name, value) in &properties.properties {
        println!("  {name} = {value:?}");
    }
    let data = package.export_data(export_index).ok_or("导出数据越界")?;
    for (line, bytes) in data.chunks(16).enumerate() {
        let offset = line * 16;
        if offset > 160 {
            break;
        }
        println!("{offset:04X}: {bytes:02X?}");
    }
    println!(
        "after properties: {:02X?}",
        &data[properties.bytes_read.min(data.len())..data.len().min(properties.bytes_read + 48)]
    );
    for (offset, byte) in data
        .iter()
        .enumerate()
        .skip(properties.bytes_read)
        .take(120)
    {
        print!("{offset:04X}:{byte:02X} ");
        if (offset - properties.bytes_read + 1) % 12 == 0 {
            println!();
        }
    }
    println!();
    Ok(())
}
