//! 基于 LGPL `wim-format` 的 WIM/ESD/SWM 只读适配器。
//!
//! `wim-format` 负责解析容器、查找表、元数据和 XPRESS/LZX/LZMS 解码；本模块
//! 负责 Lumark 侧的多分卷发现、目录树展开、资源预算和按范围读取。WIM 内容
//! 不整体读入内存，文件按解码块范围流式写入。
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use lumark_legacy::archive::{ArchiveReader, Entry};
use lumark_legacy::detect::ArchiveFormat as GeeFormat;
use lumark_legacy::error::{GeeZipError, GeeZipResult};

use sha1::{Digest, Sha1};
use wim_format::file_archive::{FileArchive, read_header};
use wim_format::metadata::{Metadata, StreamType};

/// 单次从 WIM 读取并写入的块大小。
const WIM_READ_CHUNK: u64 = 4 * 1024 * 1024;

/// 一个文件在 WIM 中的资源位置。
#[derive(Debug, Clone, Copy)]
struct WimResource {
    /// 内容 SHA-1，用于在分卷中查找资源。
    hash: [u8; 20],
    /// 未压缩大小。
    size: u64,
}

/// WIM/SWM 读取器；每个分卷保留独立文件句柄和查找表。
pub(crate) struct WimArchiveReader {
    /// 按分卷号排序的已打开分卷。
    parts: Vec<FileArchive<File>>,
    /// 已展开的目录条目。
    entries: Vec<Entry>,
    /// 路径到内容资源的映射。
    resources: HashMap<String, WimResource>,
}

impl WimArchiveReader {
    /// 打开 WIM 或 SWM；遇到分卷时自动发现同目录的其余分卷。
    pub(crate) fn open(path: &Path) -> GeeZipResult<Self> {
        let parts = open_parts(path)?;
        let first = parts.first().ok_or_else(|| corrupt("WIM 没有可用分卷"))?;
        let metadata_bytes = first
            .read_metadata(1)
            .map_err(|error| corrupt(format!("读取 WIM 元数据失败：{error}")))?;
        let metadata = Metadata::parse(&metadata_bytes)
            .map_err(|error| corrupt(format!("解析 WIM 元数据失败：{error}")))?;
        let mut entries = Vec::with_capacity(metadata.nodes.len());
        let mut resources = HashMap::new();
        for child in metadata
            .nodes
            .first()
            .map_or(&[][..], |node| &node.children)
        {
            collect_node(&metadata, *child, "", &parts, &mut entries, &mut resources)?;
        }
        Ok(Self {
            parts,
            entries,
            resources,
        })
    }

    /// 按路径查找文件资源，并按范围读取。
    fn read_range(&self, resource: WimResource, start: u64, end: u64) -> GeeZipResult<Vec<u8>> {
        for part in &self.parts {
            if part.lookup.find(&resource.hash).is_some() {
                return part
                    .read_blob_range(&resource.hash, start..end)
                    .map_err(|error| corrupt(format!("读取 WIM 数据失败：{error}")));
            }
        }
        Err(corrupt(format!(
            "WIM 分卷中找不到资源 {:02x?}",
            resource.hash
        )))
    }
}

impl ArchiveReader for WimArchiveReader {
    /// 返回 WIM 格式标识。
    fn format(&self) -> GeeFormat {
        GeeFormat::Wim
    }

    /// 返回 WIM 中的文件和目录列表。
    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        Ok(self.entries.clone())
    }

    /// 解压单个 WIM 条目；流式读取并按 SHA-1 校验完整内容。
    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        if entry.is_dir {
            return Ok(0);
        }
        let Some(resource) = self.resources.get(&entry.path).copied() else {
            return Ok(0);
        };
        if resource.size == 0 {
            return Ok(0);
        }
        let mut hasher = Sha1::new();
        let mut offset = 0u64;
        while offset < resource.size {
            let end = resource.size.min(offset.saturating_add(WIM_READ_CHUNK));
            let bytes = self.read_range(resource, offset, end)?;
            hasher.update(&bytes);
            writer
                .write_all(&bytes)
                .map_err(|error| corrupt(format!("写入 WIM 条目失败：{error}")))?;
            offset = end;
        }
        let actual: [u8; 20] = hasher.finalize().into();
        if actual != resource.hash {
            return Err(corrupt(format!("WIM 条目校验失败：{}", entry.path)));
        }
        Ok(resource.size)
    }
}

