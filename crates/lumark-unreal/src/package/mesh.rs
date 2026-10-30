//! StaticMesh 几何读取（首个 LOD）。
//!
//! 当前面向 UE5.6+ cooked Zen 资产：解析 `FPositionVertexBuffer`、索引缓冲和
//! section；内联顶点数据是 UE5.6/5.8 fixture 的主要布局。3D 查看器只依赖
//! 位置和三角形，因此暂不读取切线、UV、颜色和后续 LOD。

use super::{Mappings, PackageError, UnrealPackage, invalid};

/// 一个 section。
#[derive(Debug, Clone, Copy)]
pub struct MeshSection {
    /// 材质槽索引。
    pub material_index: i32,
    /// 在索引缓冲中的起始索引。
    pub first_index: i32,
    /// 三角形数量。
    pub triangle_count: i32,
}

/// 一个 LOD 的 CPU 几何。
#[derive(Debug, Clone, Default)]
pub struct MeshLod {
    /// 顶点位置（UE5 LWC，f64）。
    pub positions: Vec<[f64; 3]>,
    /// 三角形索引。
    pub indices: Vec<u32>,
    /// section。
    pub sections: Vec<MeshSection>,
}

impl MeshLod {
    /// 返回三角形数量。
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// 一个已读取的 StaticMesh。
#[derive(Debug, Clone)]
pub struct StaticMesh {
    /// 资产名。
    pub name: String,
    /// 首个 LOD。
    pub lod: MeshLod,
}

/// 从包中读取指定导出的 StaticMesh（仅首个 LOD）。
pub fn read_static_mesh(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<StaticMesh, PackageError> {
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or_else(|| invalid("StaticMesh 导出索引越界"))?;
    if export.class_name != "StaticMesh" {
        return Err(PackageError::Unsupported(format!(
            "{} 不是 StaticMesh",
            export.name
        )));
    }
    let properties = package.read_export_properties(export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("StaticMesh 导出数据越界"))?;
    let mut reader = MeshReader {
        data,
        position: properties.bytes_read,
    };
    // UObject hasGuid。
    if reader.bool32()? {
        reader.skip(16)?;
    }
    // FStripDataFlags。
    let strip_global = reader.u8()?;
    let _strip_class = reader.u8()?;
    if strip_global & 2 != 0 {
        return Err(PackageError::Unsupported(
            "StaticMesh 的可视数据已被 strip".to_string(),
        ));
    }
    let cooked = reader.bool32()?;
    if !cooked {
        return Err(PackageError::Unsupported(
            "StaticMesh 未 cooked，当前只支持 cooked 几何".to_string(),
        ));
    }
    let _body_setup = reader.i32()?;
    let _nav_collision = reader.i32()?;
    // LightingGuid 在 Sockets 之前序列化（UE4.27+/UE5 cooked 布局）。
    reader.skip(16)?;
    // Sockets：FName 数组的引用数组。
    let socket_count = reader.count(4, 100_000)?;
    reader.skip(socket_count * 4)?;

    // 尝试 1：有 KeepMobileMinLODSettingOnDesktop 的 int；尝试 2：直接 LOD 数组。
    let base = reader.position;
    let mut last_error = None;
    for skip_min_mobile in [true, false] {
        reader.position = base;
        if skip_min_mobile {
            let _ = reader.i32()?;
        }
        match read_lod_array(&mut reader) {
            Ok(lod) => {
                return Ok(StaticMesh {
                    name: export.name.clone(),
                    lod,
                });
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| invalid("无法解析 StaticMesh 的 render data 布局")))
}

/// 读取 LOD 数组并解析首个 LOD。
fn read_lod_array(reader: &mut MeshReader<'_>) -> Result<MeshLod, PackageError> {
    let lod_count = reader.count(4, 16)?;
    if lod_count == 0 {
        return Err(invalid("StaticMesh 没有 LOD"));
    }
    read_lod(reader, 0)
}

/// 读取一个 LOD。
fn read_lod(reader: &mut MeshReader<'_>, index: usize) -> Result<MeshLod, PackageError> {
    // FStaticMeshLODResources
    let _strip_global = reader.u8()?;
    let _strip_class = reader.u8()?;
    let section_count = reader.count(40, 64)?;
    let mut sections = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        let material_index = reader.i32()?;
        let first_index = reader.i32()?;
        let triangle_count = reader.i32()?;
        let _min_vertex = reader.i32()?;
        let _max_vertex = reader.i32()?;
        let _enable_collision = reader.bool32()?;
        let _cast_shadow = reader.bool32()?;
        let _force_opaque = reader.bool32()?;
        let _visible_in_ray_tracing = reader.bool32()?;
        let _affect_distance_field = reader.bool32()?;
        sections.push(MeshSection {
            material_index,
            first_index,
            triangle_count,
        });
    }
    // UE5.6+ SourceMeshBounds：3 个 double × 2 + double。
    reader.skip(56)?;
    let _max_deviation = reader.f32()?;
    let cooked_out = reader.bool32()?;
    let inlined = reader.bool32()?;
    if cooked_out {
        return Err(invalid(format!("StaticMesh LOD {index} 已 cook 掉")));
    }
    // UE5.5+：bHasRayTracingGeometry（代码按 4 字节跳过）。
    reader.skip(4)?;
    if !inlined {
        return Err(PackageError::Unsupported(
            "StaticMesh 顶点数据在 bulk data 中，当前只支持内联 LOD".to_string(),
        ));
    }
    read_inline_buffers(reader, sections)
}

/// 读取内联顶点/索引缓冲。
fn read_inline_buffers(
    reader: &mut MeshReader<'_>,
    sections: Vec<MeshSection>,
) -> Result<MeshLod, PackageError> {
    // SerializeBuffers 的 FStripDataFlags。
    let _strip_global = reader.u8()?;
    let _strip_class = reader.u8()?;
    // FPositionVertexBuffer：stride, count, bulk array<FVector>。
    let position_stride = reader.i32()?;
    let position_count = reader.count(1, 10_000_000)?;
    let position_element_size = reader.i32()?;
    let position_bulk_count = reader.count(1, 10_000_000)?;
    if !matches!(position_stride, 12 | 24)
        || position_element_size != position_stride
        || position_bulk_count != position_count
    {
        return Err(invalid(format!(
            "顶点缓冲布局不匹配：stride={position_stride} element={position_element_size} count={position_count}/{position_bulk_count}"
        )));
    }
    let mut positions = Vec::with_capacity(position_count);
    if position_stride == 12 {
        for _ in 0..position_count {
            positions.push([
                reader.f32()? as f64,
                reader.f32()? as f64,
                reader.f32()? as f64,
            ]);
        }
    } else {
        for _ in 0..position_count {
            positions.push([reader.f64()?, reader.f64()?, reader.f64()?]);
        }
    }

    // FStaticMeshVertexBuffer：metadata + tangent bulk + texcoord bulk。
    let _vertex_strip_global = reader.u8()?;
    let _vertex_strip_class = reader.u8()?;
    let num_tex_coords = reader.i32()?;
    let num_vertices = reader.count(1, 10_000_000)?;
    let _use_full_precision_uvs = reader.bool32()?;
    let _use_high_precision_tangents = reader.bool32()?;
    let tangent_element_size = reader.i32()?;
    let tangent_count = reader.count(1, 10_000_000)?;
    if num_vertices != position_count || tangent_count != num_vertices {
        return Err(invalid("顶点数与切线数不一致"));
    }
    reader.skip(
        (tangent_element_size as usize)
            .checked_mul(tangent_count)
            .ok_or_else(|| invalid("切线数据长度溢出"))?,
    )?;
    if !(0..=8).contains(&num_tex_coords) {
        return Err(invalid(format!("UV 通道数非法：{num_tex_coords}")));
    }
    let uv_element_size = reader.i32()?;
    let uv_count = reader.count(1, 10_000_000)?;
    let uv_bytes = (uv_element_size as usize)
        .checked_mul(uv_count)
        .ok_or_else(|| invalid("UV 数据长度溢出"))?;
    reader.skip(uv_bytes)?;

    // FColorVertexBuffer。
    let _color_strip_global = reader.u8()?;
    let _color_strip_class = reader.u8()?;
    let color_stride = reader.i32()?;
    let color_count = reader.count(1, 10_000_000)?;
    if color_count > 0 {
        let color_element_size = reader.i32()?;
        let color_bulk_count = reader.count(1, 10_000_000)?;
        if color_stride != 4 || color_element_size != 4 || color_bulk_count != color_count {
            return Err(invalid("顶点颜色缓冲布局不匹配"));
        }
        reader.skip(color_bulk_count * 4)?;
    }

    // FRawStaticIndexBuffer：bool + bulk array<byte>。
    let is_32bit = reader.bool32()?;
    let index_element_size = reader.i32()?;
    let index_byte_count = reader.count(1, 100_000_000)?;
    if index_element_size != 1 {
        return Err(invalid("索引 bulk 元素大小不是 1"));
    }
    let index_width = if is_32bit { 4 } else { 2 };
    if index_byte_count % index_width != 0 {
        return Err(invalid("索引字节数不是索引宽度的倍数"));
    }
    let index_count = index_byte_count / index_width;
    if !index_count.is_multiple_of(3) {
        return Err(invalid("索引数量不是 3 的倍数"));
    }
    let mut indices = Vec::with_capacity(index_count);
    if is_32bit {
        for _ in 0..index_count {
            indices.push(reader.u32()?);
        }
    } else {
        for _ in 0..index_count {
            indices.push(reader.u16()? as u32);
        }
    }
    if indices
        .iter()
        .any(|index| *index as usize >= positions.len())
    {
        return Err(invalid("索引引用越界顶点"));
    }
    Ok(MeshLod {
        positions,
        indices,
        sections,
    })
}

/// 网格数据读取器。
struct MeshReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> MeshReader<'a> {
    /// 读取指定长度。
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| invalid("网格数据长度溢出"))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| invalid("网格数据被截断"))?;
        self.position = end;
        Ok(bytes)
    }

    /// 跳过指定长度。
    fn skip(&mut self, length: usize) -> Result<(), PackageError> {
        let _ = self.bytes(length)?;
        Ok(())
    }

    /// 读取 u8。
    fn u8(&mut self) -> Result<u8, PackageError> {
        Ok(self.bytes(1)?[0])
    }

    /// 读取 u16。
    fn u16(&mut self) -> Result<u16, PackageError> {
        Ok(u16::from_le_bytes(
            self.bytes(2)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u32。
    fn u32(&mut self) -> Result<u32, PackageError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i32。
    fn i32(&mut self) -> Result<i32, PackageError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 f32。
    fn f32(&mut self) -> Result<f32, PackageError> {
        Ok(f32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 f64。
    fn f64(&mut self) -> Result<f64, PackageError> {
        Ok(f64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 32 位布尔。
    fn bool32(&mut self) -> Result<bool, PackageError> {
        match self.u32()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(invalid(format!("网格布尔值非法：{value}"))),
        }
    }

    /// 读取数组数量。
    fn count(&mut self, min_size: usize, limit: usize) -> Result<usize, PackageError> {
        let count = self.i32()?;
        if count < 0 || count as usize > limit || count as usize * min_size > self.data.len() {
            return Err(invalid(format!("网格数组数量非法：{count}")));
        }
        Ok(count as usize)
    }
}
