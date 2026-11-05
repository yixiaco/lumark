//! 手工诊断 StaticMesh：带 `.ubulk` 侧载数据读取几何（外部 bulk 布局）。
//!
//! 用法：`inspect_mesh_bulk <utoc> <key|-> <global.utoc> <entry> <usmap> <ubulk|-> [export] [out.png]`。

use std::error::Error;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{Mappings, ZenScriptObjects, parse_package};

/// 读取并打印网格概况。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let toc = args.next().ok_or("缺少 .utoc 路径")?;
    let key = args.next().unwrap_or_else(|| "-".to_string());
    let global = args.next().ok_or("缺少 global.utoc 路径")?;
    let entry = args.next().ok_or("缺少包路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let ubulk_entry = args.next().filter(|value| value != "-");
    let export_arg = args
        .next()
        .filter(|value| value != "-")
        .map(|value| value.parse::<usize>())
        .transpose()?;
    let output = args.next();
    let key = (key != "-").then_some(key.as_str());
    let container = IoStoreContainer::open(&toc, key)?;
    let scripts = ZenScriptObjects::from_container(std::path::Path::new(&global), key)?;
    let data = container.read_file(&entry)?;
    let package = parse_package(data, Some(&scripts), None)?;
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let ubulk = ubulk_entry
        .map(|path| container.read_file(&path))
        .transpose()?;
    // 未指定导出索引时取第一个 StaticMesh 导出（包内通常还有 BodySetup 等对象）。
    let export_index = export_arg.unwrap_or_else(|| {
        package
            .info()
            .exports
            .iter()
            .position(|export| export.class_name == "StaticMesh")
            .unwrap_or(0)
    });
    let mesh = package.read_static_mesh_with_bulk(export_index, &mappings, ubulk.as_deref())?;
    println!(
        "static mesh: {} vertices={} triangles={} sections={} lods={}",
        mesh.name,
        mesh.lod.positions.len(),
        mesh.lod.triangle_count(),
        mesh.lod.sections.len(),
        mesh.lods.len()
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
