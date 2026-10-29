//! PE 可执行文件的只读归档视图。
//!
//! 每个 PE 节作为文件夹，文件夹内的 `[0]` 是该节的原始字节；资源节
//! 解析为资源目录，图标资源重建为 `.ico`，其余资源保留原始字节。文件尾部
//! 的覆盖数据作为根目录的 `[0]`，便于像压缩包一样浏览安装器载荷。

use super::*;
use crate::format::OverwritePolicy;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};

/// 资源路径：类型、名称和语言层级。
type ResourcePath = Vec<(u32, Option<String>)>;
/// 一个资源叶子的路径和数据范围。
type ResourceRecord = (ResourcePath, u32, u32);

/// PE 节头。
#[derive(Debug, Clone)]
struct PeSection {
    /// 节名。
    name: String,
    /// 虚拟地址。
    virtual_address: u32,
    /// 虚拟大小。
    virtual_size: u32,
    /// 文件原始偏移。
    raw_offset: u32,
    /// 文件原始大小。
    raw_size: u32,
}

/// 归档条目的数据来源。
#[derive(Debug, Clone)]
enum PeData {
    /// 从原始文件按偏移和长度读取。
    Range {
        /// 文件偏移。
        offset: u64,
        /// 字节数。
        size: u64,
    },
    /// 已经构造好的字节，例如重建的 ICO/BMP。
    Inline(Vec<u8>),
}

impl PeData {
    /// 返回数据字节数。
    fn size(&self) -> u64 {
        match self {
            Self::Range { size, .. } => *size,
            Self::Inline(bytes) => bytes.len() as u64,
        }
    }

    /// 读取完整数据；范围数据会校验实际长度。
    fn read(&self, path: &Path) -> Result<Vec<u8>, ArchiveError> {
        match self {
            Self::Range { offset, size } => read_file_range(path, *offset, *size),
            Self::Inline(bytes) => Ok(bytes.clone()),
        }
    }
}

/// 一个 PE 归档条目。
#[derive(Debug, Clone)]
struct PeEntry {
    /// 归档内路径。
    path: String,
    /// 是否目录。
    is_dir: bool,
    /// 数据来源；目录为 `None`。
    data: Option<PeData>,
}

impl PeEntry {
    /// 返回条目字节数。
    fn size(&self) -> u64 {
        self.data.as_ref().map(PeData::size).unwrap_or(0)
    }
}

/// 一个资源叶子。
#[derive(Debug, Clone)]
struct ResourceItem {
    /// 资源类型 ID。
    type_id: u32,
    /// 资源类型名称；`None` 表示使用 ID。
    type_name: Option<String>,
    /// 资源名称 ID。
    name_id: u32,
    /// 资源名称；`None` 表示使用 ID。
    name: Option<String>,
    /// 数据来源。
    data: PeData,
}

/// 已解析的 PE 归档。
struct PeArchive {
    /// 文件大小。
    file_size: u64,
    /// 节表。
    sections: Vec<PeSection>,
    /// 覆盖数据起点和大小。
    overlay: Option<(u64, u64)>,
    /// 证书表偏移和大小。
    certificate: Option<(u64, u64)>,
    /// 资源条目。
    resources: Vec<PeEntry>,
    /// 资源节名。
    resource_section: Option<String>,
}

