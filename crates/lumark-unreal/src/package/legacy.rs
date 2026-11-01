//! UE4 旧式 `uasset` 摘要读取。
//!
//! FF7 Remake 等 UE4.27 游戏的 cooked 包使用无版本属性；完整资产预览需要
//! `.usmap`，但摘要、名称表、导入/导出表不需要映射，可先展示包级元数据。

use super::texture::{Texture2D, TextureMip};
use super::unversioned;
use super::{Mappings, PackageError};

/// 旧式包摘要（只包含详情面板可展示的字段）。
#[derive(Debug, Clone)]
pub struct LegacyPackageSummary {
    /// 包名。
    pub package_name: String,
    /// 主导出类名。
    pub class_name: String,
    /// 主导出对象名。
    pub export_name: String,
    /// 导出数量。
    pub export_count: usize,
    /// 导入数量。
    pub import_count: usize,
    /// 名称表数量。
    pub name_count: usize,
    /// 总头大小。
    pub total_header_size: i32,
}

/// 判断数据是否是 UE4 旧式 uasset（PackageFileTag）。
pub fn is_legacy_package(data: &[u8]) -> bool {
    data.get(..4).is_some_and(|bytes| {
        u32::from_le_bytes(bytes.try_into().unwrap_or_default()) == 0x9E2A_83C1
    })
}

/// 解析旧式包摘要；当前只支持 FF7 Remake 这类 unversioned 包。
pub fn parse_legacy_summary(data: &[u8]) -> Result<LegacyPackageSummary, PackageError> {
    let mut reader = Reader { data, pos: 0 };
    let magic = reader.u32()?;
    if magic != 0x9E2A_83C1 {
        return Err(invalid("不是 UE4 旧式包"));
    }
    let legacy = reader.i32()?;
    if legacy >= 0 {
        return Err(PackageError::Unsupported(
            "旧式包使用非负文件版本，暂不支持".to_string(),
        ));
    }
    let ue3 = reader.i32()?;
    let ue4 = reader.i32()?;
    let licensee = reader.i32()?;
    if ue3 != 0 || ue4 != 0 || licensee != 0 {
        return Err(PackageError::Unsupported(
            "旧式包带有显式引擎版本，暂不支持".to_string(),
        ));
    }
    let custom_count = reader.i32()?;
    if !(0..=1024).contains(&custom_count) {
        return Err(invalid("旧式包自定义版本数量非法"));
    }
    // Optimized 格式：每个自定义版本为 16 字节 GUID + i32。
    let _ = reader.bytes(custom_count as usize * 20)?;
    let total_header_size = reader.i32()?;
    let package_name = reader.fstring()?;
    let _flags = reader.u32()?;
    let name_count = reader.count(8, 1_000_000)?;
    let name_offset = reader.offset(data.len())?;
    let _gatherable_count = reader.i32()?;
    let _gatherable_offset = reader.i32()?;
    let export_count = reader.count(8, 1_000_000)?;
    let export_offset = reader.offset(data.len())?;
    let import_count = reader.count(8, 1_000_000)?;
    let import_offset = reader.offset(data.len())?;

    let mut names = Vec::with_capacity(name_count);
    reader.pos = name_offset;
    for _ in 0..name_count {
        names.push(reader.fstring()?);
        let _hash = reader.u32()?;
    }
    reader.pos = import_offset;
    let mut imports = Vec::with_capacity(import_count);
    for _ in 0..import_count {
        let class_package = reader.fname(&names)?;
        let class_name = reader.fname(&names)?;
        let _outer = reader.i32()?;
        let object_name = reader.fname(&names)?;
        imports.push((class_package, class_name, object_name));
    }
    reader.pos = export_offset;
    let mut class_name = String::new();
    let mut export_name = String::new();
    for index in 0..export_count {
        let class_index = reader.i32()?;
        let _super_index = reader.i32()?;
        let _template_index = reader.i32()?;
        let _outer = reader.i32()?;
        let object_name = reader.fname(&names)?;
        let _object_flags = reader.u32()?;
        let _serial_size = reader.i64()?;
        let _serial_offset = reader.i64()?;
        let _forced = reader.i32()?;
        let _not_client = reader.i32()?;
        let _not_server = reader.i32()?;
        let _package_guid = reader.bytes(16)?;
        let _package_flags = reader.u32()?;
        let _not_always = reader.i32()?;
        let _is_asset = reader.i32()?;
        let _dependencies = [
            reader.i32()?,
            reader.i32()?,
            reader.i32()?,
            reader.i32()?,
            reader.i32()?,
        ];
        if index == 0 || class_name.is_empty() {
            class_name = resolve_class(class_index, &imports);
        }
        if export_name.is_empty() {
            export_name = object_name;
        }
    }
    if class_name.is_empty() {
        class_name = "Unknown".to_string();
    }
    Ok(LegacyPackageSummary {
        package_name,
        class_name,
        export_name,
        export_count,
        import_count,
        name_count,
        total_header_size,
    })
}

