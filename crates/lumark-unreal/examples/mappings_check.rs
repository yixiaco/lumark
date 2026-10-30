//! 手工检查 `.usmap` 映射：输出结构/枚举数量和少量属性。
//!
//! 用法：`mappings_check <path.usmap>`。

use std::error::Error;

use lumark_unreal::Mappings;

/// 读取并打印映射概况。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("缺少 .usmap 路径")?;
    let filter = args.next();
    let mappings = Mappings::parse_file(std::path::Path::new(&path))?;
    println!(
        "structs={} enums={}",
        mappings.structs.len(),
        mappings.enums.len()
    );
    if let Some(filter) = filter {
        if let Some(pattern) = filter.strip_prefix("grep:") {
            let pattern = pattern.to_ascii_lowercase();
            let mut matches = 0;
            for name in mappings.structs.keys() {
                if name.to_ascii_lowercase().contains(&pattern) {
                    println!("{name}");
                    matches += 1;
                    if matches >= 100 {
                        break;
                    }
                }
            }
            println!("matches={matches}");
            return Ok(());
        }
        let mut current = Some(filter.as_str());
        while let Some(name) = current {
            let Some(schema) = mappings.find(name) else {
                println!("{name}: not found");
                break;
            };
            println!(
                "{name}: super={:?} props={} serializable={}",
                schema.super_name,
                schema.prop_count,
                schema.props.len()
            );
            for property in &schema.props {
                println!(
                    "  {} slot={} array={} type={:?}",
                    property.name, property.schema_index, property.array_size, property.ty
                );
            }
            current = schema.super_name.as_deref();
        }
        return Ok(());
    }
    for (index, (name, schema)) in mappings.structs.iter().enumerate() {
        if index >= 5 {
            break;
        }
        println!(
            "{name}: super={:?} props={} serializable={}",
            schema.super_name,
            schema.prop_count,
            schema.props.len()
        );
        for property in schema.props.iter().take(3) {
            println!(
                "  {} slot={} array={} type={:?}",
                property.name, property.schema_index, property.array_size, property.ty
            );
        }
    }
    Ok(())
}
