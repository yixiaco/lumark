//! UTexture2D 读取（无版本属性 + cooked platform data）。
//!
//! 纹理的 `PlatformData` 是类原生字段，位于属性流之后。UE5 cooked 包通常
//! 使用 `FByteBulkData` 的 bulk data map 指向内联 mip 或 `.ubulk` 载荷。
//! 本模块只提取尺寸、像素格式和 mip 原始字节，BCn 解码由上层完成。

use super::mesh::half_to_f32;
use super::{Mappings, PackageError, UnrealPackage, invalid};

/// 一个纹理 mip。
#[derive(Debug, Clone)]
pub struct TextureMip {
    /// 宽度。
    pub width: i32,
    /// 高度。
    pub height: i32,
    /// 深度/数组切片数。
    pub depth: i32,
    /// 原始压缩像素数据。
    pub data: Vec<u8>,
}

/// 一个已读取的 UTexture2D。
#[derive(Debug, Clone)]
pub struct Texture2D {
    /// 显示名称。
    pub name: String,
    /// 宽度。
    pub width: i32,
    /// 高度。
    pub height: i32,
    /// 像素格式（如 `PF_DXT1`）。
    pub pixel_format: String,
    /// mip 列表。
    pub mips: Vec<TextureMip>,
}

/// 返回不超过 `max_size` 的最深 mip 级。
///
/// 只有 1 级 mip、`max_size == 0`（不限制）或顶层尺寸已小于目标时使用 mip0；
/// 否则按每级减半估算，让解码边长接近目标值，避免先解码 4K 再缩放。
pub fn mip_level_for_limit(width: i32, height: i32, mip_count: usize, max_size: u32) -> usize {
    if max_size == 0 || mip_count <= 1 {
        return 0;
    }
    let mut dimension = i64::from(width.max(height).max(1));
    let target = i64::from(max_size.max(16));
    let mut level = 0usize;
    while dimension > target && level + 1 < mip_count {
        dimension = (dimension / 2).max(1);
        level += 1;
    }
    level
}

impl Texture2D {
    /// 解码指定 mip 为 RGBA8 像素；HDR 纹理会做简单色调映射。
    pub fn decode_mip_rgba(&self, index: usize) -> Result<Vec<u8>, PackageError> {
        Ok(self.decode_mip(index)?.into_rgba8())
    }

    /// 解码指定 mip，同时返回该 mip 的像素尺寸。
    ///
    /// 查看器按目标边长选择 mip 时使用：顶层 `width/height` 是 mip0 尺寸，
    /// 解码更小的 mip 后必须用该 mip 自己的尺寸构造查看器图像。
    pub fn decode_mip_rgba_with_size(
        &self,
        index: usize,
    ) -> Result<(Vec<u8>, u32, u32), PackageError> {
        let mip = self
            .mips
            .get(index)
            .ok_or_else(|| invalid("纹理 mip 索引越界"))?;
        let width = u32::try_from(mip.width.max(1)).unwrap_or(1);
        let height = u32::try_from(mip.height.max(1)).unwrap_or(1);
        Ok((self.decode_mip_rgba(index)?, width, height))
    }

