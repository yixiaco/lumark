//! 手工诊断 StaticMesh：读取 IoStore 条目、`.usmap` 并输出首个 LOD。
//!
//! 用法：`inspect_mesh <utoc> <key|-> <global.utoc> <entry> <mapping.usmap> [export]`。

use std::error::Error;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{Mappings, ZenScriptObjects, parse_package};

/// 读取并打印网格概况。
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
    let output = args.next();
    let key = (key != "-").then_some(key.as_str());
    let container = IoStoreContainer::open(&toc, key)?;
    let scripts = ZenScriptObjects::from_container(std::path::Path::new(&global), key)?;
    let data = container.read_file(&entry)?;
    let package = parse_package(data, Some(&scripts), None)?;
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let mesh = package.read_static_mesh(export_index, &mappings)?;
    println!(
        "static mesh: {} vertices={} triangles={} sections={}",
        mesh.name,
        mesh.lod.positions.len(),
        mesh.lod.triangle_count(),
        mesh.lod.sections.len()
    );
    println!(
        "positions: {:?}",
        &mesh.lod.positions[..mesh.lod.positions.len().min(8)]
    );
    println!(
        "indices: {:?}",
        &mesh.lod.indices[..mesh.lod.indices.len().min(18)]
    );
    for section in &mesh.lod.sections {
        println!(
            "  section material={} first={} triangles={}",
            section.material_index, section.first_index, section.triangle_count
        );
    }
    if let Some(output) = output {
        let rgba = lumark_unreal::render_static_mesh(&mesh.lod, 480, 360);
        let image = image::RgbaImage::from_raw(480, 360, rgba).ok_or("渲染尺寸不匹配")?;
        image.save(&output)?;
        println!("saved {output}");
    }
    Ok(())
}