impl PeArchive {
    /// 解析 PE 头、节表、资源和覆盖数据。
    fn open(path: &Path) -> Result<Self, ArchiveError> {
        let file_size = fs::metadata(path)?.len();
        let mut file = File::open(path)?;
        let mut header = vec![0u8; 64 * 1024];
        let read = file.read(&mut header)?;
        let header = &header[..read];
        if header.len() < 0x40 || &header[..2] != b"MZ" {
            return Err(ArchiveError::Corrupt("不是 PE 文件".to_string()));
        }
        let pe_offset = le_u32(header, 0x3c)? as usize;
        if pe_offset
            .checked_add(24)
            .is_none_or(|end| end > header.len())
            || &header[pe_offset..pe_offset + 4] != b"PE\0\0"
        {
            return Err(ArchiveError::Corrupt("PE 头不完整".to_string()));
        }
        let section_count = le_u16(header, pe_offset + 6)? as usize;
        let optional_size = le_u16(header, pe_offset + 20)? as usize;
        let optional = pe_offset + 24;
        let optional_end = optional
            .checked_add(optional_size)
            .ok_or_else(|| ArchiveError::Corrupt("PE 可选头长度溢出".to_string()))?;
        if optional_end > header.len() || optional_size < 2 {
            return Err(ArchiveError::Corrupt("PE 可选头不完整".to_string()));
        }
        let magic = le_u16(header, optional)?;
        let is_64 = match magic {
            0x20b => true,
            0x10b => false,
            _ => return Err(ArchiveError::Corrupt("未知 PE 可选头".to_string())),
        };
        let dirs_offset = optional + if is_64 { 112 } else { 96 };
        let dirs_count_offset = optional + if is_64 { 108 } else { 92 };
        let dirs_count = le_u32(header, dirs_count_offset)? as usize;
        let mut resource = None;
        let mut certificate = None;
        if dirs_count > 2 && dirs_offset + 24 <= optional_end {
            let rva = le_u32(header, dirs_offset + 16)?;
            let size = le_u32(header, dirs_offset + 20)?;
            if rva != 0 && size != 0 {
                resource = Some((rva, size));
            }
        }
        if dirs_count > 4 && dirs_offset + 40 <= optional_end {
            let offset = le_u32(header, dirs_offset + 32)?;
            let size = le_u32(header, dirs_offset + 36)?;
            if offset != 0 && size != 0 {
                certificate = Some((u64::from(offset), u64::from(size)));
            }
        }
        let section_table = optional_end;
        if section_table
            .checked_add(section_count.saturating_mul(40))
            .is_none_or(|end| end > header.len())
        {
            return Err(ArchiveError::Corrupt("PE 节表不完整".to_string()));
        }
        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let base = section_table + index * 40;
            let raw_name = &header[base..base + 8];
            let name = String::from_utf8_lossy(raw_name)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            let name = if name.is_empty() {
                format!("SECTION_{index}")
            } else {
                name
            };
            sections.push(PeSection {
                name,
                virtual_size: le_u32(header, base + 8)?,
                virtual_address: le_u32(header, base + 12)?,
                raw_size: le_u32(header, base + 16)?,
                raw_offset: le_u32(header, base + 20)?,
            });
        }
        let headers_end = section_table + section_count.saturating_mul(40);
        let sections_end = sections
            .iter()
            .map(|section| u64::from(section.raw_offset) + u64::from(section.raw_size))
            .max()
            .unwrap_or(0);
        let overlay_start = u64::try_from(headers_end)
            .unwrap_or(u64::MAX)
            .max(sections_end);
        let overlay =
            (overlay_start < file_size).then_some((overlay_start, file_size - overlay_start));
        let (resource_section, resources) = match resource {
            Some((rva, _)) => match parse_resources(path, &sections, rva, file_size) {
                Ok((section, entries)) => (section, entries),
                Err(_) => (None, Vec::new()),
            },
            None => (None, Vec::new()),
        };
        Ok(Self {
            file_size,
            sections,
            overlay,
            certificate,
            resources,
            resource_section,
        })
    }

    /// 生成节、资源和覆盖数据条目。
    fn entries(&self) -> Vec<PeEntry> {
        let mut entries = Vec::new();
        let mut used = HashSet::new();
        for section in &self.sections {
            let folder = unique_name(sanitize_component(&section.name), &mut used);
            entries.push(PeEntry {
                path: folder.clone(),
                is_dir: true,
                data: None,
            });
            if self.resource_section.as_deref() == Some(section.name.as_str())
                && !self.resources.is_empty()
            {
                entries.extend(self.resources.iter().cloned());
                continue;
            }
            let data = PeData::Range {
                offset: u64::from(section.raw_offset),
                size: u64::from(section.raw_size),
            };
            let path = unique_name(format!("{folder}/[0]"), &mut used);
            entries.push(PeEntry {
                path,
                is_dir: false,
                data: Some(data),
            });
        }
        if let Some((offset, size)) = self.overlay {
            let path = unique_name("[0]".to_string(), &mut used);
            entries.push(PeEntry {
                path,
                is_dir: false,
                data: Some(PeData::Range { offset, size }),
            });
        }
        if let Some((offset, size)) = self.certificate {
            let path = unique_name("CERTIFICATE.bin".to_string(), &mut used);
            entries.push(PeEntry {
                path,
                is_dir: false,
                data: Some(PeData::Range { offset, size }),
            });
        }
        entries
    }
}

