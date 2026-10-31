//! Manual probe: parse FF7R legacy SkeletalMesh section headers.
//!
//! Usage: `parse_ff7_section <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings, read_legacy_skeletal_mesh_summary};

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self.position.checked_add(length).ok_or("overflow")?;
        let bytes = self.data.get(self.position..end).ok_or("truncated")?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
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
    let summary = read_legacy_skeletal_mesh_summary(&package, 0, &mappings)?;
    let data = package.export_data(0).ok_or("export data")?;
    println!(
        "skeleton_end={} lods={} sections={:?}",
        summary.skeleton_end, summary.lod_count, summary.section_counts
    );
    let mut reader = Reader {
        data,
        position: summary.skeleton_end,
    };
    let lod_count = reader.i32()?;
    println!("native_lod_count={lod_count}");
    for lod in 0..lod_count.min(4) {
        let strip = reader.bytes(2)?;
        let count = reader.i32()?;
        println!(
            "lod{lod} start={} strip={strip:02X?} sections={count}",
            reader.position
        );
        for section in 0..count {
            let start = reader.position;
            let inner_strip = reader.bytes(2)?;
            let material = reader.i16()?;
            let base_index = reader.i32()?;
            let triangles = reader.i32()?;
            let sorting = reader.u8()?;
            let legacy_cloth = reader.i32()?;
            let cloth_section = reader.i16()?;
            let cloth_lod = reader.u8()?;
            let recompute = reader.i32()?;
            let cast_shadow = reader.i32()?;
            let base_vertex = reader.u32()?;
            let bone_map_count = reader.i32()?;
            let bone_bytes = if (0..=68).contains(&bone_map_count) {
                reader.bytes(bone_map_count as usize * 2)?
            } else {
                &[]
            };
            let num_vertices = reader.i32()?;
            let max_influences = reader.i32()?;
            let cloth_mapping = reader.i32()?;
            let physical_verts = reader.i32()?;
            let physical_normals = reader.i32()?;
            let cloth_asset = reader.i16()?;
            let clothing = reader.bytes(20)?;
            let ff7_should = reader.i32()?;
            let ff7_len = reader.i32()?;
            println!(
                " section{section} start={start} end={} strip={inner_strip:02X?} material={material} base={base_index} tris={triangles} sorting={sorting} cloth={legacy_cloth} clothsec={cloth_section} clothlod={cloth_lod} recompute={recompute} shadow={cast_shadow} basevertex={base_vertex} bones={bone_map_count} verts={num_vertices} maxinf={max_influences} clothmap={cloth_mapping} physv={physical_verts} physn={physical_normals} clothasset={cloth_asset} clothing={clothing:02X?} ff7=({ff7_should},{ff7_len})",
                reader.position
            );
            let _ = bone_bytes;
            if ff7_should == 1 && ff7_len >= 0 {
                reader.bytes(ff7_len as usize * 16)?;
            }
        }
    }
    // FMultisizeIndexContainer
    let index_data_size = reader.u8()?;
    let index_element_size = reader.i32()?;
    let index_count = reader.i32()?;
    println!(
        "after_sections={} index_data_size={index_data_size} index_element_size={index_element_size} index_count={index_count}",
        reader.position
    );
    let index_bytes = reader.bytes(index_count as usize * index_data_size as usize)?;
    if index_data_size == 2 {
        let first: Vec<u16> = index_bytes
            .as_chunks::<2>()
            .0
            .iter()
            .take(12)
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect();
        println!("indices16 first={first:?}");
    } else {
        let first: Vec<u32> = index_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .take(12)
            .map(|chunk| u32::from_le_bytes(*chunk))
            .collect();
        println!("indices32 first={first:?}");
    }
    let active_bones = reader.i32()?;
    println!("active_bones={active_bones} at={}", reader.position);
    reader.bytes(active_bones as usize * 2)?;
    let size = reader.i32()?;
    let num_vertices = reader.i32()?;
    println!(
        "size={size} num_vertices={num_vertices} at={}",
        reader.position
    );
    let required_bones = reader.i32()?;
    println!("required_bones={required_bones} at={}", reader.position);
    reader.bytes(required_bones as usize * 2)?;
    let mesh_to_import = reader.i32()?;
    println!("mesh_to_import={mesh_to_import}");
    if (0..=1_000_000).contains(&mesh_to_import) {
        reader.bytes(mesh_to_import as usize * 4)?;
    }
    let max_import_vertex = reader.i32()?;
    let num_tex_coords = reader.i32()?;
    println!("max_import_vertex={max_import_vertex} num_tex_coords={num_tex_coords}");
    let vb_strip = reader.bytes(2)?;
    let vb_num_tex = reader.i32()?;
    let vb_full = reader.i32()?;
    let extension = reader.bytes(12)?;
    let origin = reader.bytes(12)?;
    let vert_count = reader.i32()?;
    println!(
        "vertex_buffer strip={vb_strip:02X?} tex={vb_num_tex} full={vb_full} extension={extension:02X?} origin={origin:02X?} count={vert_count} at={}",
        reader.position
    );
    Ok(())
}
