//! Manual probe: scan a legacy FF7R Skeleton/SkeletalMesh export for `FReferenceSkeleton`.
//!
//! Usage: `scan_ff7_skeleton <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{
    LegacyPackage, Mappings, read_legacy_skeletal_mesh_geometry,
    read_legacy_skeletal_mesh_skeleton, read_legacy_skeletal_mesh_summary, read_legacy_skeleton,
    render_skeletal_mesh_orbit,
};

fn i32_at(data: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn f32_at(data: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn f64_at(data: &[u8], offset: usize) -> Option<f64> {
    Some(f64::from_le_bytes(
        data.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn skip_fstring(data: &[u8], offset: usize) -> Option<usize> {
    let length = i32_at(data, offset)?;
    if !(-1024..=1024).contains(&length) || length == 0 {
        return None;
    }
    let cells = length.unsigned_abs() as usize;
    let bytes = cells.checked_mul(if length < 0 { 2 } else { 1 })?;
    let end = offset.checked_add(4)?.checked_add(bytes)?;
    data.get(offset + 4..end)?;
    Some(end)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("missing .uasset")?;
    let uexp = args.next().ok_or("missing .uexp")?;
    let mapping = args.next().ok_or("missing .usmap")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    let properties = package.read_export_properties(0, &mappings)?;
    println!(
        "skeletal_mesh_custom_version={:?}",
        package.custom_version([
            0x00, 0x4A, 0x8A, 0xD7, 0x97, 0x46, 0x58, 0xE8, 0xB5, 0x19, 0xA8, 0xBA, 0xB4, 0x46,
            0x7D, 0x48,
        ])
    );
    for (guid, version) in package.custom_versions() {
        println!(
            "custom {version} {}",
            guid.map(|byte| format!("{byte:02X}")).join("")
        );
    }
    let data = package.export_data(0).ok_or("export data out of range")?;
    println!(
        "properties={} bytes_read={} names={} data={}",
        properties.properties.len(),
        properties.bytes_read,
        package.names().len(),
        data.len()
    );
    if let Ok(skeleton) = read_legacy_skeleton(&package, 0, &mappings) {
        println!(
            "parsed bones={} first={:?}",
            skeleton.bones.len(),
            skeleton
                .bones
                .first()
                .map(|bone| (&bone.name, bone.parent_index))
        );
    }
    if let Ok(geometry) = read_legacy_skeletal_mesh_geometry(&package, 0, &mappings) {
        println!(
            "skeletal_mesh_geometry verts={} tris={} sections={} lods={} skin={:?}",
            geometry.mesh.positions.len(),
            geometry.mesh.triangle_count(),
            geometry.mesh.sections.len(),
            geometry.lod_count,
            geometry.skin.as_ref().map(|skin| (
                skin.vertices,
                skin.max_influences,
                skin.weights.first().copied()
            ))
        );
        let (mut min, mut max) = ([f64::MAX; 3], [f64::MIN; 3]);
        for position in &geometry.mesh.positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(position[axis]);
                max[axis] = max[axis].max(position[axis]);
            }
        }
        println!("geometry_bounds min={min:?} max={max:?}");
        println!(
            "geometry_first={:?}",
            geometry.mesh.positions.iter().take(8).collect::<Vec<_>>()
        );
        let rgba = render_skeletal_mesh_orbit(
            &geometry.mesh,
            &geometry.skeleton,
            480,
            360,
            35.0,
            25.0,
            1.0,
        );
        if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
            image.save("target/ff7_geometry.png")?;
        }
        let front =
            render_skeletal_mesh_orbit(&geometry.mesh, &geometry.skeleton, 480, 360, 0.0, 0.0, 1.0);
        if let Some(image) = image::RgbaImage::from_raw(480, 360, front) {
            image.save("target/ff7_geometry_front.png")?;
        }
        let top = render_skeletal_mesh_orbit(
            &geometry.mesh,
            &geometry.skeleton,
            480,
            360,
            0.0,
            89.0,
            1.0,
        );
        if let Some(image) = image::RgbaImage::from_raw(480, 360, top) {
            image.save("target/ff7_geometry_top.png")?;
        }
    }
    if let Ok(summary) = read_legacy_skeletal_mesh_summary(&package, 0, &mappings) {
        println!(
            "skeletal_mesh_summary bones={} lods={} sections={:?}",
            summary.skeleton.bones.len(),
            summary.lod_count,
            summary.section_counts
        );
    }
    if let Ok(skeleton) = read_legacy_skeletal_mesh_skeleton(&package, 0, &mappings) {
        println!(
            "skeletal_mesh_skeleton bones={} root={:?}",
            skeleton.bones.len(),
            skeleton
                .bones
                .first()
                .map(|bone| (&bone.name, bone.parent_index))
        );
    }
    let names = package.names();
    let end = data.len();
    for offset in properties.bytes_read..end {
        for with_export_name in [false, true] {
            for pose_f64 in [false, true] {
                let Some(bone_count) =
                    i32_at(data, offset).filter(|count| (1..=512).contains(count))
                else {
                    break;
                };
                let bone_count = bone_count as usize;
                let mut position = offset + 4;
                let mut bone_names = Vec::with_capacity(bone_count);
                let mut valid = true;
                for _ in 0..bone_count {
                    let (Some(index), Some(number), Some(parent)) = (
                        i32_at(data, position),
                        i32_at(data, position + 4),
                        i32_at(data, position + 8),
                    ) else {
                        valid = false;
                        break;
                    };
                    position += 12;
                    if index < 0
                        || index as usize >= names.len()
                        || !(0..=100).contains(&number)
                        || parent < -1
                        || parent >= bone_count as i32
                    {
                        valid = false;
                        break;
                    }
                    if with_export_name {
                        let Some(next) = skip_fstring(data, position) else {
                            valid = false;
                            break;
                        };
                        position = next;
                    }
                    bone_names.push(names[index as usize].clone());
                }
                if !valid {
                    continue;
                }
                let Some(pose_count) = i32_at(data, position) else {
                    continue;
                };
                if pose_count != bone_count as i32 {
                    continue;
                }
                position += 4;
                let mut poses_valid = true;
                for _ in 0..bone_count {
                    for component in 0..10 {
                        let component_offset = position + component * if pose_f64 { 8 } else { 4 };
                        let value = if pose_f64 {
                            f64_at(data, component_offset)
                        } else {
                            f32_at(data, component_offset).map(f64::from)
                        };
                        let Some(value) = value else {
                            poses_valid = false;
                            break;
                        };
                        if !value.is_finite() {
                            poses_valid = false;
                            break;
                        }
                    }
                    if !poses_valid {
                        break;
                    }
                    position += if pose_f64 { 80 } else { 40 };
                }
                if poses_valid {
                    println!(
                        "candidate offset={offset} export_name={with_export_name} pose_f64={pose_f64} bones={bone_count} names={:?}",
                        &bone_names[..bone_names.len().min(8)]
                    );
                }
            }
        }
    }
    Ok(())
}
