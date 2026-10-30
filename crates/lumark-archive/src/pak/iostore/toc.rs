//! Unreal IoStore（`.utoc`）头部、目录索引和压缩块解析。
//!
//! 文件名映射来自 TOC 末尾的目录索引；该索引在加密容器中同样使用
//! AES-256-ECB 加密，因此列目录必须提供密钥。块数据存放于同名 `.ucas`，
//! 读取时按压缩块偏移流式解密、解压。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::ArchiveError;

use super::super::codec::{decode_key, decrypt_ecb};

/// `.utoc` 头部魔数。
pub(crate) const TOC_MAGIC: &[u8; 16] = b"-==--==--==--==-";
/// TOC 与目录索引读取上限。
pub(crate) const MAX_TOC_BYTES: u64 = 512 * 1024 * 1024;
/// 条目、目录和文件名数量安全上限。
pub(crate) const MAX_ENTRIES: usize = 1_000_000;
/// 当前支持的最高 IoStore TOC 版本（UE5.6）。
pub(crate) const MAX_TOC_VERSION: u8 = 8;
/// 容器加密标志。
const FLAG_ENCRYPTED: u8 = 2;
/// 容器签名标志。
const FLAG_SIGNED: u8 = 4;
/// 压缩块固定序列化长度。
const COMPRESSION_BLOCK_SIZE: usize = 12;
/// SHA-1 签名长度。
const SHA1_BYTES: usize = 20;

/// 解析后的 IoStore TOC 头部。
#[derive(Debug, Clone)]
pub(crate) struct Header {
    pub(crate) version: u8,
    pub(crate) header_size: u32,
    pub(crate) entry_count: u32,
    pub(crate) compression_block_count: u32,
    pub(crate) compression_method_count: u32,
    pub(crate) compression_method_name_length: u32,
    pub(crate) compression_block_size: u32,
    pub(crate) directory_index_size: u32,
    pub(crate) partition_count: u32,
    pub(crate) container_flags: u8,
    pub(crate) perfect_hash_seeds_count: u32,
    pub(crate) chunks_without_perfect_hash_count: u32,
}

impl Header {
    /// 判断容器是否加密。
    pub(crate) fn encrypted(&self) -> bool {
        self.container_flags & FLAG_ENCRYPTED != 0
    }

    /// 判断容器是否带签名表。
    fn signed(&self) -> bool {
        self.container_flags & FLAG_SIGNED != 0
    }
}

/// 逻辑偏移与长度（大端 5+5 字节存储）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct OffsetAndLength {
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

/// 压缩块在 `.ucas` 中的物理偏移、大小和算法索引。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CompressionBlock {
    pub(crate) offset: u64,
    pub(crate) compressed_size: u32,
    pub(crate) uncompressed_size: u32,
    /// 0 表示不压缩；其它值是从 1 开始的压缩方法索引。
    pub(crate) method_index: u8,
}

/// 目录索引中的一个文件。
#[derive(Debug, Clone)]
pub(crate) struct FileRecord {
    pub(crate) path: String,
    pub(crate) toc_index: u32,
}

/// 完整解析后的 TOC。
pub(crate) struct Toc {
    pub(crate) header: Header,
    pub(crate) chunk_ids: Vec<[u8; 12]>,
    pub(crate) offsets: Vec<OffsetAndLength>,
    pub(crate) blocks: Vec<CompressionBlock>,
    pub(crate) compression_methods: Vec<String>,
    pub(crate) files: Vec<FileRecord>,
    /// 规范化路径到 TOC 条目索引的映射。
    pub(crate) file_indices: HashMap<String, u32>,
}