impl ArchiveService {
    /// 列出 PE 的节、资源和覆盖数据。
    pub(crate) fn list_pe(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let archive = PeArchive::open(path)?;
        let entries = archive.entries();
        let mut folder_sizes = HashMap::new();
        for entry in &entries {
            if entry.is_dir {
                continue;
            }
            let mut parent = entry.path.as_str();
            while let Some((directory, _)) = parent.rsplit_once('/') {
                *folder_sizes.entry(directory.to_string()).or_default() += entry.size();
                parent = directory;
            }
        }
        let entries = entries
            .into_iter()
            .enumerate()
            .map(|(id, entry)| {
                let size = if entry.is_dir {
                    folder_sizes.get(&entry.path).copied().unwrap_or(0)
                } else {
                    entry.size()
                };
                ArchiveEntry {
                    id: id as u64,
                    path: entry.path,
                    is_dir: entry.is_dir,
                    size,
                    compressed_size: (!entry.is_dir).then_some(size),
                    modified: None,
                    crc32: None,
                    method: "PE".to_string(),
                    encrypted: false,
                }
            })
            .collect();
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Pe),
            id: 0,
            entries,
            encrypted: false,
            header_encrypted: false,
            solid: false,
            comment: None,
            total_size: archive.file_size,
            compressed_size: archive.file_size,
        })
    }

    /// 解压 PE 的选中节和资源。
    pub(crate) fn extract_pe<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let archive = PeArchive::open(path)?;
        let entries = archive.entries();
        let selected_entries = entries
            .iter()
            .filter(|entry| !entry.is_dir && selected(&entry.path, &options.selected_paths))
            .collect::<Vec<_>>();
        let bytes_total = selected_entries
            .iter()
            .map(|entry| entry.size())
            .sum::<u64>();
        let entries_total = selected_entries.len();
        fs::create_dir_all(&options.output_dir)?;
        let mut input = File::open(path)?;
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 PE".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in selected_entries {
            ensure_not_cancelled(&options.cancel)?;
            let mut destination = safe_join(&options.output_dir, &entry.path)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            match options.overwrite_policy {
                OverwritePolicy::Rename => destination = unique_destination(&destination),
                OverwritePolicy::Skip => {
                    report.skipped += 1;
                    continue;
                }
                OverwritePolicy::Ask if !options.overwrite => {
                    report.skipped += 1;
                    continue;
                }
                _ => {}
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let size = entry.size();
            ensure_budget(options, &report, size)?;
            let Some(data) = entry.data.as_ref() else {
                continue;
            };
            let mut output = File::create(&destination)?;
            let written = match data {
                PeData::Inline(bytes) => {
                    output.write_all(bytes)?;
                    bytes.len() as u64
                }
                PeData::Range { offset, size } => {
                    input.seek(SeekFrom::Start(*offset))?;
                    let mut reader = Read::by_ref(&mut input).take(*size);
                    copy_cancellable_limited(&mut reader, &mut output, &options.cancel, *size)?
                }
            };
            if written != size {
                return Err(ArchiveError::Corrupt(format!(
                    "PE 条目 {} 数据不完整：期望 {size} 字节，实际 {written} 字节",
                    entry.path
                )));
            }
            report.files += 1;
            report.bytes = report.bytes.saturating_add(written);
            progress(ExtractProgress {
                current: entry.path.clone(),
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total,
            });
        }
        Ok(report)
    }

    /// 读取全部 PE 条目，验证节和资源范围。
    pub(crate) fn test_pe<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let archive = PeArchive::open(path)?;
        let entries = archive.entries();
        let files = entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .collect::<Vec<_>>();
        let bytes_total = files.iter().map(|entry| entry.size()).sum::<u64>();
        let mut report = ExtractReport::default();
        let mut input = File::open(path)?;
        for entry in &files {
            let size = entry.size();
            match entry.data.as_ref() {
                Some(PeData::Range { offset, size }) => {
                    input.seek(SeekFrom::Start(*offset))?;
                    let mut reader = Read::by_ref(&mut input).take(*size);
                    let copied = io::copy(&mut reader, &mut io::sink())?;
                    if copied != *size {
                        return Err(ArchiveError::Corrupt(format!(
                            "PE 条目 {} 数据不完整",
                            entry.path
                        )));
                    }
                }
                Some(PeData::Inline(bytes)) if bytes.len() as u64 != size => {
                    return Err(ArchiveError::Corrupt(format!(
                        "PE 条目 {} 数据不完整",
                        entry.path
                    )));
                }
                Some(PeData::Inline(_)) | None => {}
            }
            report.files += 1;
            report.bytes = report.bytes.saturating_add(size);
            progress(ExtractProgress {
                current: entry.path.clone(),
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total: files.len(),
            });
        }
        Ok(report)
    }
}

