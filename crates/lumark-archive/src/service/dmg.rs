//! DMG（UDIF）只读列表、解压和完整性测试。

use super::*;

use crate::format::OverwritePolicy;
use dpp::{DmgPipeline, ExtractMode, FsEntryKind};

impl ArchiveService {
    /// 列出 DMG 内 HFS+/APFS 卷中的文件。
    pub(crate) fn list_dmg(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let mut pipeline = DmgPipeline::open(path)
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 打开失败：{error}")))?;
        let mut filesystem = pipeline
            .open_filesystem_with_mode(ExtractMode::TempFile)
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 文件系统打开失败：{error}")))?;
        let walk = filesystem
            .walk()
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 目录遍历失败：{error}")))?;
        let mut total_size = 0u64;
        let mut entries = Vec::new();
        for item in walk {
            let path = dmg_path(&item.path);
            if path.is_empty() {
                continue;
            }
            match item.entry.kind {
                FsEntryKind::File => {
                    total_size = total_size.saturating_add(item.entry.size);
                    entries.push(ArchiveEntry {
                        id: entries.len() as u64,
                        path,
                        is_dir: false,
                        size: item.entry.size,
                        compressed_size: None,
                        modified: None,
                        crc32: None,
                        method: "UDIF".to_string(),
                        encrypted: false,
                    });
                }
                FsEntryKind::Directory => entries.push(ArchiveEntry {
                    id: entries.len() as u64,
                    path,
                    is_dir: true,
                    size: 0,
                    compressed_size: None,
                    modified: None,
                    crc32: None,
                    method: "UDIF".to_string(),
                    encrypted: false,
                }),
                FsEntryKind::Symlink => {}
            }
        }
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Dmg),
            id: 0,
            entries,
            encrypted: false,
            header_encrypted: false,
            solid: false,
            comment: None,
            total_size,
            compressed_size: fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
        })
    }

    /// 解压 DMG 中的文件，跳过符号链接。
    pub(crate) fn extract_dmg<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut pipeline = DmgPipeline::open(path)
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 打开失败：{error}")))?;
        let mut filesystem = pipeline
            .open_filesystem_with_mode(ExtractMode::TempFile)
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 文件系统打开失败：{error}")))?;
        let walk = filesystem
            .walk()
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 目录遍历失败：{error}")))?;
        fs::create_dir_all(&options.output_dir)?;
        let selected_files = walk
            .into_iter()
            .filter(|item| item.entry.kind == FsEntryKind::File)
            .filter_map(|item| {
                let path = dmg_path(&item.path);
                (!path.is_empty() && selected(&path, &options.selected_paths))
                    .then_some((item, path))
            })
            .collect::<Vec<_>>();
        let entries_total = selected_files.len();
        let bytes_total = selected_files
            .iter()
            .map(|(item, _)| item.entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 DMG".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for (item, path_string) in selected_files {
            ensure_not_cancelled(&options.cancel)?;
            let mut destination = safe_join(&options.output_dir, &path_string)?;
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
            ensure_budget(options, &report, item.entry.size)?;
            let mut output = File::create(&destination)?;
            let copied = filesystem
                .read_file_to(&item.path, &mut output)
                .map_err(|error| ArchiveError::Corrupt(format!("DMG 文件读取失败：{error}")))?;
            report.files += 1;
            report.bytes = report.bytes.saturating_add(copied);
            progress(ExtractProgress {
                current: path_string,
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total,
            });
        }
        Ok(report)
    }

    /// 完整读取 DMG 中的普通文件。
    pub(crate) fn test_dmg<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut pipeline = DmgPipeline::open(path)
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 打开失败：{error}")))?;
        let mut filesystem = pipeline
            .open_filesystem_with_mode(ExtractMode::TempFile)
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 文件系统打开失败：{error}")))?;
        let walk = filesystem
            .walk()
            .map_err(|error| ArchiveError::Corrupt(format!("DMG 目录遍历失败：{error}")))?;
        let entries_total = walk
            .iter()
            .filter(|item| item.entry.kind == FsEntryKind::File)
            .count();
        let bytes_total = walk
            .iter()
            .filter(|item| item.entry.kind == FsEntryKind::File)
            .map(|item| item.entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在测试 DMG".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for item in walk
            .iter()
            .filter(|item| item.entry.kind == FsEntryKind::File)
        {
            let mut sink = io::sink();
            let copied = filesystem
                .read_file_to(&item.path, &mut sink)
                .map_err(|error| ArchiveError::Corrupt(format!("DMG 文件读取失败：{error}")))?;
            report.bytes = report.bytes.saturating_add(copied);
            report.files += 1;
            progress(ExtractProgress {
                current: dmg_path(&item.path),
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total,
            });
        }
        Ok(report)
    }
}

/// 把 DMG 绝对路径转换成归档相对路径。
fn dmg_path(path: &str) -> String {
    path.trim_start_matches('/')
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}