/// 把负的 FPackageIndex 解析为导入对象的名称（即资产的类名）。
fn resolve_class(class_index: i32, imports: &[(String, String, String)]) -> String {
    if class_index < 0 {
        let index = (-class_index - 1) as usize;
        imports
            .get(index)
            .map(|(_, _, object_name)| object_name.clone())
            .unwrap_or_else(|| format!("Import({index})"))
    } else if class_index == 0 {
        "Class".to_string()
    } else {
        format!("Export({class_index})")
    }
}

/// 读取器。
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .pos
            .checked_add(length)
            .ok_or_else(|| invalid("旧式包长度溢出"))?;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| invalid(format!("旧式包数据被截断 pos={} len={length}", self.pos)))?;
        self.pos = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, PackageError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn i32(&mut self) -> Result<i32, PackageError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn i64(&mut self) -> Result<i64, PackageError> {
        Ok(i64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    fn count(&mut self, min_size: usize, limit: usize) -> Result<usize, PackageError> {
        let count = self.i32()?;
        if count < 0 || count as usize > limit || count as usize * min_size > self.data.len() {
            return Err(invalid("旧式包数组数量非法"));
        }
        Ok(count as usize)
    }

    fn offset(&mut self, limit: usize) -> Result<usize, PackageError> {
        let offset = self.i32()?;
        if offset < 0 || offset as usize > limit {
            return Err(invalid("旧式包偏移越界"));
        }
        Ok(offset as usize)
    }

    fn fstring(&mut self) -> Result<String, PackageError> {
        let length = self.i32()?;
        if length == 0 {
            return Ok(String::new());
        }
        let wide = length < 0;
        let cells = length.unsigned_abs() as usize;
        let bytes = self.bytes(cells * if wide { 2 } else { 1 })?;
        if wide {
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            Ok(String::from_utf16_lossy(&units))
        } else {
            Ok(String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .to_string())
        }
    }

    fn fname(&mut self, names: &[String]) -> Result<String, PackageError> {
        let index = self.i32()?;
        let number = self.i32()?;
        if index < 0 {
            return Ok(format!("Name({index})"));
        }
        let base = names
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("Name({index})"));
        if number == 0 {
            Ok(base)
        } else {
            Ok(format!("{base}_{}", number - 1))
        }
    }
}

/// 构造带上下文的非法数据错误。
fn invalid(message: impl Into<String>) -> PackageError {
    PackageError::Invalid(message.into())
}

/// 一个旧式包导出。
#[derive(Debug, Clone)]
pub struct LegacyExport {
    /// 对象名。
    pub name: String,
    /// 类名。
    pub class_name: String,
    /// 在 `.uasset` 中的序列化偏移。
    pub serial_offset: usize,
    /// 序列化字节数。
    pub serial_size: usize,
}

/// 一个已解析的旧式包。
#[derive(Debug, Clone)]
pub struct LegacyPackage {
    /// 包摘要。
    pub summary: LegacyPackageSummary,
    /// 名称表。
    names: Vec<String>,
    /// 导入表。
    imports: Vec<(String, String, String)>,
    /// 导出表。
    exports: Vec<LegacyExport>,
    /// 包内自定义版本（GUID + 版本号）。
    custom_versions: Vec<([u8; 16], i32)>,
    /// `.uexp` 数据（导出数据起点为 0）。
    uexp: Vec<u8>,
}