    /// 解码指定 mip；浮点/HDR 格式返回线性 f32 RGBA 数据。
    pub fn decode_mip(&self, index: usize) -> Result<DecodedTexture, PackageError> {
        let mip = self
            .mips
            .get(index)
            .ok_or_else(|| invalid("纹理 mip 索引越界"))?;
        let width = usize::try_from(mip.width.max(1)).unwrap_or(1);
        let height = usize::try_from(mip.height.max(1)).unwrap_or(1);
        if width.saturating_mul(height) > 100_000_000 {
            return Err(invalid("纹理尺寸超过 1 亿像素安全上限"));
        }
        let count = width * height;
        let format = self.pixel_format.to_ascii_uppercase();
        match format.as_str() {
            // 浮点/HDR 格式：直接返回线性 f32，供 .hdr 导出或 HDR 预览使用。
            "PF_FLOATRGB" => return decode_half_texture(&mip.data, count, 3, width, height),
            "PF_FLOATRGBA" | "PF_RGBA16F" => {
                return decode_half_texture(&mip.data, count, 4, width, height);
            }
            "PF_R16F" => return decode_single_half_texture(&mip.data, count, width, height),
            "PF_RGBE" => return decode_rgbe_texture(&mip.data, count, width, height),
            "PF_R32_FLOAT" => return decode_single_float_texture(&mip.data, count, width, height),
            "PF_R32G32B32F" => return decode_float_texture(&mip.data, count, width, height),
            "PF_A32B32G32R32F" => {
                return decode_float_abgr_texture(&mip.data, count, width, height);
            }
            _ => {}
        }
        let mut words = vec![0u32; count];
        match format.as_str() {
            "PF_DXT1" | "PF_BC1" => {
                texture2ddecoder::decode_bc1(&mip.data, width, height, &mut words)
                    .map_err(|error| invalid(format!("BC1 解码失败：{error}")))?;
            }
            "PF_DXT3" | "PF_BC2" => {
                texture2ddecoder::decode_bc2(&mip.data, width, height, &mut words)
                    .map_err(|error| invalid(format!("BC2 解码失败：{error}")))?;
            }
            "PF_DXT5" | "PF_BC3" => {
                texture2ddecoder::decode_bc3(&mip.data, width, height, &mut words)
                    .map_err(|error| invalid(format!("BC3 解码失败：{error}")))?;
            }
            "PF_BC4" => {
                texture2ddecoder::decode_bc4(&mip.data, width, height, &mut words)
                    .map_err(|error| invalid(format!("BC4 解码失败：{error}")))?;
            }
            "PF_BC5" => {
                texture2ddecoder::decode_bc5(&mip.data, width, height, &mut words)
                    .map_err(|error| invalid(format!("BC5 解码失败：{error}")))?;
            }
            "PF_BC7" => {
                texture2ddecoder::decode_bc7(&mip.data, width, height, &mut words)
                    .map_err(|error| invalid(format!("BC7 解码失败：{error}")))?;
            }
            "PF_B8G8R8A8" => {
                if mip.data.len() < count * 4 {
                    return Err(invalid("BGRA 纹理数据长度不足"));
                }
                let mut rgba = Vec::with_capacity(count * 4);
                for pixel in mip.data[..count * 4].as_chunks::<4>().0 {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
                return Ok(DecodedTexture::ldr(width, height, rgba));
            }
            "PF_R8G8B8A8" => {
                if mip.data.len() < count * 4 {
                    return Err(invalid("RGBA 纹理数据长度不足"));
                }
                return Ok(DecodedTexture::ldr(
                    width,
                    height,
                    mip.data[..count * 4].to_vec(),
                ));
            }
            "PF_G8" | "PF_R8" => {
                if mip.data.len() < count {
                    return Err(invalid("G8 纹理数据长度不足"));
                }
                let mut rgba = Vec::with_capacity(count * 4);
                for value in &mip.data[..count] {
                    rgba.extend_from_slice(&[*value, *value, *value, 255]);
                }
                return Ok(DecodedTexture::ldr(width, height, rgba));
            }
            other => {
                return Err(PackageError::Unsupported(format!(
                    "暂不支持纹理像素格式 {other}"
                )));
            }
        }
        // texture2ddecoder 的 u32 为 [b, g, r, a] 小端存储。
        let mut rgba = Vec::with_capacity(words.len() * 4);
        for word in words {
            let [b, g, r, a] = word.to_le_bytes();
            rgba.extend_from_slice(&[r, g, b, a]);
        }
        Ok(DecodedTexture::ldr(width, height, rgba))
    }
}

/// 解码后的纹素数据。
#[derive(Debug, Clone)]
pub enum TexturePixels {
    /// 8 位 RGBA 数据。
    Ldr(Vec<u8>),
    /// 线性 RGBA f32 数据（HDR）。
    Hdr(Vec<f32>),
}

/// 解码后的纹理 mip。
#[derive(Debug, Clone)]
pub struct DecodedTexture {
    /// 宽度（像素）。
    pub width: u32,
    /// 高度（像素）。
    pub height: u32,
    /// 纹素数据。
    pub pixels: TexturePixels,
}

impl DecodedTexture {
    /// 创建 8 位纹理。
    fn ldr(width: usize, height: usize, rgba: Vec<u8>) -> Self {
        Self {
            width: width as u32,
            height: height as u32,
            pixels: TexturePixels::Ldr(rgba),
        }
    }

