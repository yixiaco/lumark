//! IoStore 容器读取：文件列表、块读取和 ArchiveService 适配。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::ArchiveError;
use crate::format::{ArchiveEntry, ArchiveFormat, ArchiveInfo};

use super::super::codec::{decode_key, decompress_block, decrypt_ecb};
use super::container_header::IoContainerHeader;
use super::toc::{FileRecord, Toc, probe_header, read_toc};

/// 单个块读取上限，防止损坏索引要求分配过大内存。
const MAX_CHUNK_BYTES: u64 = 1024 * 1024 * 1024;

/// IoStore 目录索引中的一个文件。
#[derive(Debug, Clone)]
pub struct IoStoreFile {
    /// 规范化后的包路径。
    pub path: String,
    /// 未压缩逻辑大小。
    pub size: u64,
    /// 对应 TOC 条目索引。
    pub toc_index: u32,
}

/// 只读 IoStore 容器。
pub struct IoStoreContainer {
    toc_path: PathBuf,
    cas_path: PathBuf,
    toc: Toc,
    key: Option<[u8; 32]>,
    files: Vec<IoStoreFile>,
}

impl IoStoreContainer {
    /// 打开 `.utoc`（配套 `.ucas` 必须存在）；加密容器必须提供密钥。
    pub fn open(path: impl AsRef<Path>, password: Option<&str>) -> Result<Self, ArchiveError> {
        let toc_path = path.as_ref().to_path_buf();
        let cas_path = toc_path.with_extension("ucas");
        if !cas_path.is_file() {
            return Err(ArchiveError::Corrupt(format!(
                "IoStore 容器缺少配套 .ucas 文件：{}",
                cas_path.display()
            )));
        }
        let toc = read_toc(&toc_path, password)?;
        let key = if toc.header.encrypted() {
            let password = password.ok_or(ArchiveError::PasswordRequired)?;
            Some(decode_key(password).map_err(ArchiveError::Corrupt)?)
        } else {
            None
        };
        let files = build_files(&toc)?;
        Ok(Self {
            toc_path,
            cas_path,
            toc,
            key,
            files,
        })
    }

    /// 返回目录索引中的文件列表。
    pub fn files(&self) -> &[IoStoreFile] {
        &self.files
    }

    /// 返回容器是否加密。
    pub fn encrypted(&self) -> bool {
        self.toc.header.encrypted()
    }

    /// 返回压缩方法名表。
    pub fn compression_methods(&self) -> &[String] {
        &self.toc.compression_methods
    }

    /// 容器是否包含 Oodle 压缩块。
    pub fn needs_oodle(&self) -> bool {
        self.toc
            .compression_methods
            .iter()
            .any(|method| method.eq_ignore_ascii_case("oodle"))
    }

    /// 返回 TOC 文件路径。
    pub fn toc_path(&self) -> &Path {
        &self.toc_path
    }

    /// 判断目录索引中是否存在指定路径。
    pub fn contains_file(&self, path: &str) -> bool {
        self.toc.file_indices.contains_key(path)
    }

    /// 按目录索引路径读取文件。
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, ArchiveError> {
        let index = self
            .toc
            .file_indices
            .get(path)
            .copied()
            .ok_or_else(|| ArchiveError::Corrupt(format!("IoStore 中不存在条目：{path}")))?;
        self.read_chunk(index)
    }

    /// 返回 TOC 条目总数。
    pub fn entry_count(&self) -> usize {
        self.toc.chunk_ids.len()
    }