/// 只读字节游标，所有越界都会转为可读错误。
struct Bytes<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Bytes<'a> {
    /// 创建游标。
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// 读取指定长度切片。
    fn take(&mut self, length: usize) -> Result<&'a [u8], ArchiveError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| corrupt("IoStore 索引长度溢出"))?;
        let slice = self
            .data
            .get(self.position..end)
            .ok_or_else(|| corrupt("IoStore 索引被截断"))?;
        self.position = end;
        Ok(slice)
    }

    /// 读取 u8。
    fn u8(&mut self) -> Result<u8, ArchiveError> {
        Ok(self.take(1)?[0])
    }

    /// 读取 u16。
    fn u16(&mut self) -> Result<u16, ArchiveError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u32。
    fn u32(&mut self) -> Result<u32, ArchiveError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u64。
    fn u64(&mut self) -> Result<u64, ArchiveError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i32。
    fn i32(&mut self) -> Result<i32, ArchiveError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 Unreal FString（正长度 UTF-8 / 负长度 UTF-16，均含结尾 NUL）。
    fn string(&mut self) -> Result<String, ArchiveError> {
        let length = self.i32()?;
        if length == 0 {
            return Ok(String::new());
        }
        if length < 0 {
            let units = (-length) as usize;
            if units > MAX_ENTRIES {
                return Err(corrupt("IoStore 字符串长度超过上限"));
            }
            let bytes = self.take(
                units
                    .checked_mul(2)
                    .ok_or_else(|| corrupt("字符串长度溢出"))?,
            )?;
            let mut values = Vec::with_capacity(units);
            for chunk in bytes.as_chunks::<2>().0 {
                values.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            while values.last() == Some(&0) {
                values.pop();
            }
            return String::from_utf16(&values)
                .map_err(|error| corrupt(format!("非法 UTF-16 字符串：{error}")));
        }
        let length = length as usize;
        if length > 64 * 1024 * 1024 {
            return Err(corrupt("IoStore 字符串长度超过上限"));
        }
        let bytes = self.take(length)?;
        Ok(String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string())
    }

    /// 跳过指定字节。
    fn skip(&mut self, length: usize) -> Result<(), ArchiveError> {
        let _ = self.take(length)?;
        Ok(())
    }
}

/// 构造统一的损坏错误。
fn corrupt(message: impl Into<String>) -> ArchiveError {
    ArchiveError::Corrupt(message.into())
}

/// 从 `.utoc` 文件读取并解析 TOC。
pub(crate) fn read_toc(toc_path: &Path, password: Option<&str>) -> Result<Toc, ArchiveError> {
    let length = std::fs::metadata(toc_path)?.len();
    if length > MAX_TOC_BYTES {
        return Err(corrupt("IoStore TOC 超过 512 MiB 安全上限"));
    }
    let data = std::fs::read(toc_path)?;
    let mut cursor = Bytes::new(&data);
    let header = read_header(&mut cursor)?;
    let key = if header.encrypted() {
        match password {
            Some(password) => Some(decode_key(password).map_err(ArchiveError::Corrupt)?),
            None => return Err(ArchiveError::PasswordRequired),
        }
    } else {
        None
    };
    if header.partition_count != 1 {
        return Err(corrupt(format!(
            "暂不支持分片 IoStore 容器（partition_count={}）",
            header.partition_count
        )));
    }
    cursor.position = header.header_size as usize;

    let chunk_ids = read_chunk_ids(&mut cursor, &header)?;
    let offsets = read_offsets(&mut cursor, &header)?;
    read_hash_map(&mut cursor, &header)?;
    let blocks = read_compression_blocks(&mut cursor, &header)?;
    let compression_methods = read_compression_methods(&mut cursor, &header)?;
    read_signatures(&mut cursor, &header)?;
    let files = match read_directory_index(&mut cursor, &header, key.as_ref()) {
        Ok(files) => files,
        // 加密容器里解密成功但结构不成立，几乎都是密钥不对；把解析错误
        // 统一映射为 WrongPassword，界面才能给出“密钥无效”的恢复路径。
        Err(ArchiveError::Corrupt(_)) if header.encrypted() => {
            return Err(ArchiveError::WrongPassword);
        }
        Err(error) => return Err(error),
    };

    let file_indices = files
        .iter()
        .map(|file| (file.path.clone(), file.toc_index))
        .collect::<HashMap<_, _>>();
    Ok(Toc {
        header,
        chunk_ids,
        offsets,
        blocks,
        compression_methods,
        files,
        file_indices,
    })
}