    /// 创建 HDR 纹理。
    fn hdr(width: usize, height: usize, rgba: Vec<f32>) -> Self {
        Self {
            width: width as u32,
            height: height as u32,
            pixels: TexturePixels::Hdr(rgba),
        }
    }

    /// 是否为 HDR 数据。
    pub fn is_hdr(&self) -> bool {
        matches!(self.pixels, TexturePixels::Hdr(_))
    }

    /// 转换为 RGBA8；HDR 使用 Reinhard 映射 + gamma 2.2 近似显示。
    pub fn into_rgba8(self) -> Vec<u8> {
        match self.pixels {
            TexturePixels::Ldr(bytes) => bytes,
            TexturePixels::Hdr(values) => {
                let mut rgba = Vec::with_capacity(values.len());
                for pixel in values.as_chunks::<4>().0 {
                    let map = |channel: f32| {
                        let positive = channel.max(0.0);
                        let mapped = (positive / (1.0 + positive)).powf(1.0 / 2.2);
                        (mapped.clamp(0.0, 1.0) * 255.0).round() as u8
                    };
                    rgba.extend_from_slice(&[
                        map(pixel[0]),
                        map(pixel[1]),
                        map(pixel[2]),
                        (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8,
                    ]);
                }
                rgba
            }
        }
    }
}

/// 读取每像素 `channels` 个 half 分量的浮点纹理。
fn decode_half_texture(
    data: &[u8],
    count: usize,
    channels: usize,
    width: usize,
    height: usize,
) -> Result<DecodedTexture, PackageError> {
    let stride = channels * 2;
    let needed = count
        .checked_mul(stride)
        .ok_or_else(|| invalid("浮点纹理长度溢出"))?;
    if data.len() < needed {
        return Err(invalid("浮点纹理数据长度不足"));
    }
    let mut values = Vec::with_capacity(count * 4);
    for pixel in data[..needed].chunks_exact(stride) {
        let mut rgba = [0.0f32, 0.0, 0.0, 1.0];
        for (channel, target) in rgba.iter_mut().take(channels).enumerate() {
            let offset = channel * 2;
            *target = half_to_f32(u16::from_le_bytes([pixel[offset], pixel[offset + 1]]));
        }
        values.extend_from_slice(&rgba);
    }
    Ok(DecodedTexture::hdr(width, height, values))
}

/// 读取单通道 half 浮点纹理，灰度复制到 RGB。
fn decode_single_half_texture(
    data: &[u8],
    count: usize,
    width: usize,
    height: usize,
) -> Result<DecodedTexture, PackageError> {
    if data.len() < count * 2 {
        return Err(invalid("R16F 纹理数据长度不足"));
    }
    let mut values = Vec::with_capacity(count * 4);
    for pixel in data[..count * 2].as_chunks::<2>().0 {
        let value = half_to_f32(u16::from_le_bytes([pixel[0], pixel[1]]));
        values.extend_from_slice(&[value, value, value, 1.0]);
    }
    Ok(DecodedTexture::hdr(width, height, values))
}

/// 读取 RGBe（共享指数）HDR 纹理。
fn decode_rgbe_texture(
    data: &[u8],
    count: usize,
    width: usize,
    height: usize,
) -> Result<DecodedTexture, PackageError> {
    if data.len() < count * 4 {
        return Err(invalid("RGBE 纹理数据长度不足"));
    }
    let mut values = Vec::with_capacity(count * 4);
    for pixel in data[..count * 4].as_chunks::<4>().0 {
        let exponent = pixel[3] as i32;
        if exponent == 0 {
            values.extend_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            continue;
        }
        // RGBE 共享指数：mantissa × 2^(E-136)。
        let scale = 2.0f32.powi(exponent - 136);
        values.extend_from_slice(&[
            pixel[0] as f32 * scale,
            pixel[1] as f32 * scale,
            pixel[2] as f32 * scale,
            1.0,
        ]);
    }
    Ok(DecodedTexture::hdr(width, height, values))
}

/// 读取单通道 f32 纹理，灰度复制到 RGB。
fn decode_single_float_texture(
    data: &[u8],
    count: usize,
    width: usize,
    height: usize,
) -> Result<DecodedTexture, PackageError> {
    if data.len() < count * 4 {
        return Err(invalid("R32F 纹理数据长度不足"));
    }
    let mut values = Vec::with_capacity(count * 4);
    for pixel in data[..count * 4].as_chunks::<4>().0 {
        let value = f32::from_le_bytes(*pixel);
        values.extend_from_slice(&[value, value, value, 1.0]);
    }
    Ok(DecodedTexture::hdr(width, height, values))
}

/// 读取每像素 3 个 f32 分量的浮点纹理。
fn decode_float_texture(
    data: &[u8],
    count: usize,
    width: usize,
    height: usize,
) -> Result<DecodedTexture, PackageError> {
    let needed = count
        .checked_mul(12)
        .ok_or_else(|| invalid("浮点纹理长度溢出"))?;
    if data.len() < needed {
        return Err(invalid("RGB32F 纹理数据长度不足"));
    }
    let mut values = Vec::with_capacity(count * 4);
    for pixel in data[..needed].as_chunks::<12>().0 {
        values.extend_from_slice(&[
            f32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]),
            f32::from_le_bytes([pixel[4], pixel[5], pixel[6], pixel[7]]),
            f32::from_le_bytes([pixel[8], pixel[9], pixel[10], pixel[11]]),
            1.0,
        ]);
    }
    Ok(DecodedTexture::hdr(width, height, values))
}