impl LegacyPackage {
    /// 解析旧式 `uasset`/`uexp`。
    pub fn parse(asset: &[u8], uexp: &[u8]) -> Result<Self, PackageError> {
        let mut reader = Reader {
            data: asset,
            pos: 0,
        };
        let magic = reader.u32()?;
        if magic != 0x9E2A_83C1 {
            return Err(invalid("不是 UE4 旧式包"));
        }
        let legacy = reader.i32()?;
        if legacy >= 0 {
            return Err(PackageError::Unsupported(
                "旧式包使用非负文件版本，暂不支持".to_string(),
            ));
        }
        let ue3 = reader.i32()?;
        let ue4 = reader.i32()?;
        let licensee = reader.i32()?;
        if ue3 != 0 || ue4 != 0 || licensee != 0 {
            return Err(PackageError::Unsupported(
                "旧式包带有显式引擎版本，暂不支持".to_string(),
            ));
        }
        let custom_count = reader.i32()?;
        if !(0..=1024).contains(&custom_count) {
            return Err(invalid("旧式包自定义版本数量非法"));
        }
        let mut custom_versions = Vec::with_capacity(custom_count as usize);
        for _ in 0..custom_count {
            let mut guid = [0u8; 16];
            guid.copy_from_slice(reader.bytes(16)?);
            custom_versions.push((guid, reader.i32()?));
        }
        let total_header_size = reader.i32()?;
        let package_name = reader.fstring()?;
        let _flags = reader.u32()?;
        let name_count = reader.count(8, 1_000_000)?;
        let name_offset = reader.offset(asset.len())?;
        let _gatherable_count = reader.i32()?;
        let _gatherable_offset = reader.i32()?;
        let export_count = reader.count(8, 1_000_000)?;
        let export_offset = reader.offset(asset.len())?;
        let import_count = reader.count(8, 1_000_000)?;
        let import_offset = reader.offset(asset.len())?;

        let mut names = Vec::with_capacity(name_count);
        reader.pos = name_offset;
        for _ in 0..name_count {
            names.push(reader.fstring()?);
            let _hash = reader.u32()?;
        }
        let mut imports = Vec::with_capacity(import_count);
        reader.pos = import_offset;
        for _ in 0..import_count {
            let class_package = reader.fname(&names)?;
            let class_name = reader.fname(&names)?;
            let _outer = reader.i32()?;
            let object_name = reader.fname(&names)?;
            imports.push((class_package, class_name, object_name));
        }
        let mut exports = Vec::with_capacity(export_count);
        reader.pos = export_offset;
        for _ in 0..export_count {
            let class_index = reader.i32()?;
            let _super_index = reader.i32()?;
            let _template_index = reader.i32()?;
            let _outer = reader.i32()?;
            let object_name = reader.fname(&names)?;
            let _object_flags = reader.u32()?;
            let serial_size = reader.i64()?;
            let serial_offset = reader.i64()?;
            let _forced = reader.i32()?;
            let _not_client = reader.i32()?;
            let _not_server = reader.i32()?;
            let _package_guid = reader.bytes(16)?;
            let _package_flags = reader.u32()?;
            let _not_always = reader.i32()?;
            let _is_asset = reader.i32()?;
            let _dependencies = [
                reader.i32()?,
                reader.i32()?,
                reader.i32()?,
                reader.i32()?,
                reader.i32()?,
            ];
            let serial_size = usize::try_from(serial_size.max(0)).unwrap_or(0);
            let serial_offset = usize::try_from(serial_offset.max(0)).unwrap_or(0);
            exports.push(LegacyExport {
                name: object_name,
                class_name: resolve_class(class_index, &imports),
                serial_offset,
                serial_size,
            });
        }
        let summary = LegacyPackageSummary {
            package_name,
            class_name: exports
                .first()
                .map(|export| export.class_name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            export_name: exports
                .first()
                .map(|export| export.name.clone())
                .unwrap_or_default(),
            export_count,
            import_count,
            name_count,
            total_header_size,
        };
        Ok(Self {
            summary,
            names,
            imports,
            exports,
            custom_versions,
            uexp: uexp.to_vec(),
        })
    }

    /// 查询指定 GUID 的自定义版本号。
    pub fn custom_version(&self, guid: [u8; 16]) -> Option<i32> {
        self.custom_versions
            .iter()
            .find_map(|(candidate, version)| (*candidate == guid).then_some(*version))
    }

    /// 全部自定义版本。
    pub fn custom_versions(&self) -> &[([u8; 16], i32)] {
        &self.custom_versions
    }

    /// 名称表。
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// 导入表。
    pub fn imports(&self) -> &[(String, String, String)] {
        &self.imports
    }

    /// 把 UE 对象索引解析成导入/导出名称。
    ///
    /// UE 的 `FPackageIndex`：正数为导出下标 + 1，负数为导入下标取反 - 1，
    /// 0 表示 `None`。
    pub fn resolve_object_name(&self, index: i32) -> Option<String> {
        if index > 0 {
            self.exports
                .get((index - 1) as usize)
                .map(|export| export.name.clone())
        } else if index < 0 {
            self.imports
                .get((-index - 1) as usize)
                .map(|(_, _, object_name)| object_name.clone())
        } else {
            None
        }
    }

    /// 导出表。
    pub fn exports(&self) -> &[LegacyExport] {
        &self.exports
    }

    /// 返回指定导出的 `.uexp` 数据。
    pub fn export_data(&self, index: usize) -> Option<&[u8]> {
        let export = self.exports.get(index)?;
        let header = usize::try_from(self.summary.total_header_size.max(0)).unwrap_or(0);
        let start = export.serial_offset.checked_sub(header)?;
        let end = start.checked_add(export.serial_size)?;
        self.uexp.get(start..end)
    }

    /// 读取指定导出的无版本属性；需要 `.usmap`。
    pub fn read_export_properties(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<unversioned::UnversionedProperties, PackageError> {
        let export = self
            .exports
            .get(index)
            .ok_or_else(|| invalid("旧式导出索引越界"))?;
        let data = self
            .export_data(index)
            .ok_or_else(|| invalid("旧式导出数据越界"))?;
        unversioned::read_properties_legacy(data, &self.names, mappings, &export.class_name)
    }

    /// 从指定偏移尝试读取属性（仅诊断用）。
    pub fn read_export_properties_at(
        &self,
        index: usize,
        mappings: &Mappings,
        skip: usize,
    ) -> Result<unversioned::UnversionedProperties, PackageError> {
        let export = self
            .exports
            .get(index)
            .ok_or_else(|| invalid("旧式导出索引越界"))?;
        let data = self
            .export_data(index)
            .and_then(|data| data.get(skip..))
            .ok_or_else(|| invalid("旧式导出偏移越界"))?;
        unversioned::read_properties_legacy(data, &self.names, mappings, &export.class_name)
    }

    /// 从导出数据的内嵌偏移读取一个无版本结构属性。
    ///
    /// 压缩动画的曲线数据和部分运行时结构直接写在导出字节流里，没有独立的
    /// 导出表；结构名来自游戏的 `.usmap`。
    pub fn read_embedded_struct_properties(
        &self,
        index: usize,
        skip: usize,
        mappings: &Mappings,
        struct_name: &str,
    ) -> Result<unversioned::UnversionedProperties, PackageError> {
        let data = self
            .export_data(index)
            .and_then(|data| data.get(skip..))
            .ok_or_else(|| invalid("旧式导出偏移越界"))?;
        unversioned::read_struct_properties(data, &self.names, mappings, struct_name)
    }
}
/// 旧式属性头统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyPropertyHeader {
    /// 属性值数量。
    pub property_values: usize,
    /// 属性头字节数。
    pub header_bytes: usize,
    /// 是否带零值位图。
    pub has_zero_mask: bool,
}

/// 读取 `.uexp` 开头的 unversioned 属性头。
pub fn read_legacy_property_header(data: &[u8]) -> Option<LegacyPropertyHeader> {
    let mut pos = 0usize;
    let mut property_values = 0usize;
    let mut zero_mask_bits = 0usize;
    let mut has_zero_mask = false;
    loop {
        let packed = u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?);
        pos += 2;
        let zeros = packed & 0x80 != 0;
        let values = (packed >> 9) as usize;
        let is_last = packed & 0x100 != 0;
        property_values += values;
        if zeros {
            has_zero_mask = true;
            zero_mask_bits += values;
        }
        if is_last {
            break;
        }
        if pos > 4096 {
            return None;
        }
    }
    if zero_mask_bits > 0 {
        let bytes = if zero_mask_bits <= 8 {
            1
        } else if zero_mask_bits <= 16 {
            2
        } else {
            zero_mask_bits.div_ceil(32) * 4
        };
        let _ = data.get(pos..pos + bytes)?;
        pos += bytes;
    }
    Some(LegacyPropertyHeader {
        property_values,
        header_bytes: pos,
        has_zero_mask,
    })
}

