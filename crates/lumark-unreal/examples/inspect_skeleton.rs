//! 手工诊断 USkeleton：读取 IoStore 条目、`.usmap` 并输出参考骨骼树。
//!
//! 用法：`inspect_skeleton <utoc> <key|-> <global.utoc> <entry> <mapping.usmap> [export]`。

use std::error::Error;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{Mappings, ZenScriptObjects, parse_package};

/// 读取并打印骨骼树。
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
    let skeleton = package.read_skeleton(export_index, &mappings)?;
    println!("skeleton: {} bones={}", skeleton.name, skeleton.bones.len());
    for (index, bone) in skeleton.bones.iter().enumerate() {
        println!(
            "  {index}: {} parent={} translation={:?}",
            bone.name, bone.parent_index, bone.translation
        );
    }
    Ok(())
}
