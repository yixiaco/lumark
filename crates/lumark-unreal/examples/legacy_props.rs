//! 手工诊断：解析旧式 uasset/uexp 并打印字段级属性。
//!
//! 用法：`legacy_props <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("缺少 .uasset")?;
    let uexp = args.next().ok_or("缺少 .uexp")?;
    let mapping = args.next().ok_or("缺少 .usmap")?;
    let asset_data = std::fs::read(&asset)?;
    let uexp_data = std::fs::read(&uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    for (index, export) in package.exports().iter().enumerate() {
        println!(
            "export{index} offset={} size={} uexp={} header={}",
            export.serial_offset,
            export.serial_size,
            uexp_data.len(),
            package.summary.total_header_size
        );
    }
    println!(
        "package={} class={} exports={} imports={} names={}",
        package.summary.package_name,
        package.summary.class_name,
        package.summary.export_count,
        package.summary.import_count,
        package.summary.name_count
    );
    for (index, export) in package.exports().iter().enumerate() {
        println!("export {index}: {} ({})", export.name, export.class_name);
        match package.read_export_properties(index, &mappings) {
            Ok(properties) => {
                for (name, value) in properties.properties.iter().take(40) {
                    println!("  {name} = {value:?}");
                }
            }
            Err(error) => println!("  properties error: {error}"),
        }
    }
    Ok(())
}