/// 只读取头部，用于便宜的加密探测。
pub(crate) fn probe_header(toc_path: &Path) -> Result<Header, ArchiveError> {
    let length = std::fs::metadata(toc_path)?.len();
    if length < 0x90 {
        return Err(corrupt("IoStore TOC 文件过短"));
    }
    let mut file = std::fs::File::open(toc_path)?;
    let mut header = vec![0u8; 0x90];
    use std::io::Read as _;
    file.read_exact(&mut header)?;
    read_header(&mut Bytes::new(&header))
}

/// 解析 0x90 字节头部。
fn read_header(cursor: &mut Bytes<'_>) -> Result<Header, ArchiveError> {
    let magic = cursor.take(16)?;
    if magic != TOC_MAGIC {
        return Err(corrupt("IoStore TOC 魔数不存在"));
    }
    let version = cursor.u8()?;
    if version == 0 || version > MAX_TOC_VERSION {
        return Err(corrupt(format!("不支持的 IoStore TOC 版本 {version}")));
    }
    let _reserved0 = cursor.u8()?;
    let _reserved1 = cursor.u16()?;
    let header_size = cursor.u32()?;
    if header_size < 0x90 {
        return Err(corrupt("IoStore TOC 头长度非法"));
    }
    let entry_count = cursor.u32()?;
    let compression_block_count = cursor.u32()?;
    let compression_block_entry_size = cursor.u32()?;
    if compression_block_entry_size as usize != COMPRESSION_BLOCK_SIZE {
        return Err(corrupt(format!(
            "不支持的 IoStore 压缩块条目长度 {compression_block_entry_size}"
        )));
    }
    let compression_method_count = cursor.u32()?;
    let compression_method_name_length = cursor.u32()?;
    if compression_method_name_length > 256 {
        return Err(corrupt("IoStore 压缩方法名过长"));
    }
    let compression_block_size = cursor.u32()?;
    if compression_block_size == 0 {
        return Err(corrupt("IoStore 压缩块大小为 0"));
    }
    let directory_index_size = cursor.u32()?;
    let partition_count = cursor.u32()?;
    let _container_id = cursor.u64()?;
    let _encryption_key_guid = cursor.take(16)?;
    let container_flags = cursor.u8()?;
    let _reserved3 = cursor.u8()?;
    let _reserved4 = cursor.u16()?;
    let perfect_hash_seeds_count = cursor.u32()?;
    let _partition_size = cursor.u64()?;
    let chunks_without_perfect_hash_count = cursor.u32()?;
    let _reserved7 = cursor.u32()?;
    let _reserved8 = cursor.take(40)?;
    if entry_count as usize > MAX_ENTRIES
        || compression_block_count as usize > MAX_ENTRIES
        || compression_method_count as usize > 64
        || directory_index_size as usize > MAX_TOC_BYTES as usize
        || perfect_hash_seeds_count as usize > MAX_ENTRIES
        || chunks_without_perfect_hash_count as usize > MAX_ENTRIES
    {
        return Err(corrupt("IoStore TOC 计数超过安全上限"));
    }
    Ok(Header {
        version,
        header_size,
        entry_count,
        compression_block_count,
        compression_method_count,
        compression_method_name_length,
        compression_block_size,
        directory_index_size,
        partition_count,
        container_flags,
        perfect_hash_seeds_count,
        chunks_without_perfect_hash_count,
    })
}

