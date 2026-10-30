//! Zen 包的名称表与映射名。
//!
//! 名称批次由 Unreal 的批量序列化写入：数量、字符串总字节数、哈希版本、
//! 名称哈希、i16 长度（正数 ASCII / 负数 UTF-16）和字符串本体。

use std::io::Cursor;

use super::{PackageError, invalid, read_bytes, read_u32, read_u64};

/// 名称批次数量上限。
const MAX_NAMES: usize = 1_000_000;

/// 一个映射名：低 30 位是名称索引，高位是名称空间，`number` 是序号。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MappedName {
    index_and_type: u32,
    pub(crate) number: u32,
}

impl MappedName {
    /// 名称索引。
    pub(crate) fn index(self) -> usize {
        (self.index_and_type & 0x3FFF_FFFF) as usize
    }

    /// 名称空间（0 包，1 容器，2 全局）。
    pub(crate) fn kind(self) -> u32 {
        self.index_and_type >> 30
    }
}

/// 名称表。
#[derive(Debug, Clone, Default)]
pub(crate) struct NameMap {
    names: Vec<String>,
}

impl NameMap {
    /// 从字节流读取名称批次。
    pub(crate) fn read(cursor: &mut Cursor<&[u8]>) -> Result<Self, PackageError> {
        let count = read_u32(cursor)? as usize;
        if count > MAX_NAMES {
            return Err(invalid("Zen 名称数量超过安全上限"));
        }
        if count == 0 {
            return Ok(Self::default());
        }
        let _string_bytes = read_u32(cursor)?;
        let _hash_version = read_u64(cursor)?;
        // 跳过每个名称的 64 位哈希。
        let hashes = count
            .checked_mul(8)
            .ok_or_else(|| invalid("名称哈希长度溢出"))?;
        let _ = read_bytes(cursor, hashes)?;

        let mut lengths = Vec::with_capacity(count);
        for _ in 0..count {
            lengths.push(i16::from_be_bytes(
                read_bytes(cursor, 2)?.try_into().unwrap_or_default(),
            ));
        }
        let mut names = Vec::with_capacity(count);
        for length in lengths {
            let name = if length < 0 {
                let units = i16::MIN - length;
                let bytes = read_bytes(cursor, units as usize * 2)?;
                let units = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                    .collect::<Vec<_>>();
                String::from_utf16(&units)
                    .map_err(|error| invalid(format!("非法 UTF-16 名称：{error}")))?
            } else {
                let bytes = read_bytes(cursor, length as usize)?;
                String::from_utf8_lossy(bytes).to_string()
            };
            names.push(name);
        }
        Ok(Self { names })
    }

    /// 读取一个映射名。
    pub(crate) fn read_name(cursor: &mut Cursor<&[u8]>) -> Result<MappedName, PackageError> {
        Ok(MappedName {
            index_and_type: read_u32(cursor)?,
            number: read_u32(cursor)?,
        })
    }

    /// 解析映射名为带序号的字符串。
    pub(crate) fn get(&self, name: MappedName) -> Result<String, PackageError> {
        if name.kind() > 2 {
            return Err(invalid("映射名名称空间非法"));
        }
        let base = self
            .names
            .get(name.index())
            .ok_or_else(|| invalid("映射名索引越界"))?;
        if name.number == 0 {
            Ok(base.clone())
        } else {
            Ok(format!("{base}_{}", name.number - 1))
        }
    }

    /// 返回原始名称数量。
    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// 返回全部名称（用于诊断和元数据展示）。
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    /// 按索引返回名称。
    pub(crate) fn name_at(&self, index: usize) -> Option<&str> {
        self.names.get(index).map(String::as_str)
    }
}