/// 解析资源节并生成资源文件夹和叶子。
fn parse_resources(
    path: &Path,
    sections: &[PeSection],
    resource_rva: u32,
    file_size: u64,
) -> Result<(Option<String>, Vec<PeEntry>), ArchiveError> {
    let section = sections
        .iter()
        .find(|section| {
            let span = section.virtual_size.max(section.raw_size);
            resource_rva >= section.virtual_address
                && resource_rva < section.virtual_address.saturating_add(span)
        })
        .ok_or_else(|| ArchiveError::Corrupt("找不到资源节".to_string()))?;
    if section.raw_size == 0 || section.raw_size > 64 * 1024 * 1024 {
        return Err(ArchiveError::Corrupt("资源节大小异常".to_string()));
    }
    let bytes = read_file_range(
        path,
        u64::from(section.raw_offset),
        u64::from(section.raw_size),
    )?;
    let base = resource_rva.saturating_sub(section.virtual_address) as usize;
    if base >= bytes.len() {
        return Err(ArchiveError::Corrupt("资源目录偏移越界".to_string()));
    }
    let directory = parse_resource_directory(&bytes, base, base, &mut Vec::new())?;
    let mut items = Vec::new();
    for (path, data_rva, size) in directory {
        let Some(offset) = rva_to_offset(sections, data_rva) else {
            continue;
        };
        if offset.saturating_add(u64::from(size)) > file_size {
            continue;
        }
        let (type_id, type_name) = path.first().cloned().unwrap_or((0, None));
        let (name_id, name) = path.get(1).cloned().unwrap_or((0, None));
        items.push(ResourceItem {
            type_id,
            type_name,
            name_id,
            name,
            data: PeData::Range {
                offset,
                size: u64::from(size),
            },
        });
    }
    build_resource_entries(&section.name, items, path)
}