/// 展开一个元数据节点及其子树。
fn collect_node(
    metadata: &Metadata<'_>,
    index: usize,
    parent: &str,
    parts: &[FileArchive<File>],
    entries: &mut Vec<Entry>,
    resources: &mut HashMap<String, WimResource>,
) -> GeeZipResult<()> {
    let node = metadata
        .nodes
        .get(index)
        .ok_or_else(|| corrupt("WIM 元数据节点越界"))?;
    let name = decode_name(node.entry.name)?;
    if name.is_empty() {
        return Ok(());
    }
    let path = if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    };
    let inode = metadata
        .inode_entry(index)
        .ok_or_else(|| corrupt("WIM 元数据 inode 越界"))?;
    if inode.reparse_fields().is_some() {
        return Ok(());
    }
    let is_dir = inode.is_directory();
    let modified = filetime_to_unix(inode.last_write_time);
    let hash = inode
        .streams
        .iter()
        .find(|stream| stream.kind == StreamType::Data && stream.name.is_empty())
        .map_or([0u8; 20], |stream| stream.hash);
    let resource = if !is_dir && hash != [0u8; 20] {
        Some(WimResource {
            hash,
            size: find_size(parts, &hash).unwrap_or(0),
        })
    } else {
        None
    };
    entries.push(Entry {
        path: path.clone(),
        size: resource.map_or(0, |value| value.size),
        compressed_size: 0,
        crc32: None,
        modified,
        is_dir,
    });
    if let Some(resource) = resource {
        resources.insert(path.clone(), resource);
    }
    for child in &node.children {
        collect_node(metadata, *child, &path, parts, entries, resources)?;
    }
    Ok(())
}

/// 在所有分卷中查找资源大小。
fn find_size(parts: &[FileArchive<File>], hash: &[u8; 20]) -> Option<u64> {
    parts
        .iter()
        .find_map(|part| part.lookup.find(hash).map(|blob| blob.size))
}

/// 打开单个 WIM 或完整 SWM 分卷集合。
fn open_parts(path: &Path) -> GeeZipResult<Vec<FileArchive<File>>> {
    let first = open_archive(path)?;
    let header = first.header.clone();
    if header.total_parts <= 1 {
        return Ok(vec![first]);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut parts = vec![first];
    let mut candidates = directory_candidates(parent, path)?;
    candidates.retain(|candidate| candidate != path);
    for candidate in candidates {
        let archive = match open_archive(&candidate) {
            Ok(archive) => archive,
            Err(_) => continue,
        };
        if archive.header.guid == header.guid
            && archive.header.total_parts == header.total_parts
            && archive.header.part_number > 1
        {
            parts.push(archive);
        }
    }
    parts.sort_by_key(|part| part.header.part_number);
    parts.dedup_by_key(|part| part.header.part_number);
    if parts.len() != header.total_parts as usize {
        return Err(corrupt(format!(
            "SWM 分卷不完整：需要 {} 个，找到 {} 个",
            header.total_parts,
            parts.len()
        )));
    }
    for (index, part) in parts.iter().enumerate() {
        if part.header.part_number as usize != index + 1 {
            return Err(corrupt("SWM 分卷编号不连续"));
        }
    }
    Ok(parts)
}

/// 打开单个 WIM 分卷，并兼容未压缩归档使用非零 chunk size 的写法。
fn open_archive(path: &Path) -> GeeZipResult<FileArchive<File>> {
    let mut file = File::open(path)?;
    let mut header =
        read_header(&mut file).map_err(|error| corrupt(format!("读取 WIM 头失败：{error}")))?;
    if header.flags & 2 == 0 && header.validate_compression().is_err() {
        header.chunk_size = 0;
    }
    FileArchive::open_with_header(file, header)
        .map_err(|error| corrupt(format!("打开 WIM 失败：{error}")))
}

/// 收集同目录中可能是分卷的路径，避免扫描无关文件。
fn directory_candidates(parent: &Path, path: &Path) -> GeeZipResult<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let candidate = entry.path();
        let extension = candidate
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if candidate != path && extension.eq_ignore_ascii_case("swm") {
            candidates.push(candidate);
        }
    }
    candidates.sort();
    Ok(candidates)
}

/// 把 UTF-16LE 的 WIM 名称转换为字符串。
fn decode_name(bytes: &[u8]) -> GeeZipResult<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16(&units)
        .map_err(|error| corrupt(format!("WIM 文件名不是有效 UTF-16：{error}")))
}

/// 把 Windows FILETIME 转换为 Unix 秒。
fn filetime_to_unix(value: u64) -> Option<u64> {
    const EPOCH_DIFF: u64 = 116_444_736_000_000_000;
    if value <= EPOCH_DIFF {
        None
    } else {
        Some((value - EPOCH_DIFF) / 10_000_000)
    }
}

/// 构造带 WIM 格式标识的读取错误。
fn corrupt(message: impl Into<String>) -> GeeZipError {
    GeeZipError::format(message.into(), GeeFormat::Wim)
}