/// 从旧式 `.uexp` 启发式读取 UTexture2D 基础 mip。
pub fn read_legacy_texture(
    asset: &[u8],
    uexp: &[u8],
    ubulk: Option<&[u8]>,
) -> Result<Texture2D, PackageError> {
    let ubulk =
        ubulk.ok_or_else(|| PackageError::Unsupported("旧式纹理缺少 .ubulk".to_string()))?;
    let (width, height, pixel_format) = find_legacy_platform(uexp)?;
    let size = legacy_mip_size(&pixel_format, width, height)?;
    let data = ubulk
        .get(..size)
        .ok_or_else(|| invalid("旧式纹理基础 mip 超出 .ubulk 长度"))?
        .to_vec();
    let name = parse_legacy_summary(asset)
        .map(|summary| summary.export_name)
        .unwrap_or_default();
    Ok(Texture2D {
        name,
        width,
        height,
        pixel_format,
        mips: vec![TextureMip {
            width,
            height,
            depth: 1,
            data,
        }],
    })
}

/// 在旧式导出数据里寻找 `FTexturePlatformData`。
fn find_legacy_platform(data: &[u8]) -> Result<(i32, i32, String), PackageError> {
    let mut offset = 0usize;
    while offset + 16 <= data.len() {
        let width = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or_default());
        let height =
            i32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap_or_default());
        if (1..=16384).contains(&width) && (1..=16384).contains(&height) {
            let length = i32::from_le_bytes(
                data[offset + 12..offset + 16]
                    .try_into()
                    .unwrap_or_default(),
            );
            if (1..=64).contains(&length) {
                let start = offset + 16;
                let end = start.saturating_add(length as usize);
                if let Some(raw) = data.get(start..end) {
                    let format = String::from_utf8_lossy(raw)
                        .trim_end_matches('\0')
                        .to_string();
                    if format.starts_with("PF_") && legacy_mip_size(&format, width, height).is_ok()
                    {
                        return Ok((width, height, format));
                    }
                }
            }
        }
        offset += 1;
    }
    Err(PackageError::Unsupported(
        "找不到旧式 FTexturePlatformData".to_string(),
    ))
}

