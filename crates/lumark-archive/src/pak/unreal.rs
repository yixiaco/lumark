//! Unreal Engine 4/5 PAK 只读解析。
//!
//! 支持版本 1–11 的 footer、普通索引和 v10+ 的完整目录/编码条目；压缩支持
//! Store/Zlib/Gzip/LZ4/Zstd，加密支持 AES-256-ECB，Oodle 需要用户自备
//! `oo2core` DLL。格式细节参考 trumank/repak（MIT OR Apache-2.0）生成的真实
//! UnrealPak 样本；本模块按 Lumark 的流式、预算和错误模型重写。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use lumark_legacy::archive::{ArchiveReader, Entry};
use lumark_legacy::detect::ArchiveFormat;
use lumark_legacy::error::{GeeZipError, GeeZipResult};

use crate::error::ArchiveError;

/// Unreal PAK footer 魔数。
const MAGIC: u32 = 0x5A6F12E1;
/// 索引读取上限。
const MAX_INDEX_BYTES: u64 = 512 * 1024 * 1024;
/// 单条加密条目缓存的字节上限。
const MAX_ENCRYPTED_ENTRY: u64 = 256 * 1024 * 1024;
/// 条目/块数量安全上限。
const MAX_ENTRIES: usize = 1_000_000;

/// 解析后的 footer。
#[derive(Debug, Clone)]
struct Footer {
    /// PAK 版本 1–11。
    version: u32,
    /// 索引偏移。
    index_offset: u64,
    /// 索引大小。
    index_size: u64,
    /// 索引是否加密。
    encrypted: bool,
    /// 是否冻结索引。
    frozen: bool,
    /// 压缩名槽位；空槽位为 `None`。
    compression: Vec<Option<String>>,
}

/// 索引中的条目元数据。
#[derive(Debug, Clone)]
struct PakEntry {
    /// 归档内路径。
    path: String,
    /// 条目头偏移。
    offset: u64,
    /// 压缩后大小。
    compressed: u64,
    /// 原始大小。
    uncompressed: u64,
    /// 压缩槽位；`None` 表示不压缩。
    compression_slot: Option<u32>,
    /// 压缩块 `(start, end)`。
    blocks: Vec<(u64, u64)>,
    /// 是否加密。
    encrypted: bool,
    /// 压缩块原始大小。
    block_size: u32,
}

/// 判断是否为 v8a（压缩索引使用 u8；v8b 使用 u32）。
fn u8_compression_index(footer: &Footer) -> bool {
    footer.version == 8 && footer.compression.len() == 4
}

/// 只读的字节游标，所有越界都返回可读错误。
struct Bytes<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Bytes<'a> {
    /// 创建游标。
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// 读取指定长度的切片。
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| "Unreal PAK 索引长度溢出".to_string())?;
        let slice = self
            .data
            .get(self.position..end)
            .ok_or_else(|| "Unreal PAK 索引被截断".to_string())?;
        self.position = end;
        Ok(slice)
    }

    /// 读取 u8。
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    /// 读取 u32。
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i32。
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u64。
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 UE 的 FString（正长度 UTF-8 / 负长度 UTF-16）。
    fn string(&mut self) -> Result<String, String> {
        let length = self.i32()?;
        if length < 0 {
            let units = (-length) as usize;
            if units > 1_000_000 {
                return Err("Unreal PAK 字符串长度超过上限".to_string());
            }
            let bytes = self.take(units.checked_mul(2).ok_or("字符串长度溢出")?)?;
            let mut values = Vec::with_capacity(units);
            for chunk in bytes.as_chunks::<2>().0 {
                values.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            while values.last() == Some(&0) {
                values.pop();
            }
            String::from_utf16(&values).map_err(|error| format!("非法 UTF-16 字符串：{error}"))
        } else {
            let length = length as usize;
            if length > 64 * 1024 * 1024 {
                return Err("Unreal PAK 字符串长度超过上限".to_string());
            }
            let bytes = self.take(length)?;
            let text = String::from_utf8_lossy(bytes).to_string();
            Ok(text.trim_end_matches('\0').to_string())
        }
    }
}

