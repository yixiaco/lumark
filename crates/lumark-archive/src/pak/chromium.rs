//! Chromium/Electron 数据包（`.pak`）读取。
//!
//! 数据包按资源 ID 索引，没有文件名；这里把主资源表映射为
//! `resources/<id>`，别名表只用于校验目标索引，不重复列出同一份数据。

use std::io::{Read, Write};
use std::path::PathBuf;

use lumark_legacy::archive::{ArchiveReader, Entry};
use lumark_legacy::detect::ArchiveFormat;
use lumark_legacy::error::{GeeZipError, GeeZipResult};

/// 允许解压的最大数据包大小；Chromium 资源包通常只有几 MB 到几十 MB。
const MAX_UNPACKED_PACK: u64 = 256 * 1024 * 1024;

/// 允许读取的最大数据包大小；超过时拒绝，避免一次性读入失控。
const MAX_PACK_BYTES: u64 = 512 * 1024 * 1024;

/// Chromium 数据包只读读取器。
pub(crate) struct ChromiumPakReader {
    data: Vec<u8>,
    entries: Vec<Entry>,
    records: Vec<(usize, usize)>,
}

impl ChromiumPakReader {
    /// 读取数据包（必要时先解开 gzip 外壳）并解析资源表。
    pub(crate) fn new(path: impl Into<PathBuf>) -> GeeZipResult<Self> {
        let path = path.into();
        let length = std::fs::metadata(&path)
            .map_err(|error| GeeZipError::io(error, format!("statting '{}'", path.display())))?
            .len();
        if length > MAX_PACK_BYTES {
            return Err(GeeZipError::format(
                "Chromium PAK exceeds the 512 MiB safety limit",
                ArchiveFormat::ChromiumPak,
            ));
        }
        let raw = std::fs::read(&path)
            .map_err(|error| GeeZipError::io(error, format!("opening '{}'", path.display())))?;
        let data = if raw.starts_with(b"\x1f\x8b") {
            let decoder = flate2::read::GzDecoder::new(&raw[..]);
            let mut output = Vec::new();
            decoder
                .take(MAX_UNPACKED_PACK + 1)
                .read_to_end(&mut output)
                .map_err(|error| GeeZipError::io(error, "decompressing Chromium PAK"))?;
            if output.len() as u64 > MAX_UNPACKED_PACK {
                return Err(GeeZipError::format(
                    "Chromium PAK exceeds the 256 MiB safety limit",
                    ArchiveFormat::ChromiumPak,
                ));
            }
            output
        } else {
            raw
        };
        let (resources, _aliases) = parse_pack(&data)?;
        let mut entries = Vec::with_capacity(resources.len());
        let mut records = Vec::with_capacity(resources.len());
        for (id, offset, length) in &resources {
            entries.push(Entry {
                path: format!("resources/{id}"),
                size: *length as u64,
                compressed_size: *length as u64,
                crc32: None,
                modified: None,
                is_dir: false,
            });
            records.push((*offset, *length));
        }
        Ok(Self {
            data,
            entries,
            records,
        })
    }
}

/// 主资源表：`(资源 ID, 偏移, 长度)`。
type Resources = Vec<(u16, usize, usize)>;
/// 别名表：`(别名 ID, 目标资源索引)`。
type Aliases = Vec<(u16, u16)>;