/// 读取 `PF_A32B32G32R32F`（内存顺序 A/B/G/R）并转为 RGBA。
fn decode_float_abgr_texture(
    data: &[u8],
    count: usize,
    width: usize,
    height: usize,
) -> Result<DecodedTexture, PackageError> {
    let needed = count
        .checked_mul(16)
        .ok_or_else(|| invalid("浮点纹理长度溢出"))?;
    if data.len() < needed {
        return Err(invalid("ABGR32F 纹理数据长度不足"));
    }
    let mut values = Vec::with_capacity(count * 4);
    for pixel in data[..needed].as_chunks::<16>().0 {
        values.extend_from_slice(&[
            f32::from_le_bytes([pixel[12], pixel[13], pixel[14], pixel[15]]),
            f32::from_le_bytes([pixel[8], pixel[9], pixel[10], pixel[11]]),
            f32::from_le_bytes([pixel[4], pixel[5], pixel[6], pixel[7]]),
            f32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]),
        ]);
    }
    Ok(DecodedTexture::hdr(width, height, values))
}

/// 从包中读取指定导出的 UTexture2D。
pub fn read_texture2d(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
    ubulk: Option<&[u8]>,
) -> Result<Texture2D, PackageError> {
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or_else(|| invalid("纹理导出索引越界"))?;
    if export.kind != super::AssetKind::Texture {
        return Err(PackageError::Unsupported(format!(
            "{} 不是 Texture2D",
            export.name
        )));
    }
    let properties = package.read_export_properties(export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("纹理导出数据越界"))?;
    let mut reader = DataReader {
        data,
        position: properties.bytes_read,
    };

    // 找到像素格式 FName；它后面是 skip 偏移和 FTexturePlatformData。
    let Some((format, platform_start, skip_offset)) =
        find_platform(&mut reader, package.names(), data.len())
    else {
        return Err(PackageError::Unsupported(
            "找不到 FTexturePlatformData（可能不是 cooked 2D 纹理）".to_string(),
        ));
    };
    reader.position = platform_start;
    let platform = read_platform(&mut reader, package, ubulk)?;
    let _ = skip_offset;
    Ok(Texture2D {
        name: export.name.clone(),
        width: platform.width,
        height: platform.height,
        pixel_format: format,
        mips: platform.mips,
    })
}

