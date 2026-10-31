//! 手工诊断：尝试不同 UObject 偏移读取 FF7R 旧式包属性。
//!
//! 用法：`legacy_scan <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("缺少 .uasset")?;
    let uexp = args.next().ok_or("缺少 .uexp")?;
    let mapping = args.next().ok_or("缺少 .usmap")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    for skip in 0..64 {
        match package.read_export_properties_at(0, &mappings, skip) {
            Ok(properties) => {
                println!(
                    "skip={skip} OK properties={} bytes_read={}",
                    properties.properties.len(),
                    properties.bytes_read
                );
                for (name, value) in properties.properties.iter().take(12) {
                    println!("  {name} = {value:?}");
                }
            }
            Err(error) => println!("skip={skip} ERR {error}"),
        }
    }
    Ok(())
}
