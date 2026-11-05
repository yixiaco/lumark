//! `FByteBulkData` 读取：Zen bulk data 映射与传统内联两种形式。
//!
//! UE5.2 起（对象版本 `DATA_RESOURCES`）cooked 包的 bulk 数据由 Zen 头中的
//! bulk data 映射描述，导出数据里只序列化一个映射索引，长度/偏移/标志都在映射
//! 表里；更早的包仍内联完整的 `FByteBulkData` 头。两种形式的载荷都可能内联在
//! 导出数据中，或存放在容器的 `.ubulk`（IoStore 的 BulkData chunk）里，因此
//! 调用方必须把已读到的侧载数据传进来，否则纹理 mip、网格顶点等数据读不出来。

use super::{BulkDataEntry, PackageError, invalid};

/// `BULKDATA_Unused`：该条目在 cook 时被裁掉。
const FLAG_UNUSED: u32 = 1 << 5;
/// `BULKDATA_ForceInlinePayload`：载荷紧跟头部内联存放。
const FLAG_FORCE_INLINE: u32 = 1 << 6;
/// `BULKDATA_Size64Bit`：长度字段使用 64 位。
const FLAG_SIZE_64_BIT: u32 = 1 << 13;
/// `BULKDATA_LazyLoadable`：没有独立载荷，按内联处理。
const FLAG_LAZY_LOADABLE: u32 = 1 << 18;
/// 单条 bulk 数据长度上限，避免损坏索引要求过大内存。
const MAX_BULK_BYTES: usize = 1024 * 1024 * 1024;

/// `FByteBulkData` 读取所需的游标能力。
pub(crate) trait BulkReader {
    /// 返回当前读取位置。
    fn position(&self) -> usize;
    /// 复位读取位置。
    fn set_position(&mut self, position: usize);
    /// 读取指定长度并把游标后移。
    fn take(&mut self, length: usize) -> Result<&[u8], PackageError>;
    /// 读取 u32。
    fn read_u32(&mut self) -> Result<u32, PackageError>;
    /// 读取 i32。
    fn read_i32(&mut self) -> Result<i32, PackageError>;
    /// 读取 i64。
    fn read_i64(&mut self) -> Result<i64, PackageError>;
}

/// 一次 bulk 读取结果。
#[derive(Debug)]
pub(crate) struct BulkRead {
    /// 载荷；`None` 表示数据在容器 `.ubulk` 中而调用方没有提供侧载数据。
    pub(crate) data: Option<Vec<u8>>,
    /// bulk 声明的长度，用于空数据判定与错误提示。
    pub(crate) size: usize,
}

/// 读取一个 `FByteBulkData`。
///
/// `entries` 为包级 bulk data 映射（为空表示该包使用传统内联头）；`ubulk` 为
/// 容器中该包的 `.ubulk` 内容；`label` 只用于错误信息。
pub(crate) fn read_byte_bulk<R: BulkReader>(
    reader: &mut R,
    entries: &[BulkDataEntry],
    ubulk: Option<&[u8]>,
    label: &str,
) -> Result<BulkRead, PackageError> {
    if !entries.is_empty() {
        let index_position = reader.position();
        let index = reader.read_i32()?;
        if let Some(entry) = usize::try_from(index)
            .ok()
            .and_then(|index| entries.get(index))
        {
            return read_mapped(reader, entry, ubulk, label);
        }
        // 索引越界说明该包没有使用映射，回退为传统头解析。
        reader.set_position(index_position);
    }
    read_legacy(reader, ubulk, label)
}

/// 读取映射表中的一条 bulk。
fn read_mapped<R: BulkReader>(
    reader: &mut R,
    entry: &BulkDataEntry,
    ubulk: Option<&[u8]>,
    label: &str,
) -> Result<BulkRead, PackageError> {
    let size = bulk_size(entry.serial_size, label)?;
    if entry.flags & FLAG_UNUSED != 0 {
        return Ok(BulkRead {
            data: Some(Vec::new()),
            size: 0,
        });
    }
    if is_inline(entry.flags) {
        let data = reader.take(size)?.to_vec();
        return Ok(BulkRead {
            data: Some(data),
            size,
        });
    }
    let offset = bulk_offset(entry.serial_offset, label)?;
    Ok(BulkRead {
        data: slice_ubulk(ubulk, offset, size, label)?,
        size,
    })
}