/// 从文件尾部定位并解析 footer。
fn read_footer(path: &Path) -> Result<Footer, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    if length < 64 {
        return Err("Unreal PAK 文件过短".to_string());
    }
    let window = length.min(256);
    file.seek(SeekFrom::End(-(window as i64)))
        .map_err(|error| error.to_string())?;
    let mut tail = vec![0u8; window as usize];
    file.read_exact(&mut tail)
        .map_err(|error| error.to_string())?;
    let tail_start = length - window;
    for index in 0..=tail.len().saturating_sub(44) {
        if tail[index..index + 4] != MAGIC.to_le_bytes() {
            continue;
        }
        let version = u32::from_le_bytes(tail[index + 4..index + 8].try_into().unwrap_or_default());
        if !(1..=11).contains(&version) {
            continue;
        }
        let magic = tail_start + index as u64;
        // footer 的加密标志紧挨在魔数之前；GUID 只在加密 PAK 中由引擎写入，
        // 未加密归档的这 17 字节可能是索引尾部或零填充，不能强依赖。
        if magic == 0 {
            continue;
        }
        let encrypted = version >= 4
            && if index > 0 {
                tail[index - 1] != 0
            } else {
                false
            };
        let footer_length = length - magic;
        if footer_length > 256 {
            continue;
        }
        let footer = &tail[index..];
        let mut cursor = Bytes::new(footer);
        let magic_value = cursor.u32()?;
        if magic_value != MAGIC {
            continue;
        }
        let parsed_version = cursor.u32()?;
        if parsed_version != version {
            continue;
        }
        let index_offset = cursor.u64()?;
        let index_size = cursor.u64()?;
        cursor.take(20)?;
        let frozen = if version == 9 {
            cursor.u8()? != 0
        } else {
            false
        };
        if index_offset
            .checked_add(index_size)
            .is_none_or(|end| end > length)
        {
            continue;
        }
        let remaining = footer.len().saturating_sub(cursor.position);
        let mut compression = Vec::new();
        if version >= 8 {
            let slots = if version >= 9 {
                if remaining != 160 {
                    continue;
                }
                5
            } else if remaining == 128 {
                4
            } else if remaining == 160 {
                5
            } else {
                continue;
            };
            for _ in 0..slots {
                let name = cursor.take(32)?;
                let name = String::from_utf8_lossy(name)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                compression.push((!name.is_empty()).then_some(name));
            }
        } else {
            // 版本 < 8 没有压缩方法名表；槽位 0 固定为不压缩，随后是
            // Zlib / Gzip / Oodle（与 UE4 PAK 的约定一致）。
            compression = vec![
                Some("None".to_string()),
                Some("Zlib".to_string()),
                Some("Gzip".to_string()),
                Some("Oodle".to_string()),
            ];
        }
        return Ok(Footer {
            version,
            index_offset,
            index_size,
            encrypted,
            frozen,
            compression,
        });
    }
    Err("Unreal PAK footer 魔数不存在".to_string())
}

/// 判断 PAK 是否加密以及索引是否加密。
pub(crate) fn probe_encryption(path: &Path) -> Result<(bool, bool), String> {
    let footer = read_footer(path)?;
    if footer.encrypted {
        return Ok((true, true));
    }
    let entries = parse_index(path, &footer, None)?;
    Ok((entries.iter().any(|entry| entry.encrypted), false))
}