/// 平台数据解析结果。
struct Platform {
    width: i32,
    height: i32,
    mips: Vec<TextureMip>,
}

/// 查找像素格式 FName、平台数据起点和后续格式偏移。
fn find_platform(
    reader: &mut DataReader<'_>,
    names: &[String],
    data_len: usize,
) -> Option<(String, usize, usize)> {
    let start = reader.position;
    let end = (start + 512).min(data_len.saturating_sub(16));
    for offset in start..end {
        reader.position = offset;
        let Ok(index) = reader.u32() else { continue };
        let Ok(number) = reader.u32() else { continue };
        if number != 0 || index & 0xC000_0000 != 0 {
            continue;
        }
        let Some(name) = names.get((index & 0x3FFF_FFFF) as usize) else {
            continue;
        };
        if !name.starts_with("PF_") {
            continue;
        }
        let platform_start = offset + 16;
        if platform_start + 16 > data_len {
            continue;
        }
        let Ok(skip) = reader.i64_at(offset + 8) else {
            continue;
        };
        if skip < 0 {
            continue;
        }
        if !validate_platform(data_len, reader.data, platform_start) {
            continue;
        }
        return Some((name.clone(), platform_start, platform_start + skip as usize));
    }
    None
}

/// 校验平台数据头部是否合理。
fn validate_platform(data_len: usize, data: &[u8], start: usize) -> bool {
    if start + 16 + 12 > data_len {
        return false;
    }
    let mut reader = DataReader {
        data,
        position: start,
    };
    let Ok(flag) = reader.u8() else { return false };
    if flag > 1 {
        return false;
    }
    reader.position = start + 16;
    let Ok(width) = reader.i32() else {
        return false;
    };
    let Ok(height) = reader.i32() else {
        return false;
    };
    let Ok(packed) = reader.u32() else {
        return false;
    };
    if !(1..=32768).contains(&width) || !(1..=32768).contains(&height) {
        return false;
    }
    let slices = packed & 0x3FFF_FFFF;
    if slices == 0 || slices > 4096 {
        return false;
    }
    let Ok(format_length) = reader.i32() else {
        return false;
    };
    if !(1..=64).contains(&format_length) {
        return false;
    }
    let Ok(format) = reader.bytes(format_length as usize) else {
        return false;
    };
    format
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == 0)
}

/// 读取平台数据和 mip。
fn read_platform(
    reader: &mut DataReader<'_>,
    package: &UnrealPackage,
    ubulk: Option<&[u8]>,
) -> Result<Platform, PackageError> {
    let using_derived = reader.u8()?;
    if using_derived != 0 {
        return Err(PackageError::Unsupported(
            "纹理使用 derived data，当前不支持".to_string(),
        ));
    }
    reader.skip(15)?;
    let width = reader.i32()?;
    let height = reader.i32()?;
    let packed = reader.u32()?;
    let _pixel_format = reader.fstring()?;
    if packed & (1 << 30) != 0 {
        let _ext_data = reader.u32()?;
        let _num_mips_in_tail = reader.u32()?;
    }
    if packed & (1 << 29) != 0 {
        return Err(PackageError::Unsupported(
            "纹理含 CPU copy（FSharedImage），当前不支持".to_string(),
        ));
    }
    let _first_mip = reader.i32()?;
    let mip_count = reader.i32()?;
    if !(0..=64).contains(&mip_count) {
        return Err(invalid(format!("纹理 mip 数量异常：{mip_count}")));
    }
    let mut mips = Vec::with_capacity(mip_count as usize);
    for _ in 0..mip_count {
        let (data, inline_size) = read_bulk(reader, package, ubulk)?;
        let mip_width = reader.i32()?;
        let mip_height = reader.i32()?;
        let mip_depth = reader.i32()?;
        let expected = data
            .as_ref()
            .map(|bytes| bytes.len())
            .unwrap_or(inline_size);
        if data.is_none() && expected > 0 {
            return Err(PackageError::Unsupported(
                "纹理 mip 数据位于 .ubulk，但当前没有提供侧载数据".to_string(),
            ));
        }
        mips.push(TextureMip {
            width: mip_width,
            height: mip_height,
            depth: mip_depth,
            data: data.unwrap_or_default(),
        });
    }
    Ok(Platform {
        width,
        height,
        mips,
    })
}

