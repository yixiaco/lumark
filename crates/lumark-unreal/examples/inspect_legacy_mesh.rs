//! 诊断：解析旧式 SkeletalMesh，打印 LOD/顶点属性统计。
//!
//! 用法：`inspect_legacy_mesh <mesh.uasset> <mesh.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings, MeshLod, read_legacy_skeletal_mesh_geometry};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("缺少 mesh.uasset")?;
    let uexp = args.next().ok_or("缺少 mesh.uexp")?;
    let mapping = args.next().ok_or("缺少 .usmap")?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&std::fs::read(asset)?, &std::fs::read(uexp)?)?;
    let geometry = read_legacy_skeletal_mesh_geometry(&package, 0, &mappings)?;
    println!(
        "skeleton={} bones={} lod_count={} sections={} verts={} tris={} skin={}",
        geometry.skeleton.name,
        geometry.skeleton.bones.len(),
        geometry.lod_count,
        geometry.section_count,
        geometry.mesh.positions.len(),
        geometry.mesh.triangle_count(),
        geometry.skin.is_some()
    );
    print_lod_attributes(&geometry.mesh);
    let (mut min, mut max) = ([f64::MAX; 3], [f64::MIN; 3]);
    for position in &geometry.mesh.positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    println!("bounds min={min:?} max={max:?}");
    Ok(())
}

/// 打印顶点属性统计与样例。
fn print_lod_attributes(lod: &MeshLod) {
    let attributes = &lod.attributes;
    println!(
        "lod0 verts={} tris={} sections={} normals={} tangents={} uvs={:?} colors={}",
        lod.positions.len(),
        lod.triangle_count(),
        lod.sections.len(),
        attributes.normals.len(),
        attributes.tangents.len(),
        attributes
            .uvs
            .iter()
            .map(|channel| channel.len())
            .collect::<Vec<_>>(),
        attributes.colors.len()
    );
    if let Some(normal) = attributes.normals.first() {
        println!("  normal0={normal:?}");
    }
    if let Some(tangent) = attributes.tangents.first() {
        println!("  tangent0={tangent:?}");
    }
    if let Some(uv) = attributes.uvs.first().and_then(|channel| channel.first()) {
        println!("  uv0={uv:?}");
    }
}