/// 读取传统 `FByteBulkData` 头及其载荷。
fn read_legacy<R: BulkReader>(
    reader: &mut R,
    ubulk: Option<&[u8]>,
    label: &str,
) -> Result<BulkRead, PackageError> {
    let flags = reader.read_u32()?;
    let (element_count, size_on_disk) = if flags & FLAG_SIZE_64_BIT != 0 {
        (reader.read_i64()?, reader.read_i64()?)
    } else {
        (reader.read_i32()? as i64, reader.read_u32()? as i64)
    };
    let offset = reader.read_i64()?;
    let declared = if size_on_disk > 0 {
        size_on_disk
    } else {
        element_count
    };
    let size = bulk_size(declared, label)?;
    if flags & FLAG_UNUSED != 0 {
        return Ok(BulkRead {
            data: Some(Vec::new()),
            size: 0,
        });
    }
    if is_inline(flags) {
        return Ok(BulkRead {
            data: Some(reader.take(size)?.to_vec()),
            size,
        });
    }
    let offset = bulk_offset(offset, label)?;
    Ok(BulkRead {
        data: slice_ubulk(ubulk, offset, size, label)?,
        size,
    })
}

/// 判断载荷是否内联在导出数据中。
///
/// UE 在高版本里把“无独立载荷”的条目写成 `ForceInlinePayload`、`LazyLoadable`
/// 或全零标志，这三种都按内联读取。
fn is_inline(flags: u32) -> bool {
    flags & FLAG_FORCE_INLINE != 0 || flags & FLAG_LAZY_LOADABLE != 0 || flags == 0
}

/// 校验 bulk 声明长度。
fn bulk_size(value: i64, label: &str) -> Result<usize, PackageError> {
    if value < 0 {
        return Err(invalid(format!("{label} bulk 数据长度为负")));
    }
    let size = value as usize;
    if size > MAX_BULK_BYTES {
        return Err(invalid(format!("{label} bulk 数据超过 1 GiB 上限")));
    }
    Ok(size)
}

/// 校验 bulk 数据偏移。
fn bulk_offset(value: i64, label: &str) -> Result<usize, PackageError> {
    if value < 0 {
        return Err(invalid(format!("{label} bulk 数据偏移为负")));
    }
    Ok(value as usize)
}