/// 用给定密码验证索引或首个加密条目。
pub(crate) fn verify_password(path: &Path, password: &str) -> Result<bool, String> {
    let key = decode_key(password)?;
    let footer = read_footer(path)?;
    match parse_index(path, &footer, Some(&key)) {
        Ok(entries) => {
            if footer.encrypted {
                return Ok(true);
            }
            let Some(entry) = entries.iter().find(|entry| entry.encrypted) else {
                return Ok(true);
            };
            match extract_entry(path, &footer, &key, entry, &mut std::io::sink()) {
                Ok(_) => Ok(true),
                Err(error) => {
                    // Oodle/未知压缩无法通过解密验证，密钥格式正确时不再误报。
                    if error.contains("Oodle") || error.contains("不支持") {
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
            }
        }
        Err(_) => Ok(false),
    }
}

/// 解码用户输入的 AES-256 密钥：64 位十六进制、Base64 或 32 字节原文。
fn decode_key(password: &str) -> Result<[u8; 32], String> {
    let value = password.trim();
    if value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        let mut output = [0u8; 32];
        for (index, chunk) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let text = std::str::from_utf8(chunk).map_err(|error| error.to_string())?;
            output[index] = u8::from_str_radix(text, 16).map_err(|error| error.to_string())?;
        }
        return Ok(output);
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(value)
        && bytes.len() == 32
    {
        let mut output = [0u8; 32];
        output.copy_from_slice(&bytes);
        return Ok(output);
    }
    let bytes = value.as_bytes();
    if bytes.len() == 32 {
        let mut output = [0u8; 32];
        output.copy_from_slice(bytes);
        return Ok(output);
    }
    Err("Unreal PAK 密钥必须是 64 位十六进制、Base64 或 32 字节原文".to_string())
}

/// 解密 AES-256-ECB 数据。
fn decrypt(key: &[u8; 32], bytes: &mut [u8]) -> Result<(), String> {
    if !bytes.len().is_multiple_of(16) {
        return Err("Unreal PAK 加密数据长度不是 16 的倍数".to_string());
    }
    use aes::cipher::{BlockDecrypt, KeyInit};
    let cipher = aes::Aes256::new_from_slice(key).map_err(|error| error.to_string())?;
    for chunk in bytes.as_chunks_mut::<16>().0 {
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(&chunk[..]);
        cipher.decrypt_block(&mut block);
        chunk.copy_from_slice(&block);
    }
    Ok(())
}

/// 完整目录索引：目录名 + 文件 `(名, 编码偏移)`。
type DirectoryIndex = Vec<(String, Vec<(String, i32)>)>;

/// 解析索引，返回条目元数据。
fn parse_index(
    path: &Path,
    footer: &Footer,
    key: Option<&[u8; 32]>,
) -> Result<Vec<PakEntry>, String> {
    if footer.frozen {
        return Err("Unreal PAK v9 冻结索引暂不支持".to_string());
    }
    if footer.index_size > MAX_INDEX_BYTES {
        return Err("Unreal PAK 索引超过安全上限".to_string());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(footer.index_offset))
        .map_err(|error| error.to_string())?;
    let mut index = vec![0u8; footer.index_size as usize];
    file.read_exact(&mut index)
        .map_err(|error| error.to_string())?;
    if footer.encrypted {
        let key = key.ok_or_else(|| "Unreal PAK 索引已加密，需要 AES 密钥".to_string())?;
        decrypt(key, &mut index)?;
    }
    let mut cursor = Bytes::new(&index);
    let mount = cursor.string()?;
    let count = cursor.u32()? as usize;
    if count > MAX_ENTRIES {
        return Err("Unreal PAK 条目数超过安全上限".to_string());
    }
    let mut entries = Vec::new();
    if footer.version >= 10 {
        cursor.u64()?;
        if cursor.u32()? != 0 {
            cursor.take(8 + 8 + 20)?;
        }
        let mut full_directory: Option<DirectoryIndex> = None;
        if cursor.u32()? != 0 {
            let offset = cursor.u64()?;
            let size = cursor.u64()?;
            cursor.take(20)?;
            if size > MAX_INDEX_BYTES {
                return Err("Unreal PAK 完整目录索引超过安全上限".to_string());
            }
            let mut data = vec![0u8; size as usize];
            let mut file = File::open(path).map_err(|error| error.to_string())?;
            file.seek(SeekFrom::Start(offset))
                .map_err(|error| error.to_string())?;
            file.read_exact(&mut data)
                .map_err(|error| error.to_string())?;
            if footer.encrypted {
                let key = key.ok_or_else(|| "Unreal PAK 索引已加密，需要 AES 密钥".to_string())?;
                decrypt(key, &mut data)?;
            }
            let mut directory = Bytes::new(&data);
            let directories = directory.u32()? as usize;
            if directories > MAX_ENTRIES {
                return Err("Unreal PAK 目录数超过安全上限".to_string());
            }
            let mut parsed = Vec::with_capacity(directories);
            for _ in 0..directories {
                let dir = directory.string()?;
                let file_count = directory.u32()? as usize;
                if file_count > MAX_ENTRIES {
                    return Err("Unreal PAK 文件数超过安全上限".to_string());
                }
                let mut files = Vec::with_capacity(file_count);
                for _ in 0..file_count {
                    files.push((directory.string()?, directory.i32()?));
                }
                parsed.push((dir, files));
            }
            full_directory = Some(parsed);
        }
        let encoded_size = cursor.u32()? as usize;
        let encoded = cursor.take(encoded_size)?;
        let non_encoded_count = cursor.u32()? as usize;
        if non_encoded_count > MAX_ENTRIES {
            return Err("Unreal PAK 非编码条目数超过安全上限".to_string());
        }
        let mut non_encoded = Vec::with_capacity(non_encoded_count);
        for _ in 0..non_encoded_count {
            non_encoded.push(parse_entry(
                &mut cursor,
                footer.version,
                u8_compression_index(footer),
            )?);
        }
        if let Some(directory) = full_directory {
            for (dir, files) in directory {
                for (file, encoded_offset) in files {
                    if encoded_offset == i32::MIN {
                        continue;
                    }
                    let mut entry = if encoded_offset >= 0 {
                        let mut reader = Bytes::new(encoded);
                        reader.position = encoded_offset as usize;
                        parse_encoded_entry(&mut reader, footer.version)?
                    } else {
                        let index = (-encoded_offset) as usize - 1;
                        non_encoded
                            .get(index)
                            .cloned()
                            .ok_or_else(|| "Unreal PAK 非编码条目索引越界".to_string())?
                    };
                    let directory_prefix = dir.trim_start_matches('/');
                    entry.path = format!("{directory_prefix}{file}").replace('\\', "/");
                    entries.push(entry);
                }
            }
        }
    } else {
        for _ in 0..count {
            let name = cursor.string()?;
            let mut entry = parse_entry(&mut cursor, footer.version, u8_compression_index(footer))?;
            entry.path = name.replace('\\', "/");
            entries.push(entry);
        }
    }
    let _ = mount;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries.dedup_by(|left, right| left.path == right.path);
    Ok(entries)
}

/// 读取普通索引条目。
fn parse_entry(
    cursor: &mut Bytes<'_>,
    version: u32,
    u8_compression_index: bool,
) -> Result<PakEntry, String> {
    let offset = cursor.u64()?;
    let compressed = cursor.u64()?;
    let uncompressed = cursor.u64()?;
    let compression_value = if u8_compression_index {
        cursor.u8()? as u32
    } else {
        cursor.u32()?
    };
    let compression_slot = (compression_value != 0).then(|| compression_value - 1);
    if version == 1 {
        cursor.u64()?;
    }
    cursor.take(20)?;
    let mut blocks = Vec::new();
    if version >= 3 && compression_slot.is_some() {
        let count = cursor.u32()?;
        if count as usize > MAX_ENTRIES {
            return Err("Unreal PAK 压缩块数超过安全上限".to_string());
        }
        for _ in 0..count {
            blocks.push((cursor.u64()?, cursor.u64()?));
        }
    }
    let encrypted = if version >= 3 {
        cursor.u8()? & 1 != 0
    } else {
        false
    };
    let block_size = if version >= 3 { cursor.u32()? } else { 0 };
    Ok(PakEntry {
        path: String::new(),
        offset,
        compressed,
        uncompressed,
        compression_slot,
        blocks,
        encrypted,
        block_size,
    })
}

/// 读取 v10+ 编码条目。
fn parse_encoded_entry(cursor: &mut Bytes<'_>, version: u32) -> Result<PakEntry, String> {
    let bits = cursor.u32()?;
    let compression_slot = match (bits >> 23) & 0x3F {
        0 => None,
        value => Some(value - 1),
    };
    let encrypted = bits & (1 << 22) != 0;
    let block_count = ((bits >> 6) & 0xFFFF) as usize;
    if block_count > MAX_ENTRIES {
        return Err("Unreal PAK 压缩块数超过安全上限".to_string());
    }
    let mut block_size = bits & 0x3F;
    if block_size == 0x3F {
        block_size = cursor.u32()?;
    } else {
        block_size <<= 11;
    }
    let value = |cursor: &mut Bytes<'_>, bit: u32| -> Result<u64, String> {
        if bits & (1 << bit) != 0 {
            Ok(cursor.u32()? as u64)
        } else {
            Ok(cursor.u64()?)
        }
    };
    let offset = value(cursor, 31)?;
    let uncompressed = value(cursor, 30)?;
    let compressed = if compression_slot.is_some() {
        value(cursor, 29)?
    } else {
        uncompressed
    };
    let base = entry_serialized_size(version, compression_slot, block_count as u32, false);
    let mut blocks = Vec::new();
    if block_count == 1 && !encrypted {
        blocks.push((base, base + compressed));
    } else if block_count > 0 {
        let mut start = base;
        for _ in 0..block_count {
            let size = cursor.u32()? as u64;
            blocks.push((start, start + size));
            start += if encrypted { (size + 15) & !15 } else { size };
        }
    }
    Ok(PakEntry {
        path: String::new(),
        offset,
        compressed,
        uncompressed,
        compression_slot,
        blocks,
        encrypted,
        block_size,
    })
}