/// 基础 mip 的压缩字节数。
fn legacy_mip_size(pixel_format: &str, width: i32, height: i32) -> Result<usize, PackageError> {
    let width = usize::try_from(width.max(1)).unwrap_or(1);
    let height = usize::try_from(height.max(1)).unwrap_or(1);
    let blocks = width.div_ceil(4).saturating_mul(height.div_ceil(4));
    let size = match pixel_format.to_ascii_uppercase().as_str() {
        "PF_DXT1" | "PF_BC1" | "PF_BC4" => blocks.saturating_mul(8),
        "PF_DXT3" | "PF_BC2" | "PF_DXT5" | "PF_BC3" | "PF_BC5" | "PF_BC6H" | "PF_BC7" => {
            blocks.saturating_mul(16)
        }
        "PF_B8G8R8A8" | "PF_R8G8B8A8" => width.saturating_mul(height).saturating_mul(4),
        "PF_G8" | "PF_R8" => width.saturating_mul(height),
        other => {
            return Err(PackageError::Unsupported(format!(
                "旧式纹理像素格式 {other} 暂不支持"
            )));
        }
    };
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析属性头统计。
    #[test]
    fn reads_unversioned_property_header() {
        let data = [0x03, 0x02, 0x0D, 0x02, 0x02, 0x02, 0x06, 0x02, 0x01, 0x03];
        let header = read_legacy_property_header(&data).expect("属性头");
        assert_eq!(header.property_values, 5);
        assert_eq!(header.header_bytes, 10);
        assert!(!header.has_zero_mask);
    }

    /// 计算 BC 纹理基础 mip 大小。
    #[test]
    fn computes_bc_mip_size() {
        assert_eq!(legacy_mip_size("PF_BC5", 512, 512).expect("BC5"), 262_144);
        assert_eq!(legacy_mip_size("PF_DXT1", 4, 4).expect("DXT1"), 8);
    }

    /// 构造一个最小的 FF7 风格 unversioned 旧式包摘要。
    fn synthetic_package() -> Vec<u8> {
        let mut names = Vec::new();
        for name in ["None", "Texture2D", "/Script/Engine", "T_Test"] {
            names.extend_from_slice(&(name.len() as i32 + 1).to_le_bytes());
            names.extend_from_slice(name.as_bytes());
            names.push(0);
            names.extend_from_slice(&0u32.to_le_bytes());
        }
        let name_offset = 73usize;
        let import_offset = name_offset + names.len();
        let mut imports = Vec::new();
        for _ in 0..1 {
            imports.extend_from_slice(&2u32.to_le_bytes());
            imports.extend_from_slice(&0u32.to_le_bytes());
            imports.extend_from_slice(&0u32.to_le_bytes());
            imports.extend_from_slice(&0u32.to_le_bytes());
            imports.extend_from_slice(&0i32.to_le_bytes());
            imports.extend_from_slice(&1u32.to_le_bytes());
            imports.extend_from_slice(&0u32.to_le_bytes());
        }
        let export_offset = import_offset + imports.len();
        let mut exports = Vec::new();
        exports.extend_from_slice(&(-1i32).to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&3u32.to_le_bytes());
        exports.extend_from_slice(&0u32.to_le_bytes());
        exports.extend_from_slice(&0u32.to_le_bytes());
        exports.extend_from_slice(&0i64.to_le_bytes());
        exports.extend_from_slice(&0i64.to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&[0u8; 16]);
        exports.extend_from_slice(&0u32.to_le_bytes());
        exports.extend_from_slice(&0i32.to_le_bytes());
        exports.extend_from_slice(&1i32.to_le_bytes());
        exports.extend_from_slice(&[0u8; 20]);

        let mut data = Vec::new();
        data.extend_from_slice(&0x9E2A_83C1u32.to_le_bytes());
        data.extend_from_slice(&(-7i32).to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&5i32.to_le_bytes());
        data.extend_from_slice(&5i32.to_le_bytes());
        data.extend_from_slice(&b"None\0"[..]);
        data.extend_from_slice(&0x8000_2000u32.to_le_bytes());
        data.extend_from_slice(&4i32.to_le_bytes());
        data.extend_from_slice(&(name_offset as i32).to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&(export_offset as i32).to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&(import_offset as i32).to_le_bytes());
        data.extend_from_slice(&names);
        data.extend_from_slice(&imports);
        data.extend_from_slice(&exports);
        data
    }

    /// 缓存摘要与类名可被正确解析。
    #[test]
    fn parses_minimal_unversioned_summary() {
        let data = synthetic_package();
        assert!(is_legacy_package(&data));
        let summary = parse_legacy_summary(&data).expect("legacy summary");
        assert_eq!(summary.package_name, "None");
        assert_eq!(summary.class_name, "Texture2D");
        assert_eq!(summary.export_name, "T_Test");
        assert_eq!(summary.export_count, 1);
        assert_eq!(summary.import_count, 1);
        assert_eq!(summary.name_count, 4);
    }
}