/// 读取一个 FByteBulkData 头并返回数据。
///
/// 返回 `(data, inline_size)`；`data` 为 None 表示需要 `.ubulk` 但尚未提供。
fn read_bulk(
    reader: &mut DataReader<'_>,
    package: &UnrealPackage,
    ubulk: Option<&[u8]>,
) -> Result<(Option<Vec<u8>>, usize), PackageError> {
    let bulk_data = package.info().bulk_data.as_slice();
    if !bulk_data.is_empty() {
        let index = reader.i32()?;
        let entry = bulk_data
            .get(index as usize)
            .ok_or_else(|| invalid(format!("纹理 bulk data 索引越界：{index}")))?;
        let size = entry.serial_size.max(0) as usize;
        let force_inline = entry.flags & (1 << 6) != 0;
        if force_inline {
            let bytes = reader.bytes(size)?.to_vec();
            return Ok((Some(bytes), size));
        }
        if let Some(ubulk) = ubulk {
            let start = entry.serial_offset.max(0) as usize;
            let end = start
                .checked_add(size)
                .ok_or_else(|| invalid("纹理 bulk data 长度溢出"))?;
            let bytes = ubulk
                .get(start..end)
                .ok_or_else(|| invalid("纹理 bulk data 超出 .ubulk 范围"))?
                .to_vec();
            return Ok((Some(bytes), size));
        }
        return Ok((None, size));
    }
    let flags = reader.u32()?;
    let element_count = reader.i32()?;
    let size_on_disk = match reader.u32()? as usize {
        size if size > 0 => size,
        _ => element_count.max(0) as usize,
    };
    let _offset = reader.i64()?;
    if flags & (1 << 6) != 0 {
        let bytes = reader.bytes(size_on_disk)?.to_vec();
        return Ok((Some(bytes), size_on_disk));
    }
    Ok((None, size_on_disk))
}