/// 条目头序列化长度，用于换算块偏移。
fn entry_serialized_size(
    version: u32,
    compression: Option<u32>,
    block_count: u32,
    u8_compression_index: bool,
) -> u64 {
    let mut size = 8 + 8 + 8;
    size += if u8_compression_index { 1 } else { 4 };
    size += if version == 1 { 8 } else { 0 };
    size += 20;
    if compression.is_some() {
        size += 4 + 16 * u64::from(block_count);
    }
    if version >= 3 {
        size += 1 + 4;
    }
    size
}

/// 解压单条条目到写入器。
fn extract_entry<W: Write + ?Sized>(
    path: &Path,
    footer: &Footer,
    key: &[u8; 32],
    entry: &PakEntry,
    writer: &mut W,
) -> Result<u64, String> {
    if entry.compressed == 0 && entry.uncompressed == 0 {
        return Ok(0);
    }
    let header_size = entry_serialized_size(
        footer.version,
        entry.compression_slot,
        entry.blocks.len() as u32,
        u8_compression_index(footer),
    );
    let data_start = entry
        .offset
        .checked_add(header_size)
        .ok_or_else(|| "Unreal PAK 条目偏移溢出".to_string())?;
    let mut compression = entry
        .compression_slot
        .map(|slot| {
            footer
                .compression
                .get(slot as usize)
                .cloned()
                .flatten()
                .ok_or_else(|| format!("Unreal PAK 压缩槽 {slot} 无对应算法"))
        })
        .transpose()?;
    // 部分 UE4.20 时代的 UnrealPak 样本把第一个压缩算法写在槽 0（没有
    // 前置的 None 槽）。真实 UE4.27+ PAK 的槽 0 仍是不压缩；这里只对
    // “槽 0 但确实有压缩块”的旧样本回退为 Zlib。
    if footer.version < 8
        && compression.as_deref() == Some("None")
        && entry.compressed != entry.uncompressed
    {
        compression = Some("Zlib".to_string());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    if entry.encrypted {
        if entry.compressed > MAX_ENCRYPTED_ENTRY {
            return Err("Unreal PAK 加密条目超过 256 MiB 上限".to_string());
        }
        let padded = (entry.compressed + 15) & !15;
        let mut data = vec![0u8; padded as usize];
        file.seek(SeekFrom::Start(data_start))
            .map_err(|error| error.to_string())?;
        file.read_exact(&mut data)
            .map_err(|error| error.to_string())?;
        decrypt(key, &mut data)?;
        data.truncate(entry.compressed as usize);
        return decompress_blocks(
            footer,
            entry,
            compression.as_deref(),
            &data,
            data_start,
            writer,
        );
    }
    if compression.is_none() {
        file.seek(SeekFrom::Start(data_start))
            .map_err(|error| error.to_string())?;
        let mut remaining = entry.compressed;
        let mut buffer = [0u8; 64 * 1024];
        let mut written = 0u64;
        while remaining > 0 {
            let take = remaining.min(buffer.len() as u64) as usize;
            let read = file
                .read(&mut buffer[..take])
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("Unreal PAK 条目被截断".to_string());
            }
            writer
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            written += read as u64;
            remaining -= read as u64;
        }
        return Ok(written);
    }
    if entry.blocks.is_empty() {
        let mut data = vec![0u8; entry.compressed as usize];
        file.seek(SeekFrom::Start(data_start))
            .map_err(|error| error.to_string())?;
        file.read_exact(&mut data)
            .map_err(|error| error.to_string())?;
        return decompress_blocks(
            footer,
            entry,
            compression.as_deref(),
            &data,
            data_start,
            writer,
        );
    }
    // 未加密的多块条目逐块读取，避免整条缓存。
    let mut written = 0u64;
    for (index, (start, end)) in entry.blocks.iter().enumerate() {
        let relative = if footer.version >= 5 {
            start.saturating_sub(data_start.saturating_sub(entry.offset))
        } else {
            start.saturating_sub(data_start)
        };
        let length = end.saturating_sub(*start);
        let block_start = data_start
            .checked_add(relative)
            .ok_or_else(|| "Unreal PAK 块偏移溢出".to_string())?;
        file.seek(SeekFrom::Start(block_start))
            .map_err(|error| error.to_string())?;
        let mut data = vec![0u8; length as usize];
        file.read_exact(&mut data)
            .map_err(|error| error.to_string())?;
        let expected = expected_block_size(entry, index);
        let mut output = vec![0u8; expected as usize];
        decompress_block(
            compression.as_deref().unwrap_or("Store"),
            &data,
            &mut output,
        )?;
        writer
            .write_all(&output)
            .map_err(|error| error.to_string())?;
        written += output.len() as u64;
    }
    if written != entry.uncompressed {
        return Err(format!(
            "Unreal PAK 条目大小不一致：期望 {}，得到 {written}",
            entry.uncompressed
        ));
    }
    Ok(written)
}