/// 读取 TOC 条目 ID 表。
fn read_chunk_ids(cursor: &mut Bytes<'_>, header: &Header) -> Result<Vec<[u8; 12]>, ArchiveError> {
    let bytes = cursor.take(header.entry_count as usize * 12)?;
    let mut ids = Vec::with_capacity(header.entry_count as usize);
    for chunk in bytes.as_chunks::<12>().0 {
        ids.push(*chunk);
    }
    Ok(ids)
}

/// 读取每条目的逻辑偏移和长度。
fn read_offsets(
    cursor: &mut Bytes<'_>,
    header: &Header,
) -> Result<Vec<OffsetAndLength>, ArchiveError> {
    let bytes = cursor.take(header.entry_count as usize * 10)?;
    let mut offsets = Vec::with_capacity(header.entry_count as usize);
    for chunk in bytes.as_chunks::<10>().0 {
        let offset =
            u64::from_be_bytes([0, 0, 0, chunk[0], chunk[1], chunk[2], chunk[3], chunk[4]]);
        let length =
            u64::from_be_bytes([0, 0, 0, chunk[5], chunk[6], chunk[7], chunk[8], chunk[9]]);
        offsets.push(OffsetAndLength { offset, length });
    }
    Ok(offsets)
}

/// 跳过完美哈希种子与回退索引。
fn read_hash_map(cursor: &mut Bytes<'_>, header: &Header) -> Result<(), ArchiveError> {
    let seeds = if header.version >= 4 {
        header.perfect_hash_seeds_count as usize
    } else {
        0
    };
    let overflow = if header.version >= 5 {
        header.chunks_without_perfect_hash_count as usize
    } else {
        0
    };
    cursor.skip(
        seeds
            .checked_mul(4)
            .ok_or_else(|| corrupt("IoStore 哈希种子长度溢出"))?,
    )?;
    cursor.skip(
        overflow
            .checked_mul(4)
            .ok_or_else(|| corrupt("IoStore 回退索引长度溢出"))?,
    )?;
    Ok(())
}

/// 读取压缩块表。
fn read_compression_blocks(
    cursor: &mut Bytes<'_>,
    header: &Header,
) -> Result<Vec<CompressionBlock>, ArchiveError> {
    let bytes = cursor.take(header.compression_block_count as usize * COMPRESSION_BLOCK_SIZE)?;
    let mut blocks = Vec::with_capacity(header.compression_block_count as usize);
    for chunk in bytes.as_chunks::<COMPRESSION_BLOCK_SIZE>().0 {
        let offset =
            u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], 0, 0, 0]);
        let compressed_size = u32::from_le_bytes([chunk[5], chunk[6], chunk[7], 0]);
        let uncompressed_size = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], 0]);
        blocks.push(CompressionBlock {
            offset,
            compressed_size,
            uncompressed_size,
            method_index: chunk[11],
        });
    }
    Ok(blocks)
}

/// 读取压缩方法名表。
fn read_compression_methods(
    cursor: &mut Bytes<'_>,
    header: &Header,
) -> Result<Vec<String>, ArchiveError> {
    let width = header.compression_method_name_length as usize;
    let mut methods = Vec::with_capacity(header.compression_method_count as usize);
    for _ in 0..header.compression_method_count {
        let bytes = cursor.take(width)?;
        let name = String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        methods.push(name);
    }
    Ok(methods)
}

/// 跳过签名表；只校验长度边界，不验证签名。
fn read_signatures(cursor: &mut Bytes<'_>, header: &Header) -> Result<(), ArchiveError> {
    if !header.signed() {
        return Ok(());
    }
    let size = cursor.u32()? as usize;
    if size > 64 * 1024 {
        return Err(corrupt("IoStore 签名长度超过上限"));
    }
    cursor.skip(size.checked_mul(2).ok_or_else(|| corrupt("签名长度溢出"))?)?;
    cursor.skip(
        (header.compression_block_count as usize)
            .checked_mul(SHA1_BYTES)
            .ok_or_else(|| corrupt("块签名长度溢出"))?,
    )?;
    Ok(())
}