/// 解析 v4/v5 头部，返回主资源 `(id, offset, length)` 与别名 `(id, target)`。
fn parse_pack(data: &[u8]) -> GeeZipResult<(Resources, Aliases)> {
    if data.len() < 9 {
        return Err(GeeZipError::format(
            "Chromium PAK is truncated",
            ArchiveFormat::ChromiumPak,
        ));
    }
    let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let (header_len, resource_count, alias_count, encoding) = match version {
        4 => {
            let count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
            (9usize, count, 0usize, data[8])
        }
        5 => {
            if data.len() < 12 {
                return Err(GeeZipError::format(
                    "Chromium PAK is truncated",
                    ArchiveFormat::ChromiumPak,
                ));
            }
            let count = u16::from_le_bytes([data[8], data[9]]) as usize;
            let aliases = u16::from_le_bytes([data[10], data[11]]) as usize;
            (12usize, count, aliases, data[4])
        }
        _ => {
            return Err(GeeZipError::format(
                format!("unsupported Chromium PAK version {version}"),
                ArchiveFormat::ChromiumPak,
            ));
        }
    };
    if encoding > 2 {
        return Err(GeeZipError::format(
            "invalid Chromium PAK text encoding",
            ArchiveFormat::ChromiumPak,
        ));
    }
    let resource_table = (resource_count + 1).checked_mul(6).ok_or_else(|| {
        GeeZipError::format("Chromium PAK table overflow", ArchiveFormat::ChromiumPak)
    })?;
    let alias_table = alias_count.checked_mul(4).ok_or_else(|| {
        GeeZipError::format("Chromium PAK table overflow", ArchiveFormat::ChromiumPak)
    })?;
    let table_end = header_len
        .checked_add(resource_table)
        .and_then(|value| value.checked_add(alias_table))
        .ok_or_else(|| {
            GeeZipError::format("Chromium PAK table overflow", ArchiveFormat::ChromiumPak)
        })?;
    if table_end > data.len() {
        return Err(GeeZipError::format(
            "Chromium PAK resource table is outside the file",
            ArchiveFormat::ChromiumPak,
        ));
    }
    let mut offsets = Vec::with_capacity(resource_count + 1);
    for index in 0..=resource_count {
        let position = header_len + index * 6;
        let offset = u32::from_le_bytes([
            data[position + 2],
            data[position + 3],
            data[position + 4],
            data[position + 5],
        ]) as usize;
        if offset > data.len() || offsets.last().is_some_and(|previous| offset < *previous) {
            return Err(GeeZipError::format(
                "Chromium PAK resource offsets are invalid",
                ArchiveFormat::ChromiumPak,
            ));
        }
        offsets.push(offset);
    }
    let mut resources = Vec::with_capacity(resource_count);
    for index in 0..resource_count {
        let position = header_len + index * 6;
        let id = u16::from_le_bytes([data[position], data[position + 1]]);
        let start = offsets[index];
        let end = offsets[index + 1];
        if start < table_end || end < start {
            return Err(GeeZipError::format(
                "Chromium PAK resource range is invalid",
                ArchiveFormat::ChromiumPak,
            ));
        }
        resources.push((id, start, end - start));
    }
    let alias_start = header_len + (resource_count + 1) * 6;
    let mut aliases = Vec::with_capacity(alias_count);
    for index in 0..alias_count {
        let position = alias_start + index * 4;
        let id = u16::from_le_bytes([data[position], data[position + 1]]);
        let target = u16::from_le_bytes([data[position + 2], data[position + 3]]);
        if target as usize >= resource_count {
            return Err(GeeZipError::format(
                "Chromium PAK alias target is invalid",
                ArchiveFormat::ChromiumPak,
            ));
        }
        aliases.push((id, target));
    }
    Ok((resources, aliases))
}

impl ArchiveReader for ChromiumPakReader {
    /// 返回 Chromium 数据包格式标识。
    fn format(&self) -> ArchiveFormat {
        ArchiveFormat::ChromiumPak
    }

    /// 返回主资源表映射出的条目。
    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        Ok(self.entries.clone())
    }

    /// 按资源偏移复制数据。
    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        let Some(index) = self.entries.iter().position(|item| item.path == entry.path) else {
            return Err(GeeZipError::EntryNotFound {
                name: entry.path.clone(),
            });
        };
        let (offset, length) = self.records[index];
        let end = offset.checked_add(length).ok_or_else(|| {
            GeeZipError::format(
                "Chromium PAK resource range overflowed",
                ArchiveFormat::ChromiumPak,
            )
        })?;
        if end > self.data.len() {
            return Err(GeeZipError::format(
                "Chromium PAK resource is outside the file",
                ArchiveFormat::ChromiumPak,
            ));
        }
        writer
            .write_all(&self.data[offset..end])
            .map_err(|error| GeeZipError::io(error, format!("writing '{}'", entry.path)))?;
        Ok(length as u64)
    }
}
