//! StaticMesh 几何读取（首个 LOD）。
//!
//! 面向 UE5 cooked Zen 资产：解析 `FPositionVertexBuffer`、切线/法线、全部 UV
//! 通道、顶点颜色、索引缓冲和 section。几何缓冲可能内联在导出数据里，也可能
//! 打包成一个 `FByteBulkData` 放在容器 `.ubulk` 中，两种形式都会尝试；无法
//! 识别的可选缓冲会跳过并保留几何，不阻断查看器。

use super::{
    AssetMetadata, Mappings, PackageError, UnrealPackage, bulk::BulkReader, bulk::read_byte_bulk,
    invalid, read_asset_metadata,
};

/// 非内联 LOD 在 bulk 载荷之后还有各缓冲元数据与 `FStaticMeshBuffersSize`。
///
/// 依次为 `DepthOnlyNumTriangles+Packed`（8）、顶点/切线/颜色/索引缓冲头
/// （`4*4 + 2*4 + 2*4 + 5*2*4`）和 `FStaticMeshBuffersSize`（12）。
const LOD_BULK_TRAILING_BYTES: usize = 8 + 4 * 4 + 2 * 4 + 2 * 4 + 5 * 2 * 4 + 12;

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

/// 静态网格材质槽。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialSlot {
    /// 槽位名称（`MaterialSlotName`）。
    pub name: String,
    /// 材质包路径；当前大多由上层按同目录命名约定解析。
    pub package: Option<String>,
}

/// 顶点属性；空 Vec 表示容器没有提供该属性。
#[derive(Debug, Clone, Default)]
pub struct MeshVertexAttributes {
    /// 法线（归一化 xyz）。
    pub normals: Vec<[f32; 3]>,
    /// 切线 xyz + 副切线符号 w。
    pub tangents: Vec<[f32; 4]>,
    /// 每个 UV 通道一组，长度与顶点数一致。
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// 顶点颜色（RGBA）。
    pub colors: Vec<[u8; 4]>,
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
    /// 法线、切线、UV 和顶点颜色。
    pub attributes: MeshVertexAttributes,
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
    /// 资产声明的 LOD 数量。
    pub lod_count: usize,
    /// 成功解析的 LOD 列表（通常至少包含 LOD0）。
    pub lods: Vec<MeshLod>,
    /// 材质槽（来自 `StaticMaterials` 属性，顺序与 section.material_index 对应）。
    pub material_slots: Vec<MaterialSlot>,
    /// 材质槽的 `MaterialInterface` 对象索引（负数为导入，顺序与
    /// `material_slots` 一致）；用于精确解析材质资产。
    pub material_imports: Vec<i32>,
}