/// 对已解密的条目数据按块解压。
fn decompress_blocks<W: Write + ?Sized>(
    footer: &Footer,
    entry: &PakEntry,
    compression: Option<&str>,
    data: &[u8],
    data_start: u64,
    writer: &mut W,
) -> Result<u64, String> {
    let mut written = 0u64;
    if entry.blocks.is_empty() {
        let mut output = vec![0u8; entry.uncompressed as usize];
        decompress_block(compression.unwrap_or("Store"), data, &mut output)?;
        writer
            .write_all(&output)
            .map_err(|error| error.to_string())?;
        return Ok(output.len() as u64);
    }
    for (index, (start, end)) in entry.blocks.iter().enumerate() {
        let relative = if footer.version >= 5 {
            start.saturating_sub(data_start.saturating_sub(entry.offset))
        } else {
            start.saturating_sub(data_start)
        } as usize;
        let length = end.saturating_sub(*start) as usize;
        let end = relative.checked_add(length).ok_or("块范围溢出")?;
        let slice = data
            .get(relative..end)
            .ok_or_else(|| "Unreal PAK 块范围越界".to_string())?;
        let expected = expected_block_size(entry, index);
        let mut output = vec![0u8; expected as usize];
        decompress_block(compression.unwrap_or("Store"), slice, &mut output)?;
        writer
            .write_all(&output)
            .map_err(|error| error.to_string())?;
        written += output.len() as u64;
    }
    if written != entry.uncompressed {
        return Err(format!(
            "Unreal PAK 条目大小不一致：期望 {}，得到 {written}",
            entry.uncompressed
        ));
    }
    Ok(written)
}