/// 递归解析资源目录，返回每组资源的类型/名称/语言路径和数据范围。
///
/// `resource_base` 是资源目录在节字节中的起点；目录项中的字符串和子目录
/// 偏移都相对于该起点，而不是节起点。
fn parse_resource_directory(
    bytes: &[u8],
    resource_base: usize,
    directory_offset: usize,
    path: &mut ResourcePath,
) -> Result<Vec<ResourceRecord>, ArchiveError> {
    if directory_offset.saturating_add(16) > bytes.len() {
        return Err(ArchiveError::Corrupt("资源目录不完整".to_string()));
    }
    let named = le_u16(bytes, directory_offset + 12)? as usize;
    let ids = le_u16(bytes, directory_offset + 14)? as usize;
    let count = named.saturating_add(ids);
    if count > 100_000 {
        return Err(ArchiveError::Corrupt("资源目录项过多".to_string()));
    }
    let entries_start = directory_offset + 16;
    let entries_end = entries_start.saturating_add(count.saturating_mul(8));
    if entries_end > bytes.len() {
        return Err(ArchiveError::Corrupt("资源目录项越界".to_string()));
    }
    let mut result = Vec::new();
    for index in 0..count {
        let base = entries_start + index * 8;
        let name_field = le_u32(bytes, base)?;
        let offset_field = le_u32(bytes, base + 4)?;
        let (id, name) = resource_name(bytes, resource_base, name_field)?;
        path.push((id, name));
        if offset_field & 0x8000_0000 != 0 {
            let child = resource_base + (offset_field & 0x7fff_ffff) as usize;
            if path.len() < 4 {
                result.extend(parse_resource_directory(bytes, resource_base, child, path)?);
            }
        } else {
            let data_offset = resource_base + (offset_field & 0x7fff_ffff) as usize;
            if data_offset.saturating_add(16) <= bytes.len() {
                let data_rva = le_u32(bytes, data_offset)?;
                let size = le_u32(bytes, data_offset + 4)?;
                result.push((path.clone(), data_rva, size));
            }
        }
        path.pop();
    }
    Ok(result)
}

/// 读取资源目录项的名称字段；高比特表示 UTF-16 字符串偏移。
fn resource_name(
    bytes: &[u8],
    resource_base: usize,
    field: u32,
) -> Result<(u32, Option<String>), ArchiveError> {
    if field & 0x8000_0000 == 0 {
        return Ok((field, None));
    }
    let offset = resource_base + (field & 0x7fff_ffff) as usize;
    if offset.saturating_add(2) > bytes.len() {
        return Err(ArchiveError::Corrupt("资源名称越界".to_string()));
    }
    let length = le_u16(bytes, offset)? as usize;
    let start = offset + 2;
    let end = start.saturating_add(length.saturating_mul(2));
    if end > bytes.len() {
        return Err(ArchiveError::Corrupt("资源名称长度越界".to_string()));
    }
    let units = bytes[start..end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect::<Vec<_>>();
    Ok((0, Some(String::from_utf16_lossy(&units))))
}

/// 把资源项转换成归档条目，并重建图标组和位图。
fn build_resource_entries(
    section_name: &str,
    items: Vec<ResourceItem>,
    path: &Path,
) -> Result<(Option<String>, Vec<PeEntry>), ArchiveError> {
    let section_name = sanitize_component(section_name);
    let mut entries = Vec::new();
    let mut used = HashSet::new();
    let mut leaves = Vec::new();
    for item in items.iter().filter(|item| item.type_id == 3) {
        let Ok(icon) = item.data.read(path) else {
            continue;
        };
        let Some(bytes) = build_single_icon(&icon) else {
            continue;
        };
        leaves.push(ResourceLeaf {
            folder: "ICON".to_string(),
            name: format!("{}.ico", resource_name_text(item)),
            data: PeData::Inline(bytes),
        });
    }
    for item in &items {
        match item.type_id {
            3 | 14 => {}
            2 => {
                let Ok(bitmap) = item.data.read(path) else {
                    continue;
                };
                let Some(bytes) = build_bitmap(&bitmap) else {
                    continue;
                };
                leaves.push(ResourceLeaf {
                    folder: "BITMAP".to_string(),
                    name: format!("{}.bmp", resource_name_text(item)),
                    data: PeData::Inline(bytes),
                });
            }
            24 => {
                leaves.push(ResourceLeaf {
                    folder: "MANIFEST".to_string(),
                    name: format!("{}.manifest", resource_name_text(item)),
                    data: item.data.clone(),
                });
            }
            _ => {
                let extension = resource_extension(item.type_id);
                leaves.push(ResourceLeaf {
                    folder: resource_type_name(item),
                    name: format!("{}{extension}", resource_name_text(item)),
                    data: item.data.clone(),
                });
            }
        }
    }
    leaves.sort_by(|left, right| {
        left.folder
            .cmp(&right.folder)
            .then(left.name.cmp(&right.name))
    });
    let mut folders = HashSet::new();
    for leaf in leaves {
        if folders.insert(leaf.folder.clone()) {
            entries.push(PeEntry {
                path: format!("{section_name}/{}", leaf.folder),
                is_dir: true,
                data: None,
            });
        }
        let base = format!("{section_name}/{}/{}", leaf.folder, leaf.name);
        let path = unique_name(base, &mut used);
        entries.push(PeEntry {
            path,
            is_dir: false,
            data: Some(leaf.data),
        });
    }
    if entries.is_empty() {
        return Ok((None, Vec::new()));
    }
    Ok((Some(section_name), entries))
}

/// 资源叶子在构建阶段的临时结构。
struct ResourceLeaf {
    /// 类型文件夹名。
    folder: String,
    /// 文件名。
    name: String,
    /// 数据。
    data: PeData,
}

/// 返回资源显示名称，字符串优先，其次使用 ID。
fn resource_name_text(item: &ResourceItem) -> String {
    item.name
        .clone()
        .map(|name| sanitize_component(&name))
        .unwrap_or_else(|| item.name_id.to_string())
}

/// 返回资源类型文件夹名。
fn resource_type_name(item: &ResourceItem) -> String {
    if let Some(name) = &item.type_name {
        return sanitize_component(name);
    }
    match item.type_id {
        1 => "CURSOR".to_string(),
        2 => "BITMAP".to_string(),
        3 => "ICON".to_string(),
        4 => "MENU".to_string(),
        5 => "DIALOG".to_string(),
        6 => "STRINGTABLE".to_string(),
        7 => "FONTDIR".to_string(),
        8 => "FONT".to_string(),
        9 => "ACCELERATOR".to_string(),
        10 => "RCDATA".to_string(),
        11 => "MESSAGETABLE".to_string(),
        12 => "GROUP_CURSOR".to_string(),
        14 => "ICON".to_string(),
        16 => "VERSION".to_string(),
        17 => "DLGINCLUDE".to_string(),
        19 => "PLUGPLAY".to_string(),
        20 => "VXD".to_string(),
        21 => "ANICURSOR".to_string(),
        22 => "ANIICON".to_string(),
        23 => "HTML".to_string(),
        24 => "MANIFEST".to_string(),
        other => format!("TYPE_{other}"),
    }
}

/// 返回资源文件扩展名。
fn resource_extension(type_id: u32) -> &'static str {
    match type_id {
        2 => ".bmp",
        16 => ".bin",
        24 => ".manifest",
        _ => ".bin",
    }
}