/// 从包中读取指定导出的 StaticMesh（仅首个 LOD）。
///
/// 不附带 `.ubulk` 侧载数据；需要解析外部 bulk 几何时使用
/// [`read_static_mesh_with_bulk`]。
pub fn read_static_mesh(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<StaticMesh, PackageError> {
    read_static_mesh_with_bulk(package, export_index, mappings, None)
}

/// 从包中读取指定导出的 StaticMesh，并允许传入容器 `.ubulk` 载荷。
///
/// UE5 cooked 静态网格的几何缓冲通常整体写入一个 `FByteBulkData` 并放在
/// `.ubulk` 中；缺少侧载数据时只能解析内联 LOD。
pub fn read_static_mesh_with_bulk(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
    ubulk: Option<&[u8]>,
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
    let (material_slots, material_imports) =
        match read_asset_metadata(package, export_index, mappings) {
            Ok(AssetMetadata::StaticMesh {
                material_names,
                material_imports,
                ..
            }) => (
                material_names
                    .into_iter()
                    .map(|name| MaterialSlot {
                        name,
                        package: None,
                    })
                    .collect::<Vec<_>>(),
                material_imports,
            ),
            _ => (Vec::new(), Vec::new()),
        };
    let base = reader.position;
    let mut last_error = None;
    for skip_min_mobile in [true, false] {
        reader.position = base;
        if skip_min_mobile {
            let _ = reader.i32()?;
        }
        match read_lod_array(package, &mut reader, ubulk) {
            Ok((lods, lod_count)) => {
                let Some(lod) = lods.first().cloned() else {
                    last_error = Some(invalid("StaticMesh 没有可解析的 LOD"));
                    continue;
                };
                return Ok(StaticMesh {
                    name: export.name.clone(),
                    lod,
                    lod_count,
                    lods,
                    material_slots,
                    material_imports,
                });
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| invalid("无法解析 StaticMesh 的 render data 布局")))
}

/// 读取 LOD 数组并解析尽可能多的 LOD。
fn read_lod_array(
    package: &UnrealPackage,
    reader: &mut MeshReader<'_>,
    ubulk: Option<&[u8]>,
) -> Result<(Vec<MeshLod>, usize), PackageError> {
    let lod_count = reader.count(4, 16)?;
    if lod_count == 0 {
        return Err(invalid("StaticMesh 没有 LOD"));
    }
    let mut lods = Vec::with_capacity(lod_count);
    for index in 0..lod_count {
        match read_lod(package, reader, index, ubulk) {
            Ok(lod) => lods.push(lod),
            Err(error) => {
                if lods.is_empty() {
                    return Err(error);
                }
                break;
            }
        }
    }
    Ok((lods, lod_count))
}

/// LOD 头部的版本差异。
///
/// UE5.6 起在 sections 之后多出 `SourceMeshBounds`，UE5.5 起多出
/// `bHasRayTracingGeometry` 标志；包内不记录对象版本时按候选布局依次尝试。
#[derive(Debug, Clone, Copy)]
struct LodLayout {
    /// 是否带 `SourceMeshBounds`（UE5.6+）。
    source_bounds: bool,
    /// 是否带 `bHasRayTracingGeometry`（UE5.5+）。
    ray_tracing_flag: bool,
}

/// 候选布局：UE5.6 优先，其次 UE5.2–5.4 与 UE5.5。
const LOD_LAYOUTS: [LodLayout; 3] = [
    LodLayout {
        source_bounds: true,
        ray_tracing_flag: true,
    },
    LodLayout {
        source_bounds: false,
        ray_tracing_flag: false,
    },
    LodLayout {
        source_bounds: false,
        ray_tracing_flag: true,
    },
];

/// 读取一个 LOD，按候选布局重试直到某个布局能完整解析。
fn read_lod(
    package: &UnrealPackage,
    reader: &mut MeshReader<'_>,
    index: usize,
    ubulk: Option<&[u8]>,
) -> Result<MeshLod, PackageError> {
    let start = reader.position;
    let mut last_error = None;
    for layout in LOD_LAYOUTS {
        reader.position = start;
        match read_lod_with_layout(package, reader, index, ubulk, layout) {
            Ok(lod) => return Ok(lod),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| invalid(format!("StaticMesh LOD {index} 无法解析"))))
}

/// 按指定布局读取一个 LOD。
fn read_lod_with_layout(
    package: &UnrealPackage,
    reader: &mut MeshReader<'_>,
    index: usize,
    ubulk: Option<&[u8]>,
    layout: LodLayout,
) -> Result<MeshLod, PackageError> {
    // FStaticMeshLODResources
    let strip_global = reader.u8()?;
    let strip_class = reader.u8()?;
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
    if layout.source_bounds {
        // FBoxSphereBounds（double）：Origin、BoxExtent、SphereRadius 共 56 字节。
        reader.skip(56)?;
    }
    let _max_deviation = reader.f32()?;
    let cooked_out = reader.bool32()?;
    let inlined = reader.bool32()?;
    if cooked_out {
        return Err(invalid(format!("StaticMesh LOD {index} 已 cook 掉")));
    }
    if layout.ray_tracing_flag {
        // bHasRayTracingGeometry；后续 RayTracingGeometry 不在本轮范围。
        reader.skip(4)?;
    }
    if inlined {
        let lod = read_inline_buffers(reader, sections, strip_global, strip_class)?;
        // FStaticMeshBuffersSize：SerializedBuffersSize/DepthOnlyIBSize/ReversedIBsSize。
        reader.skip(12)?;
        return Ok(lod);
    }
    // 非内联：整个 LOD 的几何缓冲作为一个 FByteBulkData 存放在 .ubulk 中。
    let bulk = read_byte_bulk(reader, &package.info().bulk_data, ubulk, "StaticMesh 顶点")?;
    let data = bulk.data.ok_or_else(|| {
        PackageError::Unsupported(
            "StaticMesh 顶点数据位于容器 bulk（.ubulk），但本次读取未附带侧载数据".to_string(),
        )
    })?;
    let mut blob = MeshReader {
        data: &data,
        position: 0,
    };
    let lod = read_inline_buffers(&mut blob, sections, strip_global, strip_class)?;
    // 主数据流中随后是各缓冲元数据与 FStaticMeshBuffersSize；跳过失败只影响
    // 后续 LOD，已解析的 LOD 仍然可用。
    let _ = reader.skip(LOD_BULK_TRAILING_BYTES);
    Ok(lod)
}

/// 读取内联顶点/索引缓冲。
fn read_inline_buffers(
    reader: &mut MeshReader<'_>,
    sections: Vec<MeshSection>,
    lod_strip_global: u8,
    lod_strip_class: u8,
) -> Result<MeshLod, PackageError> {
    // SerializeBuffers 的 FStripDataFlags。
    let _ = reader.bytes(2)?;
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

    // FStaticMeshVertexBuffer（strip flags + 元数据 + 切线/UV bulk）；骨骼网格
    // 流式数据复用同一读取函数。
    let (mut attributes, next_position) =
        read_vertex_buffer(reader.data, reader.position, position_count)?;
    reader.position = next_position;

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
        let bytes = reader.bytes(color_bulk_count * 4)?;
        attributes.colors = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
            .collect();
    }

    // 属性数量必须与顶点数一致；异常时整体丢弃该属性，避免下游越界。
    if attributes.normals.len() != positions.len() {
        attributes.normals.clear();
        attributes.tangents.clear();
    }
    if attributes
        .uvs
        .iter()
        .any(|channel| channel.len() != positions.len())
    {
        attributes.uvs.clear();
    }
    if attributes.colors.len() != positions.len() {
        attributes.colors.clear();
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
    // RawIndexBuffer.HasShouldExpandTo32Bit。
    let _ = reader.bool32()?;
    if indices
        .iter()
        .any(|index| *index as usize >= positions.len())
    {
        return Err(invalid("索引引用越界顶点"));
    }
    // 末尾附加索引/采样器仅用于继续解析后续 LOD；失败时保留已解析 LOD。
    let _ = skip_lod_trailing(reader, sections.len(), lod_strip_global, lod_strip_class);
    Ok(MeshLod {
        positions,
        indices,
        sections,
        attributes,
    })
}

/// 读取一个 `FStaticMeshVertexBuffer`：strip flags、元数据、切线与全部 UV。
///
/// 静态网格内联缓冲和骨骼网格流式数据使用同一序列化布局；返回解析后的属性
/// 与缓冲区结束位置。`expected_vertices` 用于校验顶点数一致。
pub(super) fn read_vertex_buffer(
    data: &[u8],
    position: usize,
    expected_vertices: usize,
) -> Result<(MeshVertexAttributes, usize), PackageError> {
    let mut reader = MeshReader { data, position };
    let _ = reader.bytes(2)?; // FStripDataFlags
    let num_tex_coords = reader.i32()?;
    let num_vertices = reader.count(1, 10_000_000)?;
    let use_full_precision_uvs = reader.bool32()?;
    let use_high_precision_tangents = reader.bool32()?;
    let tangent_element_size = reader.i32()?;
    let tangent_count = reader.count(1, 10_000_000)?;
    if num_vertices != expected_vertices || tangent_count != num_vertices {
        return Err(invalid(format!(
            "顶点数与切线数不一致：位置 {expected_vertices} / 顶点缓冲 {num_vertices} / 切线 {tangent_count}"
        )));
    }
    let mut attributes = MeshVertexAttributes::default();
    read_vertex_tangents(
        &mut reader,
        tangent_element_size,
        tangent_count,
        use_high_precision_tangents,
        &mut attributes,
    )?;
    if !(0..=8).contains(&num_tex_coords) {
        return Err(invalid(format!("UV 通道数非法：{num_tex_coords}")));
    }
    let uv_element_size = reader.i32()?;
    let uv_count = reader.count(1, 10_000_000)?;
    read_vertex_uvs(
        &mut reader,
        num_tex_coords as usize,
        num_vertices,
        uv_element_size,
        uv_count,
        use_full_precision_uvs,
        &mut attributes,
    )?;
    Ok((attributes, reader.position))
}

/// 读取切线/法线缓冲。
///
/// UE5 现代 cooked 布局：
/// - 普通精度：每顶点 2×`FPackedNormal`（TangentX、TangentZ），8 字节；
/// - 高精度：每顶点 2×`FPackedRGBA16N`，16 字节。
///
/// 旧布局（3×`FPackedNormal`，12 字节）也兼容读取 X/Y/Z；无法识别的布局会
/// 整体跳过，保留几何而不阻断查看器。
fn read_vertex_tangents(
    reader: &mut MeshReader<'_>,
    element_size: i32,
    count: usize,
    high_precision: bool,
    attributes: &mut MeshVertexAttributes,
) -> Result<(), PackageError> {
    attributes.normals.reserve(count);
    attributes.tangents.reserve(count);
    if high_precision && element_size == 16 {
        for _ in 0..count {
            let tangent = read_rgba16n(reader)?;
            let normal = read_rgba16n(reader)?;
            attributes
                .normals
                .push(normalize3([normal[0], normal[1], normal[2]]));
            attributes.tangents.push(build_tangent(tangent, normal));
        }
        return Ok(());
    }
    if !high_precision && element_size == 8 {
        for _ in 0..count {
            let tangent = decode_packed_normal(reader.u32()?);
            let normal = decode_packed_normal(reader.u32()?);
            attributes
                .normals
                .push(normalize3([normal[0], normal[1], normal[2]]));
            attributes.tangents.push(build_tangent(tangent, normal));
        }
        return Ok(());
    }
    if !high_precision && element_size == 12 {
        for _ in 0..count {
            let tangent = decode_packed_normal(reader.u32()?);
            let _tangent_y = reader.u32()?;
            let normal = decode_packed_normal(reader.u32()?);
            attributes
                .normals
                .push(normalize3([normal[0], normal[1], normal[2]]));
            attributes.tangents.push(build_tangent(tangent, normal));
        }
        return Ok(());
    }
    let bytes = (element_size.max(0) as usize)
        .checked_mul(count)
        .ok_or_else(|| invalid("切线数据长度溢出"))?;
    reader.skip(bytes)?;
    attributes.normals.clear();
    attributes.tangents.clear();
    Ok(())
}

/// 读取全部 UV 通道。
///
/// `BulkSerialize` 的 itemSize 是**单个 UV 元素**的字节数（2×f16=4 或
/// 2×f32=8），itemCount 是“顶点数 × 通道数”；UE 在通道数为奇数时还会填充
/// 一个顶点。布局不符时整体跳过并保留几何。
fn read_vertex_uvs(
    reader: &mut MeshReader<'_>,
    num_tex_coords: usize,
    num_vertices: usize,
    element_size: i32,
    count: usize,
    full_precision: bool,
    attributes: &mut MeshVertexAttributes,
) -> Result<(), PackageError> {
    let element_size_expected = if full_precision { 8 } else { 4 };
    let vertex_count = if num_vertices == 0 {
        0
    } else if count == num_vertices.saturating_mul(num_tex_coords) {
        num_vertices
    } else {
        num_vertices + usize::from(!num_tex_coords.is_multiple_of(2))
    };
    let expected = vertex_count
        .checked_mul(num_tex_coords)
        .ok_or_else(|| invalid("UV 数据长度溢出"))?;
    if element_size as usize != element_size_expected || count != expected {
        let bytes = (element_size.max(0) as usize)
            .checked_mul(count)
            .ok_or_else(|| invalid("UV 数据长度溢出"))?;
        reader.skip(bytes)?;
        attributes.uvs.clear();
        return Ok(());
    }
    let mut uvs: Vec<Vec<[f32; 2]>> = (0..num_tex_coords)
        .map(|_| Vec::with_capacity(num_vertices))
        .collect();
    for vertex in 0..vertex_count {
        for channel in &mut uvs {
            let uv = if full_precision {
                [reader.f32()?, reader.f32()?]
            } else {
                [half_to_f32(reader.u16()?), half_to_f32(reader.u16()?)]
            };
            if vertex < num_vertices {
                channel.push(uv);
            }
        }
    }
    attributes.uvs = uvs;
    Ok(())
}

/// 读取一个 `FPackedRGBA16N`（4 个带偏置的 u16 分量）。
fn read_rgba16n(reader: &mut MeshReader<'_>) -> Result<[f32; 4], PackageError> {
    let mut components = [0.0f32; 4];
    for component in &mut components {
        let raw = reader.u16()? ^ 0x8000;
        *component = (raw as f32 - 32_767.5) / 32_767.5;
    }
    Ok(components)
}

/// 解码 `FPackedNormal`（4 个字节分量，解码为 [-1, 1]）。
pub(super) fn decode_packed_normal(data: u32) -> [f32; 4] {
    // UE4.20+ 使用偏移后的字节编码。
    let data = data ^ 0x8080_8080;
    let decode = |byte: u32| byte as f32 / 127.5 - 1.0;
    [
        decode(data & 0xFF),
        decode((data >> 8) & 0xFF),
        decode((data >> 16) & 0xFF),
        decode(data >> 24),
    ]
}

/// 把切线基转换为渲染/导出使用的 `[x, y, z, 副切线符号]`。
pub(super) fn build_tangent(tangent: [f32; 4], normal: [f32; 4]) -> [f32; 4] {
    let tangent = normalize3([tangent[0], tangent[1], tangent[2]]);
    [
        tangent[0],
        tangent[1],
        tangent[2],
        if normal[3] < 0.0 { -1.0 } else { 1.0 },
    ]
}

/// 归一化三维向量；零向量回退到 +Z。
pub(super) fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length > f32::EPSILON {
        [value[0] / length, value[1] / length, value[2] / length]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// 把 IEEE 754 half 位模式转换为 f32。
pub(super) fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bits >> 10) & 0x1F) as i32;
    let mantissa = (bits & 0x03FF) as f32;
    let value = if exponent == 0 {
        // 次正规数：mantissa × 2^-24。
        mantissa / 16_777_216.0
    } else if exponent == 0x1F {
        if mantissa == 0.0 {
            f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        (1.0 + mantissa / 1024.0) * 2.0f32.powi(exponent - 15)
    };
    sign * value
}

/// 跳过 UE5.6 SerializeBuffers 末尾的索引缓冲、RT 数据和采样器。
fn skip_lod_trailing(
    reader: &mut MeshReader<'_>,
    section_count: usize,
    strip_global: u8,
    strip_class: u8,
) -> Result<(), PackageError> {
    // ReversedIndexBuffer。
    if strip_class & 4 == 0 {
        skip_raw_index_buffer(reader)?;
    }
    // DepthOnlyIndexBuffer。
    skip_raw_index_buffer(reader)?;
    // ReversedDepthOnlyIndexBuffer。
    if strip_class & 4 == 0 {
        skip_raw_index_buffer(reader)?;
    }
    // WireframeIndexBuffer（编辑器数据）。
    if strip_global & 1 == 0 {
        skip_raw_index_buffer(reader)?;
    }
    // UE5.5+ RayTracingGeometry：RawDataHeader(24) + bulk array。
    if strip_class & 8 == 0 {
        reader.skip(24)?;
        let element_size = reader.i32()?;
        let count = reader.count(1, 100_000_000)?;
        let bytes = (element_size.max(0) as usize)
            .checked_mul(count)
            .ok_or_else(|| invalid("RT 几何长度溢出"))?;
        reader.skip(bytes)?;
    }
    // areaWeightedSectionSamplers + areaWeightedSampler。
    for _ in 0..section_count {
        skip_weighted_sampler(reader)?;
    }
    skip_weighted_sampler(reader)
}

/// 跳过 FRawStaticIndexBuffer。
fn skip_raw_index_buffer(reader: &mut MeshReader<'_>) -> Result<(), PackageError> {
    let _is_32bit = reader.bool32()?;
    let element_size = reader.i32()?;
    let count = reader.count(1, 100_000_000)?;
    let bytes = (element_size.max(0) as usize)
        .checked_mul(count)
        .ok_or_else(|| invalid("索引缓冲长度溢出"))?;
    reader.skip(bytes)?;
    // UE5 RawIndexBuffer.HasShouldExpandTo32Bit。
    let _ = reader.bool32()?;
    Ok(())
}

/// 跳过 FWeightedRandomSampler。
fn skip_weighted_sampler(reader: &mut MeshReader<'_>) -> Result<(), PackageError> {
    let float_count = reader.count(4, 100_000_000)?;
    reader.skip(float_count * 4)?;
    let int_count = reader.count(4, 100_000_000)?;
    reader.skip(int_count * 4)?;
    reader.skip(4)?;
    Ok(())
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

impl BulkReader for MeshReader<'_> {
    /// 返回当前读取位置。
    fn position(&self) -> usize {
        self.position
    }

    /// 复位读取位置。
    fn set_position(&mut self, position: usize) {
        self.position = position;
    }

    /// 读取指定长度。
    fn take(&mut self, length: usize) -> Result<&[u8], PackageError> {
        self.bytes(length)
    }

    /// 读取 u32。
    fn read_u32(&mut self) -> Result<u32, PackageError> {
        self.u32()
    }

    /// 读取 i32。
    fn read_i32(&mut self) -> Result<i32, PackageError> {
        self.i32()
    }

    /// 读取 i64。
    fn read_i64(&mut self) -> Result<i64, PackageError> {
        Ok(i64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{build_tangent, decode_packed_normal, half_to_f32};

    /// 验证常见 half 位模式的转换。
    #[test]
    fn decodes_half_floats() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3C00), 1.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0xC000), -2.0);
        assert_eq!(half_to_f32(0x7BFF), 65_504.0);
        assert!(half_to_f32(0x7C01).is_nan());
    }

    /// 验证 `FPackedNormal` 的偏移解码：0x7F7F7F7F 异或后为全 1。
    #[test]
    fn decodes_packed_normal() {
        let normal = decode_packed_normal(0x7F7F_7F7F);
        for component in normal {
            assert!((component - 1.0).abs() < f32::EPSILON);
        }
        let negative = decode_packed_normal(0x8080_8080);
        for component in negative {
            assert!((component + 1.0).abs() < f32::EPSILON);
        }
    }

    /// 切线副切线符号跟随法线 W 分量。
    #[test]
    fn tangent_sign_follows_normal_w() {
        let positive = build_tangent([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.5]);
        assert_eq!(positive, [1.0, 0.0, 0.0, 1.0]);
        let negative = build_tangent([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, -0.5]);
        assert_eq!(negative, [1.0, 0.0, 0.0, -1.0]);
    }
}