    /// 返回所有指定 chunk 类型的 TOC 索引。
    pub fn find_chunks_by_type(&self, chunk_type: u8) -> Vec<u32> {
        self.toc
            .chunk_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                ((id[11] & 0b0011_1111) == chunk_type).then_some(index as u32)
            })
            .collect()
    }

    /// 读取 IoStore ContainerHeader（chunk 类型 6）。
    pub fn container_header(&self) -> Result<IoContainerHeader, ArchiveError> {
        let mut chunks = self.find_chunks_by_type(6);
        if chunks.is_empty() {
            return Err(ArchiveError::Corrupt(
                "IoStore 容器没有 ContainerHeader chunk".to_string(),
            ));
        }
        chunks.sort_unstable();
        let container_id = self.toc.header.container_id;
        let index = chunks
            .iter()
            .copied()
            .find(|index| self.package_id(*index) == Some(container_id))
            .unwrap_or(chunks[0]);
        let data = self.read_chunk(index)?;
        IoContainerHeader::parse(&data)
    }

    /// 按包 ID 读取 StoreEntry 中的直接导入包 ID。
    pub fn imported_packages(&self, package_id: u64) -> Result<Vec<u64>, ArchiveError> {
        let header = self.container_header()?;
        Ok(header
            .package(package_id)
            .map(|package| package.imported_packages.clone())
            .unwrap_or_default())
    }

    /// 按目录索引路径返回所属包 ID。
    pub fn package_id_for_path(&self, path: &str) -> Option<u64> {
        self.toc
            .file_indices
            .get(path)
            .and_then(|index| self.package_id(*index))
    }

    /// 按包 ID 读取包的 ExportBundleData（uasset）字节。
    pub fn read_package_by_id(&self, package_id: u64) -> Result<Vec<u8>, ArchiveError> {
        self.read_package_chunk(package_id, 1, "ExportBundleData")
    }

    /// 按包 ID 读取 BulkData（ubulk）字节。
    pub fn read_bulk_by_id(&self, package_id: u64) -> Result<Vec<u8>, ArchiveError> {
        self.read_package_chunk(package_id, 2, "BulkData")
    }

    /// 按包 ID 合并指定类型的 chunk。
    fn read_package_chunk(
        &self,
        package_id: u64,
        chunk_type: u8,
        label: &str,
    ) -> Result<Vec<u8>, ArchiveError> {
        let mut chunks = self.find_package_chunks(package_id, chunk_type);
        if chunks.is_empty() {
            return Err(ArchiveError::Corrupt(format!(
                "IoStore 包 0x{package_id:016X} 没有 {label} chunk"
            )));
        }
        chunks.sort_unstable();
        let mut output = Vec::new();
        for chunk in chunks {
            output.extend_from_slice(&self.read_chunk(chunk)?);
        }
        Ok(output)
    }

    /// 读取 TOC 指定索引的块。
    pub fn read_chunk(&self, toc_index: u32) -> Result<Vec<u8>, ArchiveError> {
        let offset = self
            .toc
            .offsets
            .get(toc_index as usize)
            .copied()
            .ok_or_else(|| ArchiveError::Corrupt("IoStore 条目索引越界".to_string()))?;
        let size = offset.length;
        if size == 0 {
            return Ok(Vec::new());
        }
        if size > MAX_CHUNK_BYTES {
            return Err(ArchiveError::Corrupt(
                "IoStore 单块超过 1 GiB 安全上限".to_string(),
            ));
        }
        let block_size = u64::from(self.toc.header.compression_block_size);
        if block_size == 0 {
            return Err(ArchiveError::Corrupt("IoStore 压缩块大小为 0".to_string()));
        }
        if !offset.offset.is_multiple_of(block_size) {
            return Err(ArchiveError::Corrupt(
                "IoStore 块偏移未按压缩块大小对齐".to_string(),
            ));
        }
        let first = (offset.offset / block_size) as usize;
        let last = ((offset
            .offset
            .checked_add(size)
            .ok_or_else(|| ArchiveError::Corrupt("IoStore 块范围溢出".to_string()))?
            - 1)
            / block_size) as usize;
        let blocks = self
            .toc
            .blocks
            .get(first..=last)
            .ok_or_else(|| ArchiveError::Corrupt("IoStore 压缩块索引越界".to_string()))?;
        let mut file = File::open(&self.cas_path)?;
        let cas_length = file.metadata()?.len();
        let mut output = Vec::new();
        for block in blocks {
            if block.uncompressed_size == 0 || u64::from(block.uncompressed_size) > block_size {
                return Err(ArchiveError::Corrupt("IoStore 块解压大小异常".to_string()));
            }
            file.seek(SeekFrom::Start(block.offset))?;
            let method = if block.method_index == 0 {
                None
            } else {
                Some(
                    self.toc
                        .compression_methods
                        .get(block.method_index as usize - 1)
                        .ok_or_else(|| {
                            ArchiveError::Corrupt("IoStore 压缩方法索引越界".to_string())
                        })?
                        .as_str(),
                )
            };
            match (method, &self.key) {
                (None, Some(key)) => {
                    let expected = block.uncompressed_size as usize;
                    let padded = align(expected, 16);
                    ensure_range(block.offset, padded as u64, cas_length)?;
                    let mut raw = vec![0u8; padded];
                    file.read_exact(&mut raw)?;
                    decrypt_ecb(key, &mut raw).map_err(ArchiveError::Corrupt)?;
                    raw.truncate(expected);
                    output.extend_from_slice(&raw);
                }
                (None, None) => {
                    let expected = block.uncompressed_size as usize;
                    ensure_range(block.offset, expected as u64, cas_length)?;
                    let mut raw = vec![0u8; expected];
                    file.read_exact(&mut raw)?;
                    output.extend_from_slice(&raw);
                }
                (Some(method), key) => {
                    let expected = block.uncompressed_size as usize;
                    let compressed = block.compressed_size as usize;
                    let read_length = if key.is_some() {
                        align(compressed, 16)
                    } else {
                        compressed
                    };
                    ensure_range(block.offset, read_length as u64, cas_length)?;
                    let mut raw = vec![0u8; read_length];
                    file.read_exact(&mut raw)?;
                    if let Some(key) = key {
                        decrypt_ecb(key, &mut raw).map_err(ArchiveError::Corrupt)?;
                        raw.truncate(compressed);
                    }
                    let mut decoded = vec![0u8; expected];
                    decompress_block(method, &raw, &mut decoded).map_err(ArchiveError::Corrupt)?;
                    output.extend_from_slice(&decoded);
                }
            }
        }
        let skip = (offset.offset - first as u64 * block_size) as usize;
        let end = skip
            .checked_add(size as usize)
            .ok_or_else(|| ArchiveError::Corrupt("IoStore 条目长度溢出".to_string()))?;
        if end > output.len() {
            return Err(ArchiveError::Corrupt(
                "IoStore 块数据短于条目声明长度".to_string(),
            ));
        }
        Ok(output[skip..end].to_vec())
    }

    /// 返回 TOC 条目原始 12 字节 ID。
    pub fn chunk_id(&self, toc_index: u32) -> Option<[u8; 12]> {
        self.toc.chunk_ids.get(toc_index as usize).copied()
    }

    /// 返回 TOC 条目类型（低 6 位，兼容版本位）。
    pub fn chunk_type(&self, toc_index: u32) -> Option<u8> {
        self.chunk_id(toc_index).map(|id| id[11] & 0b0011_1111)
    }

    /// 返回 TOC 条目对应的包 ID。
    pub fn package_id(&self, toc_index: u32) -> Option<u64> {
        self.chunk_id(toc_index)
            .map(|id| u64::from_le_bytes([id[0], id[1], id[2], id[3], id[4], id[5], id[6], id[7]]))
    }

    /// 返回指定包内指定类型的全部 TOC 索引。
    pub fn find_package_chunks(&self, package_id: u64, chunk_type: u8) -> Vec<u32> {
        self.toc
            .chunk_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let id_package =
                    u64::from_le_bytes([id[0], id[1], id[2], id[3], id[4], id[5], id[6], id[7]]);
                ((id[11] & 0b0011_1111) == chunk_type && id_package == package_id)
                    .then_some(index as u32)
            })
            .collect()
    }

    /// 返回条目使用的压缩方法描述。
    pub(crate) fn chunk_method(&self, toc_index: u32) -> String {
        let Some(blocks) = self.chunk_blocks(toc_index) else {
            return "IoStore".to_string();
        };
        let mut names = Vec::new();
        for block in blocks {
            let name = if block.method_index == 0 {
                "Store"
            } else {
                self.toc
                    .compression_methods
                    .get(block.method_index as usize - 1)
                    .map(String::as_str)
                    .unwrap_or("Unknown")
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        if names.is_empty() {
            "Store".to_string()
        } else {
            names.join("+")
        }
    }

    /// 返回条目压缩后大小（块大小之和）。
    pub(crate) fn chunk_compressed_size(&self, toc_index: u32) -> Option<u64> {
        self.chunk_blocks(toc_index).map(|blocks| {
            blocks
                .iter()
                .map(|block| u64::from(block.compressed_size))
                .sum()
        })
    }

    /// 返回条目覆盖的压缩块。
    fn chunk_blocks(&self, toc_index: u32) -> Option<&[super::toc::CompressionBlock]> {
        let offset = self.toc.offsets.get(toc_index as usize)?;
        if offset.length == 0 {
            return Some(&[]);
        }
        let block_size = u64::from(self.toc.header.compression_block_size);
        if block_size == 0 || !offset.offset.is_multiple_of(block_size) {
            return None;
        }
        let first = (offset.offset / block_size) as usize;
        let last = ((offset.offset.checked_add(offset.length)? - 1) / block_size) as usize;
        self.toc.blocks.get(first..=last)
    }
}

/// 构建按路径排序的文件列表，并校验 TOC 索引。
fn build_files(toc: &Toc) -> Result<Vec<IoStoreFile>, ArchiveError> {
    let mut files = Vec::with_capacity(toc.files.len());
    for FileRecord { path, toc_index } in &toc.files {
        let offset = toc.offsets.get(*toc_index as usize).ok_or_else(|| {
            ArchiveError::Corrupt("IoStore 文件指向不存在的 TOC 条目".to_string())
        })?;
        files.push(IoStoreFile {
            path: path.clone(),
            size: offset.length,
            toc_index: *toc_index,
        });
    }
    Ok(files)
}

/// 向上取整到 16 字节；加密块按 AES 分组长度读取。
fn align(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

/// 校验物理读取范围位于 `.ucas` 内。
fn ensure_range(offset: u64, length: u64, file_length: u64) -> Result<(), ArchiveError> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > file_length)
    {
        return Err(ArchiveError::Corrupt(
            "IoStore 块读取范围超出 .ucas".to_string(),
        ));
    }
    Ok(())
}