/// 把资源 RVA 映射到文件偏移。
fn rva_to_offset(sections: &[PeSection], rva: u32) -> Option<u64> {
    for section in sections {
        let span = section.virtual_size.max(section.raw_size);
        if rva >= section.virtual_address && rva < section.virtual_address.saturating_add(span) {
            let delta = rva - section.virtual_address;
            return Some(u64::from(section.raw_offset) + u64::from(delta));
        }
    }
    None
}

/// 读取文件中的一段字节并校验长度。
fn read_file_range(path: &Path, offset: u64, size: u64) -> Result<Vec<u8>, ArchiveError> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::with_capacity(size.min(4 * 1024 * 1024) as usize);
    file.take(size).read_to_end(&mut data)?;
    if data.len() as u64 != size {
        return Err(ArchiveError::Corrupt("PE 数据范围不完整".to_string()));
    }
    Ok(data)
}

/// 把单个 RT_ICON 图像包装成单图像 ICO。
///
/// RT_ICON 数据本身已经是 ICO 中存储的 PNG 或 DIB；这里只补 `ICONDIR` 和
/// `ICONDIRENTRY`，文件名继续使用资源 ID，和常见归档工具的图标列表一致。
fn build_single_icon(image: &[u8]) -> Option<Vec<u8>> {
    let (width, height, planes, bits) = icon_metadata(image)?;
    let mut output = Vec::with_capacity(22 + image.len());
    output.extend_from_slice(&[0, 0, 1, 0]);
    output.extend_from_slice(&1u16.to_le_bytes());
    output.push(if width >= 256 { 0 } else { width as u8 });
    output.push(if height >= 256 { 0 } else { height as u8 });
    output.push(0);
    output.push(0);
    output.extend_from_slice(&planes.to_le_bytes());
    output.extend_from_slice(&bits.to_le_bytes());
    output.extend_from_slice(&u32::try_from(image.len()).ok()?.to_le_bytes());
    output.extend_from_slice(&22u32.to_le_bytes());
    output.extend_from_slice(image);
    Some(output)
}

