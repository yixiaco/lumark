//! UTexture2D 读取（无版本属性 + cooked platform data）。
//!
//! 纹理的 `PlatformData` 是类原生字段，位于属性流之后。UE5 cooked 包通常
//! 使用 `FByteBulkData` 的 bulk data map 指向内联 mip 或 `.ubulk` 载荷。
//! 本模块只提取尺寸、像素格式和 mip 原始字节，BCn 解码由上层完成。

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

impl Texture2D {
    /// 解码指定 mip 为 RGBA8 像素。
    pub fn decode_mip_rgba(&self, index: usize) -> Result<Vec<u8>, PackageError> {
        let mip = self
            .mips
            .get(index)
            .ok_or_else(|| invalid("纹理 mip 索引越界"))?;
        let width = usize::try_from(mip.width.max(1)).unwrap_or(1);
        let height = usize::try_from(mip.height.max(1)).unwrap_or(1);
        if width.saturating_mul(height) > 100_000_000 {
            return Err(invalid("纹理尺寸超过 1 亿像素安全上限"));
        }
        let format = self.pixel_format.to_ascii_uppercase();
        let mut words = vec![0u32; width * height];
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
                if mip.data.len() < width * height * 4 {
                    return Err(invalid("BGRA 纹理数据长度不足"));
                }
                let mut rgba = Vec::with_capacity(width * height * 4);
                for pixel in mip.data[..width * height * 4].as_chunks::<4>().0 {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
                return Ok(rgba);
            }
            "PF_R8G8B8A8" => {
                if mip.data.len() < width * height * 4 {
                    return Err(invalid("RGBA 纹理数据长度不足"));
                }
                return Ok(mip.data[..width * height * 4].to_vec());
            }
            "PF_G8" | "PF_R8" => {
                if mip.data.len() < width * height {
                    return Err(invalid("G8 纹理数据长度不足"));
                }
                let mut rgba = Vec::with_capacity(width * height * 4);
                for value in &mip.data[..width * height] {
                    rgba.extend_from_slice(&[*value, *value, *value, 255]);
                }
                return Ok(rgba);
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
        Ok(rgba)
    }
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
}