/// 计算第 `index` 个块的原始大小。
fn expected_block_size(entry: &PakEntry, index: usize) -> u64 {
    let block = u64::from(entry.block_size);
    if block == 0 {
        return entry.uncompressed;
    }
    let remaining = entry
        .uncompressed
        .saturating_sub(block.saturating_mul(index as u64));
    remaining.min(block)
}

/// 解压单个块。
fn decompress_block(compression: &str, input: &[u8], output: &mut [u8]) -> Result<(), String> {
    match compression {
        "Store" | "None" | "" => {
            if input.len() != output.len() {
                return Err("Unreal PAK 存储块大小不一致".to_string());
            }
            output.copy_from_slice(input);
            Ok(())
        }
        "Zlib" => {
            let mut decoder = flate2::read::ZlibDecoder::new(input);
            decoder
                .read_exact(output)
                .map_err(|error| format!("Unreal PAK Zlib 解压失败：{error}"))
        }
        "Gzip" => {
            let mut decoder = flate2::read::GzDecoder::new(input);
            decoder
                .read_exact(output)
                .map_err(|error| format!("Unreal PAK Gzip 解压失败：{error}"))
        }
        "LZ4" => lz4_flex::block::decompress_into(input, output)
            .map(|_| ())
            .map_err(|error| format!("Unreal PAK LZ4 解压失败：{error}")),
        "Zstd" => zstd::stream::read::Decoder::new(input)
            .map_err(|error| format!("Unreal PAK Zstd 解压失败：{error}"))?
            .read_exact(output)
            .map_err(|error| format!("Unreal PAK Zstd 解压失败：{error}")),
        "Oodle" => super::oodle::decompress(input, output),
        other => Err(format!("Unreal PAK 压缩算法不支持：{other}")),
    }
}

/// Unreal PAK 只读读取器。
pub(crate) struct UnrealPakReader {
    path: PathBuf,
    footer: Footer,
    key: [u8; 32],
    records: Vec<PakEntry>,
    entries: Vec<Entry>,
}

