//! 旧式 UE4 `FReferenceSkeleton` 读取（Skeleton/SkeletalMesh）。
//!
//! FF7R 等 UE4.18 资产使用 f32 `FTransform`，且 SkeletalMesh 的内嵌参考骨架
//! 可能位于未知原生前缀之后；这里以范围扫描和严格校验提取骨架。

use super::legacy::LegacyPackage;
use super::mesh::{MeshLod, MeshSection};
use super::skeleton::{Bone, Skeleton, SkinWeights};
use super::{Mappings, PackageError, invalid};

/// 旧式 SkeletalMesh 的原生摘要（骨架与 LOD/section 数）。
#[derive(Debug, Clone)]
pub struct LegacySkeletalMeshSummary {
    /// 内嵌参考骨架。
    pub skeleton: Skeleton,
    /// 原生数据声明的 LOD 数。
    pub lod_count: usize,
    /// 已探测到的各 LOD section 数（当前只填首个 LOD）。
    pub section_counts: Vec<usize>,
    /// 参考骨架结束位置（原生数据中 LOD 数组起点）。
    pub skeleton_end: usize,
}

/// 从旧式 UE4 包中读取 USkeleton；FF7R 的参考姿态使用 f32。
pub fn read_legacy_skeleton(
    package: &LegacyPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<Skeleton, PackageError> {
    let export = package
        .exports()
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
    let mut reader = LegacySkeletonReader {
        data,
        position: properties.bytes_read,
        names: package.names(),
    };
    if reader.i32()? != 0 {
        let _ = reader.bytes(16)?;
    }
    parse_legacy_reference_skeleton(&mut reader, export.name.clone(), false)
}

/// 从旧式 SkeletalMesh 数据中探测内嵌的 `FReferenceSkeleton`。
pub fn read_legacy_skeletal_mesh_skeleton(
    package: &LegacyPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<Skeleton, PackageError> {
    Ok(scan_legacy_skeletal_mesh_skeleton(package, export_index, mappings)?.0)
}

/// 读取旧式 SkeletalMesh 的骨架及后续 LOD/section 数量。
pub fn read_legacy_skeletal_mesh_summary(
    package: &LegacyPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<LegacySkeletalMeshSummary, PackageError> {
    let (skeleton, position) = scan_legacy_skeletal_mesh_skeleton(package, export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("SkeletalMesh 导出数据越界"))?;
    let mut reader = LegacySkeletonReader {
        data,
        position,
        names: package.names(),
    };
    let lod_count = reader.count(1, 64)?;
    let section_counts = if lod_count > 0 {
        // FStaticLODModel 以 2 字节 strip flags 开头，随后是 section 数组。
        let _ = reader.bytes(2)?;
        vec![reader.count(1, 4096)?]
    } else {
        Vec::new()
    };
    Ok(LegacySkeletalMeshSummary {
        skeleton,
        lod_count,
        section_counts,
        skeleton_end: position,
    })
}

/// 旧式 SkeletalMesh 的首个 LOD 几何。
#[derive(Debug, Clone)]
pub struct LegacySkeletalMeshGeometry {
    /// 内嵌参考骨架。
    pub skeleton: Skeleton,
    /// 首个 LOD 的可渲染几何。
    pub mesh: MeshLod,
    /// 原生数据声明的 LOD 数。
    pub lod_count: usize,
    /// 首个 LOD 的 section 数。
    pub section_count: usize,
    /// 首个 LOD 的蒙皮权重（原生数据可读时）。
    pub skin: Option<SkinWeights>,
}

/// 解析旧式 SkeletalMesh 的首个 LOD 索引、位置和 section。
pub fn read_legacy_skeletal_mesh_geometry(
    package: &LegacyPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<LegacySkeletalMeshGeometry, PackageError> {
    let (skeleton, position) = scan_legacy_skeletal_mesh_skeleton(package, export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("SkeletalMesh 导出数据越界"))?;
    let mut reader = LegacySkeletonReader {
        data,
        position,
        names: package.names(),
    };
    let lod_count = reader.count(1, 64)?;
    if lod_count == 0 {
        return Err(PackageError::Unsupported(
            "SkeletalMesh 没有原生 LOD".to_string(),
        ));
    }
    let _ = reader.bytes(2)?;
    let section_count = reader.count(1, 4096)?;
    let mut sections = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        sections.push(parse_legacy_section(&mut reader)?);
    }
    let indices = parse_legacy_indices(&mut reader)?;
    // ActiveBoneIndices
    let active_bones = reader.count(2, 4096)?;
    let _ = reader.bytes(active_bones * 2)?;
    let _ = reader.i32()?; // Size
    let num_vertices = reader.count(1, 10_000_000)?;
    // RequiredBones
    let required_bones = reader.count(2, 4096)?;
    let required_bone_bytes = reader.bytes(required_bones * 2)?;
    let required_bone_map = required_bone_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect::<Vec<_>>();
    // MeshToImportVertexMap + MaxImportVertex
    let import_map = reader.count(4, 10_000_000)?;
    let _ = reader.bytes(import_map * 4)?;
    let _ = reader.i32()?;
    let _ = reader.i32()?; // NumTexCoords
    // FSkeletalMeshVertexBuffer：strip、NumTexCoords、bool、extension/origin、bulk。
    let _ = reader.bytes(2)?;
    let _ = reader.i32()?;
    let _ = reader.i32()?;
    let _ = reader.bytes(12)?;
    let _ = reader.bytes(12)?;
    let element_size = reader.i32()?;
    let element_count = reader.count(element_size.max(1) as usize, 10_000_000)?;
    if element_count != num_vertices {
        return Err(invalid(format!(
            "SkeletalMesh 原生顶点数 {element_count} 与声明 {num_vertices} 不一致"
        )));
    }
    if element_size < 24 {
        return Err(PackageError::Unsupported(format!(
            "SkeletalMesh 原生顶点 stride {element_size} 暂不支持"
        )));
    }
    let vertex_data = reader.bytes(element_count * element_size as usize)?;
    let mut positions = Vec::with_capacity(element_count);
    for vertex in vertex_data.chunks_exact(element_size as usize) {
        let x = f32::from_le_bytes(vertex[12..16].try_into().unwrap_or_default());
        let y = f32::from_le_bytes(vertex[16..20].try_into().unwrap_or_default());
        let z = f32::from_le_bytes(vertex[20..24].try_into().unwrap_or_default());
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(invalid("SkeletalMesh 原生顶点包含非法浮点"));
        }
        positions.push([x as f64, y as f64, z as f64]);
    }
    if let Some(max_index) = indices.iter().max()
        && *max_index as usize >= positions.len()
    {
        return Err(invalid("SkeletalMesh 原生索引超出顶点范围"));
    }
    let skin = parse_legacy_skins(
        &mut reader,
        positions.len(),
        &required_bone_map,
        skeleton.bones.len(),
    )
    .ok();
    let mesh_sections = sections
        .into_iter()
        .map(|section| MeshSection {
            material_index: section.0,
            first_index: section.1,
            triangle_count: section.2,
        })
        .collect();
    Ok(LegacySkeletalMeshGeometry {
        skeleton,
        mesh: MeshLod {
            positions,
            indices,
            sections: mesh_sections,
            attributes: Default::default(),
        },
        lod_count,
        section_count,
        skin,
    })
}

/// 解析旧式 `FSkinWeightVertexBuffer`。
fn parse_legacy_skins(
    reader: &mut LegacySkeletonReader<'_>,
    num_vertices: usize,
    required_bones: &[u16],
    skeleton_bones: usize,
) -> Result<SkinWeights, PackageError> {
    let _ = reader.bytes(2)?; // data strip flags
    let extra_bone_influences = reader.i32()? != 0;
    let count = reader.count(1, 10_000_000)?;
    if count != num_vertices {
        return Err(invalid("SkeletalMesh 蒙皮顶点数不一致"));
    }
    let element_size = reader.i32()?;
    let element_count = reader.count(element_size.max(1) as usize, 10_000_000)?;
    if element_count != num_vertices {
        return Err(invalid("SkeletalMesh 蒙皮元素数不一致"));
    }
    let max_influences: usize = if extra_bone_influences { 8 } else { 4 };
    if (element_size as usize) < max_influences * 2 {
        return Err(PackageError::Unsupported(format!(
            "SkeletalMesh 蒙皮 stride {element_size} 暂不支持"
        )));
    }
    let data = reader.bytes(element_count * element_size as usize)?;
    let mut influences = Vec::with_capacity(element_count);
    let mut weights = Vec::with_capacity(element_count);
    for vertex in data.chunks_exact(element_size as usize) {
        let mut vertex_influences = [0u16; 12];
        let mut vertex_weights = [0u16; 12];
        for influence in 0..max_influences {
            let raw_index = vertex[influence] as u16;
            let mapped = required_bones
                .get(raw_index as usize)
                .copied()
                .unwrap_or(raw_index);
            if mapped as usize >= skeleton_bones {
                return Err(invalid("SkeletalMesh 蒙皮骨骼索引越界"));
            }
            vertex_influences[influence] = mapped;
            vertex_weights[influence] = vertex[max_influences + influence] as u16;
        }
        influences.push(vertex_influences);
        weights.push(vertex_weights);
    }
    Ok(SkinWeights {
        vertices: element_count,
        max_influences,
        variable: false,
        influences,
        weights,
    })
}

/// 解析一个旧式 `FSkelMeshSection`，返回材质/起始索引/三角形数。
fn parse_legacy_section(
    reader: &mut LegacySkeletonReader<'_>,
) -> Result<(i32, i32, i32), PackageError> {
    let _ = reader.bytes(2)?; // 内部 strip flags
    let material = reader.i16()? as i32;
    let base_index = reader.i32()?;
    let triangles = reader.i32()?;
    let _ = reader.bytes(1)?; // sorting
    let _ = reader.i32()?; // legacy cloth bool
    let _ = reader.bytes(2)?; // cloth section
    let _ = reader.bytes(1)?; // cloth LOD
    let _ = reader.i32()?; // recompute tangent
    let _ = reader.i32()?; // cast shadow
    let _ = reader.i32()?; // base vertex
    let bone_map = reader.count(2, 4096)?;
    let _ = reader.bytes(bone_map * 2)?;
    let _ = reader.i32()?; // num vertices
    let _ = reader.i32()?; // max influences
    let _ = reader.i32()?; // cloth mapping array
    let physical_verts = reader.i32()?;
    let physical_normals = reader.i32()?;
    if physical_verts != 0 || physical_normals != 0 {
        return Err(PackageError::Unsupported(
            "SkeletalMesh section 带物理网格数据，暂不支持".to_string(),
        ));
    }
    let _ = reader.bytes(2)?; // cloth asset index
    let _ = reader.bytes(20)?; // clothing data
    let ff7_should_read = reader.i32()?;
    let ff7_length = reader.i32()?;
    if ff7_should_read == 1 && ff7_length > 0 {
        let _ = reader.bytes(ff7_length as usize * 16)?;
    }
    Ok((material, base_index, triangles))
}

/// 解析 `FMultisizeIndexContainer`。
fn parse_legacy_indices(reader: &mut LegacySkeletonReader<'_>) -> Result<Vec<u32>, PackageError> {
    let data_size = reader.u8()? as usize;
    let _ = reader.i32()?; // bulk element size
    let count = reader.count(data_size.max(1), 100_000_000)?;
    let bytes = reader.bytes(count * data_size)?;
    let mut indices = Vec::with_capacity(count);
    if data_size == 2 {
        for chunk in bytes.as_chunks::<2>().0 {
            indices.push(u16::from_le_bytes(*chunk) as u32);
        }
    } else if data_size == 4 {
        for chunk in bytes.as_chunks::<4>().0 {
            indices.push(u32::from_le_bytes(*chunk));
        }
    } else {
        return Err(PackageError::Unsupported(format!(
            "SkeletalMesh 索引宽度 {data_size} 非法"
        )));
    }
    Ok(indices)
}

/// 扫描并返回内嵌参考骨架及其结束偏移。
fn scan_legacy_skeletal_mesh_skeleton(
    package: &LegacyPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<(Skeleton, usize), PackageError> {
    let export = package
        .exports()
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
    let start = properties.bytes_read;
    let end = start.saturating_add(65536).min(data.len());
    for offset in start..end {
        for export_names in [false, true] {
            let mut probe = LegacySkeletonReader {
                data,
                position: offset,
                names: package.names(),
            };
            let Ok(skeleton) =
                parse_legacy_reference_skeleton(&mut probe, export.name.clone(), export_names)
            else {
                continue;
            };
            if skeleton.bones.len() >= 2
                && skeleton
                    .bones
                    .first()
                    .is_some_and(|bone| bone.parent_index == -1)
            {
                return Ok((skeleton, probe.position));
            }
        }
    }
    Err(PackageError::Unsupported(
        "SkeletalMesh 内嵌参考骨架未找到".to_string(),
    ))
}

/// 解析 UE4 旧式 `FReferenceSkeleton`（f32 FTransform）。
fn parse_legacy_reference_skeleton(
    reader: &mut LegacySkeletonReader<'_>,
    name: String,
    export_names: bool,
) -> Result<Skeleton, PackageError> {
    let bone_count = reader.count(12, 4096)?;
    let mut bones = Vec::with_capacity(bone_count);
    for _ in 0..bone_count {
        let bone_name = reader.fname()?;
        let parent_index = reader.i32()?;
        if export_names {
            skip_bone_export_name(reader)?;
        }
        bones.push(Bone {
            name: bone_name,
            parent_index,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        });
    }
    let pose_count = reader.count(40, 4096)?;
    if pose_count != bone_count {
        return Err(invalid(format!(
            "旧式骨骼姿态数量 {pose_count} 与骨骼数量 {bone_count} 不一致"
        )));
    }
    for bone in &mut bones {
        bone.rotation = [
            reader.f32()? as f64,
            reader.f32()? as f64,
            reader.f32()? as f64,
            reader.f32()? as f64,
        ];
        bone.translation = [
            reader.f32()? as f64,
            reader.f32()? as f64,
            reader.f32()? as f64,
        ];
        bone.scale = [
            reader.f32()? as f64,
            reader.f32()? as f64,
            reader.f32()? as f64,
        ];
    }
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

/// 跳过旧式骨骼的导出名字符串。
fn skip_bone_export_name(reader: &mut LegacySkeletonReader<'_>) -> Result<(), PackageError> {
    let length = reader.i32()?;
    if length == 0 {
        return Ok(());
    }
    let cells = length.unsigned_abs() as usize;
    let bytes = cells
        .checked_mul(if length < 0 { 2 } else { 1 })
        .ok_or_else(|| invalid("骨骼导出名长度溢出"))?;
    let _ = reader.bytes(bytes)?;
    Ok(())
}

/// 旧式原生数据读取器。
struct LegacySkeletonReader<'a> {
    data: &'a [u8],
    position: usize,
    names: &'a [String],
}

impl<'a> LegacySkeletonReader<'a> {
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| invalid("旧式骨骼数据长度溢出"))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| invalid("旧式骨骼数据被截断"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, PackageError> {
        Ok(self.bytes(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, PackageError> {
        Ok(i16::from_le_bytes(
            self.bytes(2)?.try_into().unwrap_or_default(),
        ))
    }

    fn i32(&mut self) -> Result<i32, PackageError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn f32(&mut self) -> Result<f32, PackageError> {
        Ok(f32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn fname(&mut self) -> Result<String, PackageError> {
        let index = self.i32()?;
        let number = self.i32()?;
        if index < 0 {
            return Ok(format!("Name({index})"));
        }
        let base = self
            .names
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("Name({index})"));
        if number <= 0 {
            Ok(base)
        } else {
            Ok(format!("{base}_{}", number - 1))
        }
    }

    fn count(&mut self, min_size: usize, limit: usize) -> Result<usize, PackageError> {
        let count = self.i32()?;
        if count < 0 || count as usize > limit || count as usize * min_size > self.data.len() {
            return Err(invalid("旧式骨骼数组数量非法"));
        }
        Ok(count as usize)
    }
}
