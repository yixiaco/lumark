//! 手工诊断：打印 `.usmap` 中指定结构的属性槽位。
//!
//! 用法：`mapping_dump <mappings.usmap> <StructName> [limit]`

use std::error::Error;
use std::path::Path;

use lumark_unreal::Mappings;

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("missing .usmap")?;
    let name = args.next().ok_or("missing struct name")?;
    let limit = args
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(24);
    let mappings = Mappings::parse_file(Path::new(&path))?;
    if mappings.find(&name).is_none() {
        println!("{name} 不在映射中");
        return Ok(());
    }
    for index in 0..limit {
        match mappings.property_at(&name, index) {
            Some((schema, element)) => {
                println!("slot{index}: {} {:?} [{element}]", schema.name, schema.ty)
            }
            None => break,
        }
    }
    Ok(())
}
