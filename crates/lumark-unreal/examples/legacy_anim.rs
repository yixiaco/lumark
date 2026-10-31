//! 手工诊断：转储旧式 AnimSequence 的属性字段头与名称表。
//!
//! 用法：`legacy_anim <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("missing .uasset")?;
    let uexp = args.next().ok_or("missing .uexp")?;
    let mapping = args.next().ok_or("missing .usmap")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    for (index, name) in package.names().iter().enumerate() {
        println!("name{index}={name}");
    }
    let data = package.export_data(0).ok_or("export data")?;
    println!("data_len={}", data.len());
    for chunk in data[..600.min(data.len())].chunks(16) {
        println!(
            "{:04X}: {}",
            (chunk.as_ptr() as usize - data.as_ptr() as usize),
            chunk
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    for index in 0..120 {
        match mappings.property_at("AnimSequence", index) {
            Some((schema, element)) => {
                println!("slot{index}={} {:?} [{element}]", schema.name, schema.ty)
            }
            None => break,
        }
    }
    match package.read_export_properties(0, &mappings) {
        Ok(properties) => {
            println!("bytes_read={}", properties.bytes_read);
            for (name, value) in &properties.properties {
                println!("{name} = {value:?}");
            }
        }
        Err(error) => println!("properties error: {error}"),
    }
    Ok(())
}