/// 目录索引条目。
#[derive(Debug, Clone, Copy)]
struct DirectoryEntry {
    name: Option<u32>,
    first_child: Option<u32>,
    next_sibling: Option<u32>,
    first_file: Option<u32>,
}

/// 文件索引条目。
#[derive(Debug, Clone, Copy)]
struct FileEntry {
    name: u32,
    next_file: Option<u32>,
    user_data: u32,
}

/// 读取并解析目录索引，返回规范化后的文件列表。
fn read_directory_index(
    cursor: &mut Bytes<'_>,
    header: &Header,
    key: Option<&[u8; 32]>,
) -> Result<Vec<FileRecord>, ArchiveError> {
    let size = header.directory_index_size as usize;
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut data = cursor.take(size)?.to_vec();
    if header.encrypted() {
        let key = key.ok_or(ArchiveError::PasswordRequired)?;
        if !data.len().is_multiple_of(16) {
            return Err(corrupt("IoStore 目录索引长度不是 16 的倍数"));
        }
        if let Err(error) = decrypt_ecb(key, &mut data) {
            return Err(if header.encrypted() {
                ArchiveError::WrongPassword
            } else {
                corrupt(error)
            });
        }
    }
    let mut cursor = Bytes::new(&data);
    let mount = cursor.string()?;
    let directory_count = cursor.u32()? as usize;
    if directory_count > MAX_ENTRIES {
        return Err(corrupt("IoStore 目录数超过安全上限"));
    }
    let mut directories = Vec::with_capacity(directory_count);
    for _ in 0..directory_count {
        directories.push(DirectoryEntry {
            name: optional_index(cursor.u32()?)?,
            first_child: optional_index(cursor.u32()?)?,
            next_sibling: optional_index(cursor.u32()?)?,
            first_file: optional_index(cursor.u32()?)?,
        });
    }
    let file_count = cursor.u32()? as usize;
    if file_count > MAX_ENTRIES {
        return Err(corrupt("IoStore 文件索引数超过安全上限"));
    }
    let mut file_entries = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        file_entries.push(FileEntry {
            name: cursor.u32()?,
            next_file: optional_index(cursor.u32()?)?,
            user_data: cursor.u32()?,
        });
    }
    let string_count = cursor.u32()? as usize;
    if string_count > MAX_ENTRIES {
        return Err(corrupt("IoStore 字符串数超过安全上限"));
    }
    let mut strings = Vec::with_capacity(string_count);
    for _ in 0..string_count {
        strings.push(cursor.string()?);
    }
    validate_index(&directories, &file_entries, &strings)?;
    collect_files(
        &mount,
        &directories,
        &file_entries,
        &strings,
        header.entry_count,
    )
}

