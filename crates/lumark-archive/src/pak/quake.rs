//! Quake/idTech PACK 归档读取。
//!
//! 结构为 12 字节头（`PACK`、目录偏移、目录长度）加 64 字节目录项；条目只
//! 保存原始数据，没有压缩、时间戳或校验和。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use lumark_legacy::archive::{ArchiveReader, Entry};
use lumark_legacy::detect::ArchiveFormat;
use lumark_legacy::error::{GeeZipError, GeeZipResult};

/// Quake PACK 只读读取器。
pub(crate) struct QuakePakReader {
    path: PathBuf,
    entries: Vec<Entry>,
    records: Vec<(u64, u64)>,
}

impl QuakePakReader {
    /// 打开并解析目录表。
    pub(crate) fn new(path: impl Into<PathBuf>) -> GeeZipResult<Self> {
        let path = path.into();
        let mut file = File::open(&path)
            .map_err(|error| GeeZipError::io(error, format!("opening '{}'", path.display())))?;
        let length = file
            .metadata()
            .map_err(|error| GeeZipError::io(error, format!("statting '{}'", path.display())))?
            .len();
        let mut header = [0u8; 12];
        file.read_exact(&mut header)
            .map_err(|error| GeeZipError::io(error, "reading Quake PAK header"))?;
        if &header[..4] != b"PACK" {
            return Err(GeeZipError::format(
                format!("'{}' is not a Quake PACK archive", path.display()),
                ArchiveFormat::QuakePak,
            ));
        }
        let directory_offset =
            u32::from_le_bytes(header[4..8].try_into().unwrap_or_default()) as u64;
        let directory_length =
            u32::from_le_bytes(header[8..12].try_into().unwrap_or_default()) as u64;
        if !directory_length.is_multiple_of(64) {
            return Err(GeeZipError::format(
                "Quake PAK directory length is not a multiple of 64",
                ArchiveFormat::QuakePak,
            ));
        }
        let count = directory_length / 64;
        if count > 1_000_000 {
            return Err(GeeZipError::format(
                "Quake PAK entry count exceeds the safety limit",
                ArchiveFormat::QuakePak,
            ));
        }
        if directory_offset
            .checked_add(directory_length)
            .is_none_or(|end| end > length)
        {
            return Err(GeeZipError::format(
                "Quake PAK directory is outside the file",
                ArchiveFormat::QuakePak,
            ));
        }
        file.seek(SeekFrom::Start(directory_offset))
            .map_err(|error| GeeZipError::io(error, "seeking Quake PAK directory"))?;
        let mut entries = Vec::with_capacity(count as usize);
        let mut records = Vec::with_capacity(count as usize);
        let mut item = [0u8; 64];
        for _ in 0..count {
            file.read_exact(&mut item)
                .map_err(|error| GeeZipError::io(error, "reading Quake PAK directory entry"))?;
            let name_end = item[..56].iter().position(|byte| *byte == 0).unwrap_or(56);
            let name = String::from_utf8_lossy(&item[..name_end])
                .replace('\\', "/")
                .trim()
                .to_string();
            let offset = u32::from_le_bytes(item[56..60].try_into().unwrap_or_default()) as u64;
            let size = u32::from_le_bytes(item[60..64].try_into().unwrap_or_default()) as u64;
            if name.is_empty() {
                continue;
            }
            if offset.checked_add(size).is_none_or(|end| end > length) {
                return Err(GeeZipError::format(
                    format!("Quake PAK entry '{name}' is outside the file"),
                    ArchiveFormat::QuakePak,
                ));
            }
            entries.push(Entry {
                path: name,
                size,
                compressed_size: size,
                crc32: None,
                modified: None,
                is_dir: false,
            });
            records.push((offset, size));
        }
        Ok(Self {
            path,
            entries,
            records,
        })
    }
}

impl ArchiveReader for QuakePakReader {
    /// 返回 Quake PAK 格式标识。
    fn format(&self) -> ArchiveFormat {
        ArchiveFormat::QuakePak
    }

    /// 返回目录表中的全部条目。
    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        Ok(self.entries.clone())
    }

    /// 按偏移复制原始条目数据。
    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        let Some(index) = self.entries.iter().position(|item| item.path == entry.path) else {
            return Err(GeeZipError::EntryNotFound {
                name: entry.path.clone(),
            });
        };
        let (offset, size) = self.records[index];
        let mut file = File::open(&self.path).map_err(|error| {
            GeeZipError::io(error, format!("opening '{}'", self.path.display()))
        })?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| GeeZipError::io(error, format!("seeking '{}'", entry.path)))?;
        let mut remaining = size;
        let mut buffer = [0u8; 64 * 1024];
        let mut written = 0u64;
        while remaining > 0 {
            let take = remaining.min(buffer.len() as u64) as usize;
            let read = file
                .read(&mut buffer[..take])
                .map_err(|error| GeeZipError::io(error, format!("reading '{}'", entry.path)))?;
            if read == 0 {
                return Err(GeeZipError::format(
                    format!("truncated Quake PAK entry '{}'", entry.path),
                    ArchiveFormat::QuakePak,
                ));
            }
            writer
                .write_all(&buffer[..read])
                .map_err(|error| GeeZipError::io(error, format!("writing '{}'", entry.path)))?;
            written += read as u64;
            remaining -= read as u64;
        }
        Ok(written)
    }
}