/// 导出数据读取器。
struct DataReader<'a> {
    data: &'a [u8],
    position: usize,
}
impl<'a> DataReader<'a> {
    /// 读取指定长度。
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| invalid("纹理数据长度溢出"))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| invalid("纹理数据被截断"))?;
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

    /// 读取 i64。
    fn i64(&mut self) -> Result<i64, PackageError> {
        Ok(i64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 从绝对偏移读取 i64，不改变当前位置。
    fn i64_at(&mut self, offset: usize) -> Result<i64, PackageError> {
        let bytes = self
            .data
            .get(offset..offset + 8)
            .ok_or_else(|| invalid("纹理偏移越界"))?;
        Ok(i64::from_le_bytes(bytes.try_into().unwrap_or_default()))
    }

    /// 读取 FString。
    fn fstring(&mut self) -> Result<String, PackageError> {
        let length = self.i32()?;
        if length <= 0 {
            return Ok(String::new());
        }
        let bytes = self.bytes(length as usize)?;
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按目标边长选择最接近的 mip，避免全尺寸解码。
    #[test]
    fn selects_mip_level_for_texture_limit() {
        assert_eq!(mip_level_for_limit(4096, 4096, 13, 512), 3);
        assert_eq!(mip_level_for_limit(2048, 2048, 12, 512), 2);
        assert_eq!(mip_level_for_limit(1024, 512, 11, 512), 1);
        assert_eq!(mip_level_for_limit(512, 512, 10, 512), 0);
        assert_eq!(mip_level_for_limit(4096, 2048, 13, 1024), 2);
        // 单 mip / 不限制 / mip 数不足时回退 mip0。
        assert_eq!(mip_level_for_limit(4096, 4096, 1, 512), 0);
        assert_eq!(mip_level_for_limit(4096, 4096, 13, 0), 0);
        assert_eq!(mip_level_for_limit(4096, 4096, 2, 512), 1);
    }

    /// 解码一个由纯色块构成的 BC1 mip。
    #[test]
    fn decodes_bc1_mip_to_rgba() {
        let texture = Texture2D {
            name: "T_Test".to_string(),
            width: 4,
            height: 4,
            pixel_format: "PF_DXT1".to_string(),
            mips: vec![TextureMip {
                width: 4,
                height: 4,
                depth: 1,
                // 两个 5:6:5 颜色端点相同，4 个索引全 0。
                data: vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00],
            }],
        };
        let rgba = texture.decode_mip_rgba(0).unwrap();
        assert_eq!(rgba.len(), 4 * 4 * 4);
        assert_eq!(rgba[3], 255);
        assert_eq!(rgba[..3], [255, 255, 255]);
    }

    /// 不支持的像素格式返回可读错误。
    #[test]
    fn rejects_unknown_pixel_format() {
        let texture = Texture2D {
            name: "T_Test".to_string(),
            width: 1,
            height: 1,
            pixel_format: "PF_Unknown".to_string(),
            mips: vec![TextureMip {
                width: 1,
                height: 1,
                depth: 1,
                data: vec![0; 16],
            }],
        };
        assert!(texture.decode_mip_rgba(0).is_err());
    }

    /// 构造单像素纹理，便于验证浮点格式解码。
    fn pixel_texture(format: &str, data: Vec<u8>) -> Texture2D {
        Texture2D {
            name: "T_Hdr".to_string(),
            width: 1,
            height: 1,
            pixel_format: format.to_string(),
            mips: vec![TextureMip {
                width: 1,
                height: 1,
                depth: 1,
                data,
            }],
        }
    }

    /// 把 f32 写成 little-endian 字节。
    fn f32_bytes(value: f32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// `PF_FloatRGBA` 解码为线性 RGBA f32。
    #[test]
    fn decodes_float_rgba_hdr() {
        let mut data = Vec::new();
        for value in [1.0f32, 2.0, 0.5, 1.0] {
            data.extend_from_slice(&half_bits(value).to_le_bytes());
        }
        let texture = pixel_texture("PF_FloatRGBA", data);
        let decoded = texture.decode_mip(0).expect("HDR 解码");
        assert!(decoded.is_hdr());
        match decoded.pixels {
            TexturePixels::Hdr(values) => {
                assert_eq!(values.len(), 4);
                assert!((values[0] - 1.0).abs() < 1e-3);
                assert!((values[1] - 2.0).abs() < 1e-2);
                assert!((values[2] - 0.5).abs() < 1e-3);
                assert!((values[3] - 1.0).abs() < 1e-3);
            }
            TexturePixels::Ldr(_) => panic!("应返回 HDR 数据"),
        }
    }

    /// `PF_R32_FLOAT` 与 `PF_R32G32B32F` 解码。
    #[test]
    fn decodes_float32_formats() {
        let single = pixel_texture("PF_R32_FLOAT", f32_bytes(2.5).to_vec());
        match single.decode_mip(0).expect("R32F").pixels {
            TexturePixels::Hdr(values) => {
                assert_eq!(values, vec![2.5, 2.5, 2.5, 1.0]);
            }
            TexturePixels::Ldr(_) => panic!("应返回 HDR 数据"),
        }
        let mut rgb = Vec::new();
        for value in [0.1f32, 0.2, 0.3] {
            rgb.extend_from_slice(&f32_bytes(value));
        }
        let decoded = pixel_texture("PF_R32G32B32F", rgb)
            .decode_mip(0)
            .expect("RGB32F");
        match decoded.pixels {
            TexturePixels::Hdr(values) => {
                assert_eq!(values, vec![0.1, 0.2, 0.3, 1.0]);
            }
            TexturePixels::Ldr(_) => panic!("应返回 HDR 数据"),
        }
    }

    /// 把 f32 转换为 IEEE half 位模式（测试用，输入落在常见可表示范围内）。
    fn half_bits(value: f32) -> u16 {
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
        let mantissa = (bits & 0x007F_FFFF) >> 13;
        if exponent <= 0 {
            sign
        } else if exponent >= 31 {
            sign | 0x7C00
        } else {
            sign | ((exponent as u16) << 10) | mantissa as u16
        }
    }
}