/// 返回图标图像的宽、高、平面数和色深。
fn icon_metadata(image: &[u8]) -> Option<(u32, u32, u16, u16)> {
    if image.starts_with(b"\x89PNG\r\n\x1a\n") && image.len() >= 24 {
        let width = u32::from_be_bytes(image[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(image[20..24].try_into().ok()?);
        if width == 0 || height == 0 {
            return None;
        }
        return Some((width, height, 1, 32));
    }
    if image.len() < 16 {
        return None;
    }
    let width = i32::from_le_bytes(image[4..8].try_into().ok()?);
    let height = i32::from_le_bytes(image[8..12].try_into().ok()?).unsigned_abs() / 2;
    let planes = u16::from_le_bytes(image[12..14].try_into().ok()?);
    let bits = u16::from_le_bytes(image[14..16].try_into().ok()?);
    if width <= 0 || height == 0 {
        return None;
    }
    Some((width as u32, height, planes, bits))
}

/// 为位图资源补上 14 字节 BMP 文件头。
fn build_bitmap(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32::from_le_bytes(dib[0..4].try_into().ok()?) as usize;
    if header_size < 40 || header_size > dib.len() {
        return None;
    }
    let bits = u16::from_le_bytes(dib[14..16].try_into().ok()?) as u32;
    let colors = u32::from_le_bytes(dib[32..36].try_into().ok()?) as usize;
    let palette = if bits <= 8 {
        if colors != 0 {
            colors
        } else {
            1usize.checked_shl(bits)?
        }
    } else {
        0
    };
    let pixel_offset = 14 + header_size + palette.saturating_mul(4);
    let file_size = 14u32.checked_add(u32::try_from(dib.len()).ok()?)?;
    let mut output = Vec::with_capacity(file_size as usize);
    output.extend_from_slice(b"BM");
    output.extend_from_slice(&file_size.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(&(pixel_offset as u32).to_le_bytes());
    output.extend_from_slice(dib);
    Some(output)
}

/// 读取小端 u16。
fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, ArchiveError> {
    bytes
        .get(offset..offset + 2)
        .and_then(|slice| slice.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| ArchiveError::Corrupt("PE 字段越界".to_string()))
}

/// 读取小端 u32。
fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, ArchiveError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| ArchiveError::Corrupt("PE 字段越界".to_string()))
}

/// 清理资源名称，避免路径分隔符和控制字符进入归档路径。
fn sanitize_component(value: &str) -> String {
    let mut cleaned = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    cleaned = cleaned.trim().to_string();
    while cleaned.ends_with('.') {
        cleaned.pop();
    }
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        cleaned = "resource".to_string();
    }
    if cleaned.chars().count() > 120 {
        cleaned = cleaned.chars().take(120).collect();
    }
    cleaned
}

/// 为路径生成不重复名称；重复时在扩展名前追加 `_2`、`_3`。
fn unique_name(candidate: String, used: &mut HashSet<String>) -> String {
    if used.insert(candidate.clone()) {
        return candidate;
    }
    let (stem, extension) = match candidate.rsplit_once('.') {
        Some((stem, extension)) if !stem.ends_with('/') && !stem.ends_with('\\') => {
            (stem.to_string(), format!(".{extension}"))
        }
        _ => (candidate, String::new()),
    };
    for index in 2.. {
        let next = format!("{stem}_{index}{extension}");
        if used.insert(next.clone()) {
            return next;
        }
    }
    unreachable!("唯一路径循环不会结束")
}