impl UnrealPakReader {
    /// 打开 PAK；加密索引必须提供有效密钥。
    pub(crate) fn new(path: impl Into<PathBuf>, password: Option<&str>) -> GeeZipResult<Self> {
        let path = path.into();
        let footer = read_footer(&path)
            .map_err(|error| GeeZipError::format(error, ArchiveFormat::UnrealPak))?;
        let key = password
            .map(decode_key)
            .transpose()
            .map_err(|error| GeeZipError::format(error, ArchiveFormat::UnrealPak))?
            .unwrap_or([0u8; 32]);
        let records = parse_index(&path, &footer, password.map(|_| &key))
            .map_err(|error| GeeZipError::format(error, ArchiveFormat::UnrealPak))?;
        let entries = records
            .iter()
            .map(|entry| Entry {
                path: entry.path.clone(),
                size: entry.uncompressed,
                compressed_size: entry.compressed,
                crc32: None,
                modified: None,
                is_dir: false,
            })
            .collect();
        Ok(Self {
            path,
            footer,
            key,
            records,
            entries,
        })
    }
}

impl ArchiveReader for UnrealPakReader {
    /// 返回 Unreal PAK 格式标识。
    fn format(&self) -> ArchiveFormat {
        ArchiveFormat::UnrealPak
    }

    /// 返回索引条目列表。
    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        Ok(self.entries.clone())
    }

    /// 按索引元数据解压单条条目。
    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        let record = self
            .records
            .iter()
            .find(|record| record.path == entry.path)
            .ok_or_else(|| GeeZipError::EntryNotFound {
                name: entry.path.clone(),
            })?;
        extract_entry(&self.path, &self.footer, &self.key, record, writer)
            .map_err(|error| GeeZipError::format(error, ArchiveFormat::UnrealPak))
    }
}

/// PAK 索引中的条目元数据（不暴露内部偏移细节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakAssetEntry {
    /// 归档内路径。
    pub path: String,
    /// 解压后大小。
    pub size: u64,
    /// 压缩后大小。
    pub compressed_size: u64,
}

/// Unreal PAK 的按条目读取器。
///
/// 与 `ArchiveService::extract` 不同，它保持 PAK 打开状态并按路径返回解压后的
/// 完整字节，供资产预览、批量导出等需要随机读取的场景使用；密钥只保存在内存中，
/// 不会写入日志或临时文件。
pub struct PakAssetReader {
    reader: UnrealPakReader,
    has_password: bool,
}

impl PakAssetReader {
    /// 打开 PAK；加密索引必须提供有效密钥。
    pub fn open(path: impl Into<PathBuf>, password: Option<&str>) -> Result<Self, ArchiveError> {
        let path = path.into();
        let footer = read_footer(&path)
            .map_err(|error| ArchiveError::Corrupt(format!("Unreal PAK：{error}")))?;
        if footer.encrypted && password.is_none() {
            return Err(ArchiveError::PasswordRequired);
        }
        let reader = UnrealPakReader::new(path, password).map_err(|error| {
            if footer.encrypted {
                // 打开前已确认提供了密钥；解析加密索引失败即视为密钥错误。
                ArchiveError::WrongPassword
            } else {
                ArchiveError::Corrupt(format!("Unreal PAK：{error}"))
            }
        })?;
        Ok(Self {
            reader,
            has_password: password.is_some(),
        })
    }

    /// 返回条目数量。
    pub fn entry_count(&self) -> usize {
        self.reader.records.len()
    }

    /// 返回全部条目元数据（保持 PAK 索引顺序）。
    pub fn entries(&self) -> Vec<PakAssetEntry> {
        self.reader
            .records
            .iter()
            .map(|record| PakAssetEntry {
                path: record.path.clone(),
                size: record.uncompressed,
                compressed_size: record.compressed,
            })
            .collect()
    }

    /// 返回全部条目路径（保持 PAK 索引顺序）。
    pub fn paths(&self) -> Vec<String> {
        self.reader
            .records
            .iter()
            .map(|record| record.path.clone())
            .collect()
    }

    /// 判断是否存在指定路径的条目。
    pub fn contains(&self, path: &str) -> bool {
        self.reader.records.iter().any(|record| record.path == path)
    }

    /// 返回指定条目的解压后大小；不存在时返回 `None`。
    pub fn entry_size(&self, path: &str) -> Option<u64> {
        self.reader
            .records
            .iter()
            .find(|record| record.path == path)
            .map(|record| record.uncompressed)
    }

