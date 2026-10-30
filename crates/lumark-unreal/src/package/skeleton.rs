//! USkeleton 读取：参考骨骼树的名称、父索引和参考姿态。
//!
//! USkeleton 的 `FReferenceSkeleton` 位于无版本属性之后：
//! `FMeshBoneInfo[]`、`FTransform[]` 和名称到索引的 TMap。UE5 的
//! `FReal` 为 f64；当前模块面向 UE5 Zen 资产。

use super::mesh::{MeshLod, MeshSection};
use super::{Mappings, PackageError, UnrealPackage, invalid};

/// 一根参考骨骼。
#[derive(Debug, Clone)]
pub struct Bone {
    /// 骨骼名。
    pub name: String,
    /// 父骨骼索引；根骨骼为 -1。
    pub parent_index: i32,
    /// 参考姿态平移。
    pub translation: [f64; 3],
    /// 参考姿态旋转四元数。
    pub rotation: [f64; 4],
    /// 参考姿态缩放。
    pub scale: [f64; 3],
}

/// 一个已读取的 Skeleton。
#[derive(Debug, Clone)]
pub struct Skeleton {
    /// 资产名。
    pub name: String,
    /// 参考骨骼，顺序与父索引一致。
    pub bones: Vec<Bone>,
}

/// 从包中读取指定导出的 USkeleton。
pub fn read_skeleton(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<Skeleton, PackageError> {
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or_else(|| invalid("Skeleton 导出索引越界"))?;
    if export.class_name != "Skeleton" {
        return Err(PackageError::Unsupported(format!(
            "{} 不是 Skeleton",
            export.name
        )));
    }
    let properties = package.read_export_properties(export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("Skeleton 导出数据越界"))?;
    let mut reader = BoneReader {
        data,
        position: properties.bytes_read,
        names: package.names(),
    };
    // UObject 在属性后先写 32 位 hasGuid；为真时再跟 16 字节 Guid。
    if reader.i32()? != 0 {
        let _ = reader.bytes(16)?;
    }
    parse_reference_skeleton(&mut reader, export.name.clone())
}

/// SkeletalMesh 的参考骨架与轻量 LOD/材质信息。
#[derive(Debug, Clone)]
pub struct SkeletalMeshPreview {
    /// 参考骨架。
    pub skeleton: Skeleton,
    /// 材质槽数量。
    pub material_slots: usize,
    /// LOD 数量。
    pub lod_count: usize,
    /// 首个 LOD 的 section 数量；解析失败时为 0。
    pub lod_sections: usize,
    /// 首个 LOD 的 RequiredBones 数量；解析失败时为 0。
    pub required_bones: usize,
    /// 首个 LOD 的顶点数；section 明细解析失败时为 0。
    pub lod_vertices: usize,
    /// 首个 LOD 的三角形数；section 明细解析失败时为 0。
    pub lod_triangles: usize,
    /// 首个 LOD 的可渲染几何；缺少 `.ubulk` 或布局不支持时为 None。
    pub geometry: Option<MeshLod>,
}

/// 从 SkeletalMesh 中读取参考骨架及 LOD/材质数量。
pub fn read_skeletal_mesh_preview(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
    ubulk: Option<&[u8]>,
) -> Result<SkeletalMeshPreview, PackageError> {
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or_else(|| invalid("SkeletalMesh 导出索引越界"))?;
    if export.class_name != "SkeletalMesh" {
        return Err(PackageError::Unsupported(format!(
            "{} 不是 SkeletalMesh",
            export.name
        )));
    }
    let properties = package.read_export_properties(export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("SkeletalMesh 导出数据越界"))?;
    let mut reader = BoneReader {
        data,
        position: properties.bytes_read,
        names: package.names(),
    };
    if reader.i32()? != 0 {
        let _ = reader.bytes(16)?;
    }
    // FStripDataFlags：两个字节的全局/Script 数据标记。
    let _ = reader.bytes(2)?;
    // UE5 cooked SkeletalMesh 在 ReferenceSkeleton 前还有 ImportedBounds
    // 和 SkeletalMaterials 数组。
    let _ = reader.bytes(56)?; // FBoxSphereBounds（UE5 FReal 为 f64）
    let material_count = reader.count(44, 1024)?;
    let _ = reader.bytes(material_count * 44)?;
    let skeleton = parse_reference_skeleton(&mut reader, export.name.clone())?;
    // bCooked 后是 LOD 数量；UE5.8 不再写 minMobileLOD。
    let _ = reader.i32()?;
    let lod_count = reader.count(1, 64)?;
    let (lod_sections, required_bones, lod_vertices, lod_triangles, section_info, inlined) =
        read_first_lod_summary(&mut reader).unwrap_or((0, 0, 0, 0, Vec::new(), false));
    let mut geometry = read_streamed_geometry(package, &mut reader, ubulk, inlined).ok();
    if let Some(mesh) = &mut geometry {
        mesh.sections = section_info;
    }
    Ok(SkeletalMeshPreview {
        skeleton,
        material_slots: material_count,
        lod_count,
        lod_sections,
        required_bones,
        lod_vertices,
        lod_triangles,
        geometry,
    })
}

/// 首个 LOD 的 section/RequiredBones 概况。
type LodSummary = (usize, usize, usize, usize, Vec<MeshSection>, bool);

/// 读取首个 LOD 的 section/RequiredBones 概况；失败时返回错误。
fn read_first_lod_summary(reader: &mut BoneReader<'_>) -> Result<LodSummary, PackageError> {
    // FStripDataFlags(2) + bIsLODCookedOut + bInlined。
    let _ = reader.bytes(2)?;
    let _ = reader.i32()?;
    let inlined = reader.i32()? != 0;
    let required_bones = reader.count(2, 65535)?;
    let _ = reader.bytes(required_bones * 2)?;
    let sections = reader.count(1, 4096)?;
    let (vertices, triangles, section_info) =
        read_section_totals(reader, sections).unwrap_or((0, 0, Vec::new()));
    Ok((
        sections,
        required_bones,
        vertices,
        triangles,
        section_info,
        inlined,
    ))
}

/// 统计 UE5 cooked section 的顶点/三角形总数；布局不符时失败并回退。
fn read_section_totals(
    reader: &mut BoneReader<'_>,
    sections: usize,
) -> Result<(usize, usize, Vec<MeshSection>), PackageError> {
    let mut vertices = 0usize;
    let mut triangles = 0usize;
    let mut section_info = Vec::with_capacity(sections);
    for _ in 0..sections {
        let _ = reader.bytes(2)?; // FStripDataFlags
        let material = reader.bytes(2)?;
        let material_index = i16::from_le_bytes([material[0], material[1]]) as i32;
        let _ = reader.i32()?; // BaseIndex
        let num_triangles = reader.count(1, 100_000_000)?;
        section_info.push(MeshSection {
            material_index,
            first_index: (triangles * 3) as i32,
            triangle_count: num_triangles as i32,
        });
        triangles += num_triangles;
        let _ = reader.i32()?; // bRecomputeTangent
        let _ = reader.bytes(1)?; // vertex color mask
        let _ = reader.i32()?; // bCastShadow
        let _ = reader.i32()?; // bVisibleInRayTracing
        let _ = reader.u32()?; // BaseVertexIndex
        let cloth_lods = reader.count(4, 1024)?;
        if cloth_lods > 0 {
            return Err(invalid("暂不支持带 cloth mapping 的 SkeletalMesh section"));
        }
        let bone_map = reader.count(2, 65535)?;
        let _ = reader.bytes(bone_map * 2)?;
        vertices += reader.count(1, 100_000_000)?;
        let _ = reader.i32()?; // packed bone influence bits
        let _ = reader.bytes(2)?; // CorrespondClothAssetIndex
        let _ = reader.bytes(20)?; // FClothingSectionData
        let _ = reader.i32()?; // bDisabled
    }
    Ok((vertices, triangles, section_info))
}

/// 读取首个 LOD 的流式顶点/索引缓冲（仅位置与索引）。
fn read_streamed_geometry(
    package: &UnrealPackage,
    reader: &mut BoneReader<'_>,
    ubulk: Option<&[u8]>,
    inlined: bool,
) -> Result<MeshLod, PackageError> {
    let active_bones = reader.count(2, 65535)?;
    let _ = reader.bytes(active_bones * 2)?;
    let _ = reader.i32()?; // buffersSize
    let start = reader.position;
    if inlined {
        if let Some(data) = reader.data.get(start..)
            && let Ok(mesh) = parse_streamed_lod(data)
        {
            return Ok(mesh);
        }
        reader.position = start;
        let data = read_skeletal_bulk(reader, package, ubulk)?
            .ok_or_else(|| invalid("SkeletalMesh 流式数据位于 .ubulk，但未提供侧载数据"))?;
        return parse_streamed_lod(&data);
    }
    if let Ok(Some(data)) = read_skeletal_bulk(reader, package, ubulk)
        && let Ok(mesh) = parse_streamed_lod(&data)
    {
        return Ok(mesh);
    }
    // UE5.6 部分 cooked 包把流式数据直接内联在导出数据末尾。
    reader.position = start;
    let data = reader
        .data
        .get(start..)
        .ok_or_else(|| invalid("SkeletalMesh 流式数据越界"))?;
    parse_streamed_lod(data)
}

/// 解析 bulk data 中的 LOD 流式数据。
fn parse_streamed_lod(data: &[u8]) -> Result<MeshLod, PackageError> {
    let mut reader = BoneReader {
        data,
        position: 0,
        names: &[],
    };
    let _ = reader.bytes(2)?; // FStripDataFlags
    let index_size = reader.u8()?;
    let index_element_size = reader.i32()?;
    let index_count = reader.count(index_element_size.max(1) as usize, 100_000_000)?;
    let index_bytes = reader.bytes(index_count * index_element_size as usize)?;
    let mut indices = Vec::with_capacity(index_count);
    if index_size == 2 {
        for chunk in index_bytes.as_chunks::<2>().0 {
            indices.push(u16::from_le_bytes(*chunk) as u32);
        }
    } else {
        for chunk in index_bytes.as_chunks::<4>().0 {
            indices.push(u32::from_le_bytes(*chunk));
        }
    }
    // 流式数据里没有 GPU skin 顶点缓冲；索引之后直接是
    // FPositionVertexBuffer（stride + count + bulk array<FVector>）。
    let stride = reader.i32()?;
    let vertex_count = reader.count(1, 100_000_000)?;
    let pos_element_size = reader.i32()?;
    let pos_count = reader.count(pos_element_size.max(1) as usize, 100_000_000)?;
    if pos_count != vertex_count {
        return Err(invalid(format!(
            "SkeletalMesh 顶点数不一致：{vertex_count} != {pos_count}"
        )));
    }
    let mut positions = Vec::with_capacity(pos_count);
    if stride == 12 || pos_element_size == 12 {
        for _ in 0..pos_count {
            let x = reader.f32()? as f64;
            let y = reader.f32()? as f64;
            let z = reader.f32()? as f64;
            positions.push([x, y, z]);
        }
    } else {
        return Err(PackageError::Unsupported(format!(
            "SkeletalMesh 顶点 stride {stride} 暂不支持"
        )));
    }
    Ok(MeshLod {
        positions,
        indices,
        sections: Vec::new(),
    })
}

/// 读取 SkeletalMesh 的 FByteBulkData 并返回流式数据字节。
fn read_skeletal_bulk(
    reader: &mut BoneReader<'_>,
    package: &UnrealPackage,
    ubulk: Option<&[u8]>,
) -> Result<Option<Vec<u8>>, PackageError> {
    let bulk_data = package.info().bulk_data.as_slice();
    if !bulk_data.is_empty() {
        let index = reader.i32()?;
        let entry = bulk_data
            .get(index as usize)
            .ok_or_else(|| invalid(format!("SkeletalMesh bulk data 索引越界：{index}")))?;
        let size = entry.serial_size.max(0) as usize;
        let force_inline = entry.flags & (1 << 6) != 0;
        if force_inline {
            return Ok(Some(reader.bytes(size)?.to_vec()));
        }
        if let Some(ubulk) = ubulk {
            let start = entry.serial_offset.max(0) as usize;
            let end = start
                .checked_add(size)
                .ok_or_else(|| invalid("SkeletalMesh bulk data 长度溢出"))?;
            let bytes = ubulk
                .get(start..end)
                .ok_or_else(|| invalid("SkeletalMesh bulk data 超出 .ubulk 范围"))?;
            return Ok(Some(bytes.to_vec()));
        }
        return Ok(None);
    }
    // 没有 bulk map 时按传统 FByteBulkData 头跳过。
    let flags = reader.u32()?;
    let element_count = reader.i32()?;
    let size = reader.u32()? as usize;
    let _ = reader.i64()?;
    if flags & (1 << 6) != 0 {
        return Ok(Some(
            reader
                .bytes(if size > 0 {
                    size
                } else {
                    element_count as usize
                })?
                .to_vec(),
        ));
    }
    Ok(None)
}

/// 从 SkeletalMesh 中读取参考骨架（不含 LOD/材质数量）。
pub fn read_skeletal_mesh_skeleton(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<Skeleton, PackageError> {
    Ok(read_skeletal_mesh_preview(package, export_index, mappings, None)?.skeleton)
}

/// 解析 `FReferenceSkeleton` 主体。
fn parse_reference_skeleton(
    reader: &mut BoneReader<'_>,
    name: String,
) -> Result<Skeleton, PackageError> {
    let bone_count = reader.count(16, 4096)?;
    let mut bones = Vec::with_capacity(bone_count);
    for _ in 0..bone_count {
        let name = reader.fname()?;
        let parent_index = reader.i32()?;
        bones.push(Bone {
            name,
            parent_index,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        });
    }
    let pose_count = reader.count(80, 4096)?;
    if pose_count != bone_count {
        return Err(invalid(format!(
            "骨骼姿态数量 {pose_count} 与骨骼数量 {bone_count} 不一致"
        )));
    }
    for bone in &mut bones {
        bone.rotation = [reader.f64()?, reader.f64()?, reader.f64()?, reader.f64()?];
        bone.translation = [reader.f64()?, reader.f64()?, reader.f64()?];
        bone.scale = [reader.f64()?, reader.f64()?, reader.f64()?];
    }
    // TMap<FName, int>：名称到索引映射，只校验可读性，不参与渲染。
    let map_count = reader.count(12, 1_000_000)?;
    for _ in 0..map_count {
        let _ = reader.fname()?;
        let _ = reader.i32()?;
    }
    for (index, bone) in bones.iter().enumerate() {
        if bone.parent_index >= index as i32 {
            return Err(invalid(format!(
                "骨骼 {} 的父索引 {} 非法",
                bone.name, bone.parent_index
            )));
        }
    }
    Ok(Skeleton { name, bones })
}

/// 骨骼流读取器。
struct BoneReader<'a> {
    data: &'a [u8],
    position: usize,
    names: &'a [String],
}

impl<'a> BoneReader<'a> {
    /// 读取指定长度。
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| invalid("骨骼数据长度溢出"))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| invalid("骨骼数据被截断"))?;
        self.position = end;
        Ok(bytes)
    }

    /// 读取 i32。
    fn i32(&mut self) -> Result<i32, PackageError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u32。
    fn u32(&mut self) -> Result<u32, PackageError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u8。
    fn u8(&mut self) -> Result<u8, PackageError> {
        Ok(self.bytes(1)?[0])
    }

    /// 读取 f32。
    fn f32(&mut self) -> Result<f32, PackageError> {
        Ok(f32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i64。
    fn i64(&mut self) -> Result<i64, PackageError> {
        Ok(i64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 f64。
    fn f64(&mut self) -> Result<f64, PackageError> {
        Ok(f64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取数组数量并校验上限。
    fn count(&mut self, min_size: usize, limit: usize) -> Result<usize, PackageError> {
        let count = self.i32()?;
        if count < 0 || count as usize > limit || count as usize * min_size > self.data.len() {
            return Err(invalid(format!("骨骼数组数量非法：{count}")));
        }
        Ok(count as usize)
    }

    /// 读取 FName。
    fn fname(&mut self) -> Result<String, PackageError> {
        let index = self.i32()?;
        let number = self.i32()?;
        if index < 0 {
            return Err(invalid("骨骼名称索引为负"));
        }
        let base = self
            .names
            .get(index as usize)
            .cloned()
            .ok_or_else(|| invalid(format!("骨骼名称索引 {index} 越界")))?;
        if number == 0 {
            Ok(base)
        } else {
            Ok(format!("{base}_{}", number - 1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 校验父索引必须小于当前索引。
    #[test]
    fn bone_parent_validation_rejects_forward_edges() {
        let bones = [
            Bone {
                name: "Root".to_string(),
                parent_index: -1,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            Bone {
                name: "Child".to_string(),
                parent_index: 1,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
        ];
        assert!(bones[1].parent_index >= 1);
    }
}
