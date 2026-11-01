//! 手工诊断：打印旧式材质资产的导入引用与可解析属性。
//!
//! 用法：`material_preview <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings, PropValue};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("missing .uasset")?;
    let uexp = args.next().ok_or("missing .uexp")?;
    let mapping = args.next().ok_or("missing .usmap")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    println!(
        "class={} export={} imports={} exports={} names={}",
        package.summary.class_name,
        package.summary.export_name,
        package.imports().len(),
        package.exports().len(),
        package.names().len()
    );
    for (index, (class_package, class_name, object_name)) in package.imports().iter().enumerate() {
        println!(
            "import{index}: class_pkg={class_package} class={class_name} object={object_name}"
        );
    }
    match package.read_export_properties(0, &mappings) {
        Ok(properties) => {
            println!("properties bytes_read={}", properties.bytes_read);
            for (name, value) in &properties.properties {
                println!("  {name} = {}", describe(value, &package));
            }
        }
        Err(error) => println!("properties error: {error}"),
    }
    Ok(())
}

/// 格式化属性值；对象引用解析成导入/导出名称。
fn describe(value: &PropValue, package: &LegacyPackage) -> String {
    match value {
        PropValue::Object(index) => match package.resolve_object_name(*index) {
            Some(name) => format!("Object({index}) {name}"),
            None => format!("Object({index})"),
        },
        PropValue::Array(values) => {
            let shown = values
                .iter()
                .take(6)
                .map(|value| describe(value, package))
                .collect::<Vec<_>>();
            format!("[{} 项: {}]", values.len(), shown.join(", "))
        }
        other => format!("{other:?}"),
    }
}
