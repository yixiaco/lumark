//! MSI 的列表、解压和完整性测试。

use super::*;

use crate::format::OverwritePolicy;
use crate::msi::MsiArchive;

impl ArchiveService {
    /// 列出 MSI 内嵌 CAB 中的文件。
    pub(crate) fn list_msi(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let archive = MsiArchive::open(path)?;
        let mut total_size = 0u64;
        let entries = archive
            .entries()
            .iter()
            .enumerate()
            .map(|(id, entry)| {
                total_size = total_size.saturating_add(entry.size);
                ArchiveEntry {
                    id: id as u64,
                    path: entry.path.clone(),
                    is_dir: false,
                    size: entry.size,
                    compressed_size: None,
                    modified: None,
                    crc32: None,
                    method: "CAB".to_string(),
                    encrypted: false,
                }
            })
            .collect();
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Msi),
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

    /// 解压 MSI 内嵌 CAB 中的文件。
    pub(crate) fn extract_msi<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut archive = MsiArchive::open(path)?;
        fs::create_dir_all(&options.output_dir)?;
        let entries_total = archive
            .entries()
            .iter()
            .filter(|entry| selected(&entry.path, &options.selected_paths))
            .count();
        let bytes_total = archive
            .entries()
            .iter()
            .filter(|entry| selected(&entry.path, &options.selected_paths))
            .map(|entry| entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 MSI 文件".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in archive.entries().to_vec() {
            ensure_not_cancelled(&options.cancel)?;
            if !selected(&entry.path, &options.selected_paths) {
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
            let mut input = archive.open_entry(&entry)?;
            let mut output = File::create(&destination)?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                ensure_not_cancelled(&options.cancel)?;
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                ensure_budget(options, &report, read as u64)?;
                output.write_all(&buffer[..read])?;
                report.bytes = report.bytes.saturating_add(read as u64);
                progress(ExtractProgress {
                    current: entry.path.clone(),
                    bytes_done: report.bytes,
                    bytes_total,
                    entries_done: report.files,
                    entries_total,
                });
            }
            report.files += 1;
        }
        Ok(report)
    }

    /// 完整读取 MSI 内嵌 CAB，验证压缩流。
    pub(crate) fn test_msi<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut archive = MsiArchive::open(path)?;
        let entries_total = archive.entries().len();
        let bytes_total = archive
            .entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在测试 MSI".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for entry in archive.entries().to_vec() {
            let mut input = archive.open_entry(&entry)?;
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