    /// 容器索引是否加密。
    pub fn encrypted(&self) -> bool {
        self.reader.footer.encrypted
    }

    /// 返回 PAK 索引声明的压缩方法名（去重、保持槽位顺序）。
    pub fn compression_methods(&self) -> Vec<String> {
        let mut methods: Vec<String> = Vec::new();
        for method in self.reader.footer.compression.iter().flatten() {
            if !methods.iter().any(|existing| existing == method) {
                methods.push(method.clone());
            }
        }
        methods
    }

    /// 容器是否需要 Oodle 解压后端。
    pub fn needs_oodle(&self) -> bool {
        self.reader
            .footer
            .compression
            .iter()
            .flatten()
            .any(|method| method.eq_ignore_ascii_case("Oodle"))
    }

    /// 读取并解压一个条目；条目加密但未提供密码时返回 `PasswordRequired`。
    pub fn read_entry(&mut self, path: &str) -> Result<Vec<u8>, ArchiveError> {
        let record = self
            .reader
            .records
            .iter()
            .find(|record| record.path == path)
            .ok_or_else(|| ArchiveError::InvalidRequest(format!("PAK 中不存在条目：{path}")))?;
        if record.encrypted && !self.has_password {
            return Err(ArchiveError::PasswordRequired);
        }
        // 只为小条目预分配；大条目交给 Vec 按需增长，避免伪造索引触发超大分配。
        let capacity = usize::try_from(record.uncompressed)
            .unwrap_or(0)
            .min(64 * 1024 * 1024);
        let mut output = Vec::with_capacity(capacity);
        extract_entry(
            &self.reader.path,
            &self.reader.footer,
            &self.reader.key,
            record,
            &mut output,
        )
        .map_err(|error| ArchiveError::Corrupt(format!("Unreal PAK 读取 {path} 失败：{error}")))?;
        Ok(output)
    }
}

/// 返回索引条目路径，供测试或界面需要时使用。
#[allow(dead_code)]
pub(crate) fn list_paths(path: &Path) -> Result<Vec<String>, String> {
    let footer = read_footer(path)?;
    Ok(parse_index(path, &footer, None)?
        .into_iter()
        .map(|entry| entry.path)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 `decrypt` 与直接调用 AES 块解密的结果一致。
    #[test]
    fn decrypt_matches_reference() {
        use aes::cipher::{BlockDecrypt, KeyInit};
        let key: [u8; 32] = [
            0x94, 0xd2, 0x5b, 0xc3, 0xae, 0xb4, 0x20, 0xe0, 0xbe, 0x91, 0x4e, 0xdc, 0x9d, 0x54,
            0x35, 0xa1, 0xea, 0xab, 0x5f, 0x28, 0x64, 0xe0, 0x9e, 0x94, 0x01, 0x9a, 0xc2, 0x05,
            0xb7, 0x27, 0xa7, 0xde,
        ];
        let ciphertext: [u8; 16] = [
            0x0f, 0x24, 0xa9, 0x71, 0x0e, 0xb8, 0x69, 0x86, 0xdd, 0xb6, 0x45, 0x41, 0xd8, 0x33,
            0x22, 0xde,
        ];
        let mut reference = ciphertext;
        let cipher = aes::Aes256::new_from_slice(&key).unwrap();
        cipher.decrypt_block(aes::Block::from_mut_slice(&mut reference));
        let mut actual = ciphertext;
        decrypt(&key, &mut actual).unwrap();
        assert_eq!(actual, reference);
    }

    /// 用真实样本验证首块解密结果。
    #[test]
    fn decrypt_sample_block() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/unreal/pack_v11_encrypt.pak");
        let data = std::fs::read(path).unwrap();
        let mut block = data[53..69].to_vec();
        let key: [u8; 32] = [
            0x94, 0xd2, 0x5b, 0xc3, 0xae, 0xb4, 0x20, 0xe0, 0xbe, 0x91, 0x4e, 0xdc, 0x9d, 0x54,
            0x35, 0xa1, 0xea, 0xab, 0x5f, 0x28, 0x64, 0xe0, 0x9e, 0x94, 0x01, 0x9a, 0xc2, 0x05,
            0xb7, 0x27, 0xa7, 0xde,
        ];
        decrypt(&key, &mut block).unwrap();
        assert_eq!(&block[..6], b"Proin ");
    }
}