/// 探测 IoStore 容器是否加密；目录索引同样加密，因此两个标志一致。
pub(crate) fn probe_encryption(path: &Path) -> Result<(bool, bool), ArchiveError> {
    let header = probe_header(path)?;
    let encrypted = header.encrypted();
    Ok((encrypted, encrypted))
}

/// 用给定密钥验证 IoStore 容器。
pub(crate) fn verify_password(path: &Path, password: &str) -> Result<bool, ArchiveError> {
    match IoStoreContainer::open(path, Some(password)) {
        Ok(_) => Ok(true),
        Err(ArchiveError::WrongPassword | ArchiveError::Corrupt(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

/// 列出 IoStore 容器中的包文件。
pub(crate) fn list(path: &Path, password: Option<&str>) -> Result<ArchiveInfo, ArchiveError> {
    let container = IoStoreContainer::open(path, password)?;
    let mut info = ArchiveInfo {
        format: Some(ArchiveFormat::IoStore),
        encrypted: container.encrypted(),
        header_encrypted: container.encrypted(),
        ..Default::default()
    };
    for (index, file) in container.files().iter().enumerate() {
        let compressed_size = container.chunk_compressed_size(file.toc_index);
        info.entries.push(ArchiveEntry {
            id: index as u64,
            path: file.path.clone(),
            is_dir: false,
            size: file.size,
            compressed_size,
            modified: None,
            crc32: None,
            method: container.chunk_method(file.toc_index),
            encrypted: container.encrypted(),
        });
        info.total_size = info.total_size.saturating_add(file.size);
        info.compressed_size = info
            .compressed_size
            .saturating_add(compressed_size.unwrap_or(0));
    }
    Ok(info)
}
