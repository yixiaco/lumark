//! UE4 旧式 `uasset` 摘要读取。
//!
//! FF7 Remake 等 UE4.27 游戏的 cooked 包使用无版本属性；完整资产预览需要
//! `.usmap`，但摘要、名称表、导入/导出表不需要映射，可先展示包级元数据。

use super::PackageError;

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

#[cfg(test)]
mod tests {
    use super::*;

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
