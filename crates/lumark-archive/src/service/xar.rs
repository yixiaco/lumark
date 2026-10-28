//! XAR/PKG 的列表、解压和完整性测试。

use super::*;

use crate::format::OverwritePolicy;

impl ArchiveService {
    /// 列出 XAR 目录条目。
    pub(crate) fn list_xar(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let archive = crate::xar::XarArchive::open(path)?;
        let mut entries = Vec::with_capacity(archive.entries().len());
        let mut total_size = 0u64;
        let mut compressed_size = 0u64;
        for (id, entry) in archive.entries().iter().enumerate() {
            if !entry.is_dir && !entry.is_symlink {
                total_size = total_size.saturating_add(entry.size);
                compressed_size = compressed_size.saturating_add(entry.compressed_size);
            }
            entries.push(ArchiveEntry {
                id: id as u64,
                path: entry.path.clone(),
                is_dir: entry.is_dir,
                size: if entry.is_dir { 0 } else { entry.size },
                compressed_size: (!entry.is_dir).then_some(entry.compressed_size),
                modified: None,
                crc32: None,
                method: if let Some(link) = &entry.link {
                    format!("符号链接 -> {link}")
                } else {
                    entry.method.to_string()
                },
                encrypted: false,
            });
        }
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Xar),
            id: 0,
            entries,
            encrypted: false,
            header_encrypted: false,
            solid: false,
            comment: None,
            total_size,
            compressed_size,
        })
    }

    /// 解压 XAR 条目，支持覆盖策略、预算和取消。
    pub(crate) fn extract_xar<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let archive = crate::xar::XarArchive::open(path)?;
        fs::create_dir_all(&options.output_dir)?;
        let entries_total = archive
            .entries()
            .iter()
            .filter(|entry| {
                !entry.is_dir && !entry.is_symlink && selected(&entry.path, &options.selected_paths)
            })
            .count();
        let bytes_total = archive
            .entries()
            .iter()
            .filter(|entry| {
                !entry.is_dir && !entry.is_symlink && selected(&entry.path, &options.selected_paths)
            })
            .map(|entry| entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 XAR 归档".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in archive.entries() {
            ensure_not_cancelled(&options.cancel)?;
            if !selected(&entry.path, &options.selected_paths) {
                continue;
            }
            let mut destination = safe_join(&options.output_dir, &entry.path)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            if entry.is_dir {
                fs::create_dir_all(&destination)?;
                continue;
            }
            if entry.is_symlink {
                report.skipped += 1;
                continue;
            }
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
            let mut input = archive.open_entry(entry)?;
            let mut output = File::create(&destination)?;
            let mut buffer = [0u8; 64 * 1024];
            let mut file_bytes = 0u64;
            loop {
                ensure_not_cancelled(&options.cancel)?;
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                ensure_budget(options, &report, read as u64)?;
                output.write_all(&buffer[..read])?;
                file_bytes = file_bytes.saturating_add(read as u64);
                report.bytes = report.bytes.saturating_add(read as u64);
                progress(ExtractProgress {
                    current: entry.path.clone(),
                    bytes_done: report.bytes,
                    bytes_total,
                    entries_done: report.files,
                    entries_total,
                });
            }
            let _ = file_bytes;
            report.files += 1;
        }
        Ok(report)
    }

    /// 完整读取 XAR 条目，验证压缩流和 CRC 是否存在。
    pub(crate) fn test_xar<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let archive = crate::xar::XarArchive::open(path)?;
        let entries_total = archive
            .entries()
            .iter()
            .filter(|entry| !entry.is_dir)
            .count();
        let bytes_total = archive
            .entries()
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在测试 XAR 归档".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in archive.entries() {
            if entry.is_dir || entry.is_symlink {
                continue;
            }
            let mut input = archive.open_entry(entry)?;
            let copied = io::copy(&mut input, &mut io::sink())?;
            report.bytes = report.bytes.saturating_add(copied);
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