/// 从 `.ubulk` 载荷中切出指定区间；未提供侧载数据时返回 `None`。
fn slice_ubulk(
    ubulk: Option<&[u8]>,
    offset: usize,
    size: usize,
    label: &str,
) -> Result<Option<Vec<u8>>, PackageError> {
    let Some(ubulk) = ubulk else {
        return Ok(None);
    };
    if size == 0 {
        return Ok(Some(Vec::new()));
    }
    let end = offset
        .checked_add(size)
        .ok_or_else(|| invalid(format!("{label} bulk 数据长度溢出")))?;
    let bytes = ubulk
        .get(offset..end)
        .ok_or_else(|| invalid(format!("{label} bulk 数据超出 .ubulk 范围")))?;
    Ok(Some(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的字节游标，只覆盖本模块需要的读取能力。
    struct TestReader<'a> {
        data: &'a [u8],
        position: usize,
    }

    impl<'a> TestReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self { data, position: 0 }
        }
    }

    impl BulkReader for TestReader<'_> {
        fn position(&self) -> usize {
            self.position
        }

        fn set_position(&mut self, position: usize) {
            self.position = position;
        }

        fn take(&mut self, length: usize) -> Result<&[u8], PackageError> {
            let end = self
                .position
                .checked_add(length)
                .ok_or_else(|| invalid("测试游标越界"))?;
            let bytes = self
                .data
                .get(self.position..end)
                .ok_or_else(|| invalid("测试游标越界"))?;
            self.position = end;
            Ok(bytes)
        }

        fn read_u32(&mut self) -> Result<u32, PackageError> {
            Ok(u32::from_le_bytes(
                self.take(4)?.try_into().unwrap_or_default(),
            ))
        }

        fn read_i32(&mut self) -> Result<i32, PackageError> {
            Ok(i32::from_le_bytes(
                self.take(4)?.try_into().unwrap_or_default(),
            ))
        }

        fn read_i64(&mut self) -> Result<i64, PackageError> {
            Ok(i64::from_le_bytes(
                self.take(8)?.try_into().unwrap_or_default(),
            ))
        }
    }

    /// 构造一条映射条目。
    fn entry(offset: i64, size: i64, flags: u32) -> BulkDataEntry {
        BulkDataEntry {
            serial_offset: offset,
            duplicate_serial_offset: offset,
            serial_size: size,
            flags,
            cooked_index: 0,
        }
    }

    /// 映射形式 + `ForceInlinePayload`：载荷紧跟索引内联。
    #[test]
    fn reads_inline_payload_from_map() {
        let mut data = Vec::new();
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let entries = [entry(900, 8, FLAG_FORCE_INLINE)];
        let mut reader = TestReader::new(&data);
        let bulk = read_byte_bulk(&mut reader, &entries, None, "纹理 mip").unwrap();
        assert_eq!(bulk.size, 8);
        assert_eq!(bulk.data.unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(reader.position(), data.len());
    }

    /// 映射形式 + 外部载荷：从 `.ubulk` 的映射偏移处读取。
    #[test]
    fn reads_external_payload_from_map_out_of_ubulk() {
        let mut data = Vec::new();
        data.extend_from_slice(&1i32.to_le_bytes());
        let entries = [entry(4, 3, 1 << 8), entry(16, 4, 1 << 8)];
        let ubulk = [0xAAu8; 32];
        let mut reader = TestReader::new(&data);
        let bulk = read_byte_bulk(&mut reader, &entries, Some(&ubulk), "纹理 mip").unwrap();
        assert_eq!(bulk.size, 4);
        assert_eq!(bulk.data.unwrap(), vec![0xAA; 4]);
    }

    /// 映射索引越界时回退传统形式。
    #[test]
    fn falls_back_to_legacy_header_when_map_index_is_out_of_range() {
        let mut data = Vec::new();
        // 同一个 4 字节既是越界索引（256），又恰好是 `PayloadInSeperateFile` 标志。
        data.extend_from_slice(&(1u32 << 8).to_le_bytes());
        data.extend_from_slice(&3i32.to_le_bytes()); // element_count
        data.extend_from_slice(&3u32.to_le_bytes()); // size_on_disk
        data.extend_from_slice(&1i64.to_le_bytes()); // offset
        let entries = [entry(0, 1, 1 << 8)];
        let ubulk = [0u8, 7, 8, 9];
        let mut reader = TestReader::new(&data);
        let bulk = read_byte_bulk(&mut reader, &entries, Some(&ubulk), "纹理 mip").unwrap();
        assert_eq!(bulk.size, 3);
        assert_eq!(bulk.data.unwrap(), vec![7, 8, 9]);
    }

    /// 传统形式 + `PayloadInSeperateFile` + 提供 `.ubulk`：必须读到外部载荷。
    #[test]
    fn reads_external_payload_from_ubulk_with_legacy_header() {
        let mut data = Vec::new();
        data.extend_from_slice(&(1u32 << 8).to_le_bytes());
        data.extend_from_slice(&5i32.to_le_bytes());
        data.extend_from_slice(&5u32.to_le_bytes());
        data.extend_from_slice(&2i64.to_le_bytes());
        let ubulk = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let mut reader = TestReader::new(&data);
        let bulk = read_byte_bulk(&mut reader, &[], Some(&ubulk), "顶点").unwrap();
        assert_eq!(bulk.size, 5);
        assert_eq!(bulk.data.unwrap(), vec![2, 3, 4, 5, 6]);
    }

    /// 传统形式缺 `.ubulk` 时返回 `None` 载荷但要保留声明长度。
    #[test]
    fn missing_ubulk_reports_declared_size_only() {
        let mut data = Vec::new();
        data.extend_from_slice(&(1u32 << 8).to_le_bytes());
        data.extend_from_slice(&16i32.to_le_bytes());
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&0i64.to_le_bytes());
        let mut reader = TestReader::new(&data);
        let bulk = read_byte_bulk(&mut reader, &[], None, "顶点").unwrap();
        assert_eq!(bulk.size, 16);
        assert!(bulk.data.is_none());
    }

    /// `Unused` 条目不读取载荷。
    #[test]
    fn unused_entry_yields_empty_payload() {
        let mut data = Vec::new();
        data.extend_from_slice(&0i32.to_le_bytes());
        let entries = [entry(64, 4096, FLAG_UNUSED)];
        let mut reader = TestReader::new(&data);
        let bulk = read_byte_bulk(&mut reader, &entries, None, "纹理 mip").unwrap();
        assert_eq!(bulk.size, 0);
        assert_eq!(bulk.data.unwrap(), Vec::<u8>::new());
    }

    /// `.ubulk` 不足以容纳声明的区间时给出可读错误。
    #[test]
    fn out_of_range_ubulk_slice_is_rejected() {
        let mut data = Vec::new();
        data.extend_from_slice(&0i32.to_le_bytes());
        let entries = [entry(8, 8, 1 << 8)];
        let ubulk = [0u8; 12];
        let mut reader = TestReader::new(&data);
        let error = read_byte_bulk(&mut reader, &entries, Some(&ubulk), "纹理 mip").unwrap_err();
        assert!(error.to_string().contains(".ubulk"), "{error}");
    }
}