/// `u32::MAX` 表示 `None`。
fn optional_index(value: u32) -> Result<Option<u32>, ArchiveError> {
    if value == u32::MAX {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

/// 校验所有下标都在对应表范围内，避免损坏索引导致 panic。
fn validate_index(
    directories: &[DirectoryEntry],
    files: &[FileEntry],
    strings: &[String],
) -> Result<(), ArchiveError> {
    let directory_count = directories.len() as u32;
    let file_count = files.len() as u32;
    let string_count = strings.len() as u32;
    for directory in directories {
        if directory.name.is_some_and(|value| value >= string_count) {
            return Err(corrupt("IoStore 目录名索引越界"));
        }
        if directory
            .first_child
            .is_some_and(|value| value >= directory_count)
            || directory
                .next_sibling
                .is_some_and(|value| value >= directory_count)
            || directory
                .first_file
                .is_some_and(|value| value >= file_count)
        {
            return Err(corrupt("IoStore 目录索引越界"));
        }
    }
    for file in files {
        if file.name >= string_count || file.next_file.is_some_and(|value| value >= file_count) {
            return Err(corrupt("IoStore 文件索引越界"));
        }
    }
    Ok(())
}

/// 遍历目录树并生成完整路径。
fn collect_files(
    mount: &str,
    directories: &[DirectoryEntry],
    files: &[FileEntry],
    strings: &[String],
    chunk_count: u32,
) -> Result<Vec<FileRecord>, ArchiveError> {
    if directories.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let mut stack = Vec::new();
    let mut visited = HashSet::new();
    collect_directory(
        0,
        directories,
        files,
        strings,
        mount,
        chunk_count,
        &mut records,
        &mut stack,
        &mut visited,
        0,
    )?;
    records.sort_by(|left, right| left.path.cmp(&right.path));
    records.dedup_by(|left, right| left.path == right.path);
    Ok(records)
}

/// 递归收集单个目录及其子树。
///
/// `visited` 防止损坏索引形成环；`stack` 保存当前目录层级，文件路径由
/// 层级加文件名拼出，最后统一按挂载点规范化。
#[allow(clippy::too_many_arguments)]
fn collect_directory(
    index: u32,
    directories: &[DirectoryEntry],
    files: &[FileEntry],
    strings: &[String],
    mount: &str,
    chunk_count: u32,
    records: &mut Vec<FileRecord>,
    stack: &mut Vec<String>,
    visited: &mut HashSet<u32>,
    depth: usize,
) -> Result<(), ArchiveError> {
    if depth > 128 {
        return Err(corrupt("IoStore 目录层级过深"));
    }
    if !visited.insert(index) {
        return Err(corrupt("IoStore 目录索引存在环"));
    }
    let directory = directories
        .get(index as usize)
        .ok_or_else(|| corrupt("IoStore 目录索引越界"))?;
    let name = match directory.name {
        Some(name) => {
            let name = strings
                .get(name as usize)
                .ok_or_else(|| corrupt("IoStore 目录名越界"))?
                .clone();
            stack.push(name);
            true
        }
        None => false,
    };

    let mut file_index = directory.first_file;
    let mut guard = 0usize;
    while let Some(entry_index) = file_index {
        guard += 1;
        if guard > files.len() {
            return Err(corrupt("IoStore 文件索引存在环"));
        }
        let file = files
            .get(entry_index as usize)
            .ok_or_else(|| corrupt("IoStore 文件索引越界"))?;
        let file_name = strings
            .get(file.name as usize)
            .ok_or_else(|| corrupt("IoStore 文件名越界"))?;
        let mut parts = stack.clone();
        parts.push(file_name.clone());
        let path = normalize_path(mount, &parts.join("/"));
        if !path.is_empty() {
            if file.user_data >= chunk_count {
                return Err(corrupt("IoStore 文件指向不存在的块"));
            }
            records.push(FileRecord {
                path,
                toc_index: file.user_data,
            });
        }
        if records.len() > MAX_ENTRIES {
            return Err(corrupt("IoStore 文件数超过安全上限"));
        }
        file_index = file.next_file;
    }

    let mut child = directory.first_child;
    let mut child_guard = 0usize;
    while let Some(child_index) = child {
        child_guard += 1;
        if child_guard > directories.len() {
            return Err(corrupt("IoStore 子目录索引存在环"));
        }
        collect_directory(
            child_index,
            directories,
            files,
            strings,
            mount,
            chunk_count,
            records,
            stack,
            visited,
            depth + 1,
        )?;
        let entry = directories
            .get(child_index as usize)
            .ok_or_else(|| corrupt("IoStore 子目录索引越界"))?;
        child = entry.next_sibling;
    }

    if name {
        stack.pop();
    }
    Ok(())
}

/// 规范化挂载点和相对路径，去掉开头的 `..` 与分隔符。
fn normalize_path(mount: &str, relative: &str) -> String {
    let joined = format!("{mount}/{relative}");
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
