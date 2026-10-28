//! CHM 的列表、解压和完整性测试。

use super::*;

use libchm::{ChmFile, EntryKind, EntrySel};

use crate::format::OverwritePolicy;

impl ArchiveService {
    /// 列出 CHM 普通内容文件。
    pub(crate) fn list_chm(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let entries = chm_entries(path)?;
        let mut total_size = 0u64;
        let archive_entries = entries
            .iter()
            .enumerate()
            .map(|(id, entry)| {
                total_size = total_size.saturating_add(entry.size);
                ArchiveEntry {
                    id: id as u64,
                    path: entry.path.clone(),
                    is_dir: entry.is_dir,
                    size: entry.size,
                    compressed_size: None,
                    modified: None,
                    crc32: None,
                    method: "LZX".to_string(),
                    encrypted: false,
                }
            })
            .collect();
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Chm),
            id: 0,
            entries: archive_entries,
            encrypted: false,
            header_encrypted: false,
            solid: false,
            comment: None,
            total_size,
            compressed_size: fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
        })
    }

    /// 解压 CHM 普通内容文件。
    pub(crate) fn extract_chm<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let entries = chm_entries(path)?;
        let mut chm = ChmFile::open(path)
            .map_err(|error| ArchiveError::Corrupt(format!("CHM 打开失败：{error}")))?;
        fs::create_dir_all(&options.output_dir)?;
        let entries_total = entries
            .iter()
            .filter(|entry| !entry.is_dir && selected(&entry.path, &options.selected_paths))
            .count();
        let bytes_total = entries
            .iter()
            .filter(|entry| !entry.is_dir && selected(&entry.path, &options.selected_paths))
            .map(|entry| entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 CHM".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in entries {
            ensure_not_cancelled(&options.cancel)?;
            if !selected(&entry.path, &options.selected_paths) {
                continue;
            }
            if entry.is_dir {
                continue;
            }
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
            let bytes = chm
                .read_path(&format!("/{}", entry.path))
                .map_err(|error| ArchiveError::Corrupt(format!("CHM 条目读取失败：{error}")))?;
            ensure_budget(options, &report, bytes.len() as u64)?;
            fs::write(&destination, &bytes)?;
            report.files += 1;
            report.bytes = report.bytes.saturating_add(bytes.len() as u64);
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

    /// 完整读取 CHM 内容，验证 LZX 数据。
    pub(crate) fn test_chm<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let entries = chm_entries(path)?;
        let mut chm = ChmFile::open(path)
            .map_err(|error| ArchiveError::Corrupt(format!("CHM 打开失败：{error}")))?;
        let entries_total = entries.iter().filter(|entry| !entry.is_dir).count();
        let bytes_total = entries.iter().map(|entry| entry.size).sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在测试 CHM".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in entries.iter().filter(|entry| !entry.is_dir) {
            let bytes = chm
                .read_path(&format!("/{}", entry.path))
                .map_err(|error| ArchiveError::Corrupt(format!("CHM 条目读取失败：{error}")))?;
            report.bytes = report.bytes.saturating_add(bytes.len() as u64);
            report.files += 1;
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
}

/// CHM 内部条目。
struct ChmEntry {
    path: String,
    is_dir: bool,
    size: u64,
}

/// 读取 CHM 普通命名空间中的文件和目录。
fn chm_entries(path: &Path) -> Result<Vec<ChmEntry>, ArchiveError> {
    let mut chm = ChmFile::open(path)
        .map_err(|error| ArchiveError::Corrupt(format!("CHM 打开失败：{error}")))?;
    let entries = chm
        .entries(EntrySel::NORMAL | EntrySel::FILES | EntrySel::DIRS)
        .map_err(|error| ArchiveError::Corrupt(format!("CHM 目录读取失败：{error}")))?;
    let mut result = Vec::new();
    for entry in entries {
        let is_dir = entry.kind == EntryKind::Dir;
        let trimmed = entry.path.trim_start_matches('/').trim_end_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        result.push(ChmEntry {
            path: trimmed.to_string(),
            is_dir,
            size: entry.length,
        });
    }
    Ok(result)
}
