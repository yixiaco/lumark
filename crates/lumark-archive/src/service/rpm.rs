//! RPM 包的列表、解压和完整性测试。

use super::*;

use crate::format::OverwritePolicy;
use crate::rpm::{RpmArchive, read_cpio_header, skip_cpio_padding};

impl ArchiveService {
    /// 列出 RPM 载荷中的 cpio 条目。
    pub(crate) fn list_rpm(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let archive = RpmArchive::open(path)?;
        let mut payload = archive.open_payload()?;
        let mut entries = Vec::new();
        let mut total_size = 0u64;
        while let Some(entry) = read_cpio_header(&mut payload)? {
            if !entry.is_dir() && !entry.is_symlink() {
                total_size = total_size.saturating_add(entry.size);
            }
            let id = entries.len() as u64;
            entries.push(ArchiveEntry {
                id,
                path: entry.path.clone(),
                is_dir: entry.is_dir(),
                size: if entry.is_dir() { 0 } else { entry.size },
                compressed_size: None,
                modified: None,
                crc32: None,
                method: if entry.is_symlink() {
                    "符号链接".to_string()
                } else {
                    "cpio".to_string()
                },
                encrypted: false,
            });
            io::copy(&mut payload.by_ref().take(entry.size), &mut io::sink())?;
            skip_cpio_padding(&mut payload, entry.size)?;
        }
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Rpm),
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

    /// 解压 RPM 载荷中的 cpio 条目。
    pub(crate) fn extract_rpm<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let info = Self::list_rpm(path)?;
        fs::create_dir_all(&options.output_dir)?;
        let entries_total = info
            .entries
            .iter()
            .filter(|entry| !entry.is_dir && selected(&entry.path, &options.selected_paths))
            .count();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 RPM 载荷".to_string(),
            bytes_done: 0,
            bytes_total: info.total_size,
            entries_done: 0,
            entries_total,
        });
        let archive = RpmArchive::open(path)?;
        let mut payload = archive.open_payload()?;
        while let Some(entry) = read_cpio_header(&mut payload)? {
            if !selected(&entry.path, &options.selected_paths) {
                io::copy(&mut payload.by_ref().take(entry.size), &mut io::sink())?;
                skip_cpio_padding(&mut payload, entry.size)?;
                continue;
            }
            let mut destination = safe_join(&options.output_dir, &entry.path)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            if entry.is_dir() {
                fs::create_dir_all(&destination)?;
                skip_cpio_padding(&mut payload, 0)?;
                continue;
            }
            if entry.is_symlink() {
                io::copy(&mut payload.by_ref().take(entry.size), &mut io::sink())?;
                skip_cpio_padding(&mut payload, entry.size)?;
                report.skipped += 1;
                continue;
            }
            match options.overwrite_policy {
                OverwritePolicy::Rename => destination = unique_destination(&destination),
                OverwritePolicy::Skip => {
                    io::copy(&mut payload.by_ref().take(entry.size), &mut io::sink())?;
                    skip_cpio_padding(&mut payload, entry.size)?;
                    report.skipped += 1;
                    continue;
                }
                OverwritePolicy::Ask if !options.overwrite => {
                    io::copy(&mut payload.by_ref().take(entry.size), &mut io::sink())?;
                    skip_cpio_padding(&mut payload, entry.size)?;
                    report.skipped += 1;
                    continue;
                }
                _ => {}
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&destination)?;
            let mut remaining = entry.size;
            let mut buffer = [0u8; 64 * 1024];
            while remaining > 0 {
                ensure_not_cancelled(&options.cancel)?;
                let take = remaining.min(buffer.len() as u64) as usize;
                let read = payload.read(&mut buffer[..take])?;
                if read == 0 {
                    return Err(ArchiveError::Corrupt(format!(
                        "RPM 条目提前结束：{}",
                        entry.path
                    )));
                }
                ensure_budget(options, &report, read as u64)?;
                output.write_all(&buffer[..read])?;
                report.bytes = report.bytes.saturating_add(read as u64);
                remaining -= read as u64;
                progress(ExtractProgress {
                    current: entry.path.clone(),
                    bytes_done: report.bytes,
                    bytes_total: info.total_size,
                    entries_done: report.files,
                    entries_total,
                });
            }
            skip_cpio_padding(&mut payload, entry.size)?;
            report.files += 1;
        }
        Ok(report)
    }

    /// 完整读取 RPM 载荷，验证 cpio 和压缩流。
    pub(crate) fn test_rpm<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let info = Self::list_rpm(path)?;
        let entries_total = info.entries.iter().filter(|entry| !entry.is_dir).count();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在测试 RPM 包".to_string(),
            bytes_done: 0,
            bytes_total: info.total_size,
            entries_done: 0,
            entries_total,
        });
        let archive = RpmArchive::open(path)?;
        let mut payload = archive.open_payload()?;
        while let Some(entry) = read_cpio_header(&mut payload)? {
            let copied = io::copy(&mut payload.by_ref().take(entry.size), &mut io::sink())?;
            skip_cpio_padding(&mut payload, entry.size)?;
            if !entry.is_dir() && !entry.is_symlink() {
                report.bytes = report.bytes.saturating_add(copied);
                report.files += 1;
                progress(ExtractProgress {
                    current: entry.path.clone(),
                    bytes_done: report.bytes,
                    bytes_total: info.total_size,
                    entries_done: report.files,
                    entries_total,
                });
            }
        }
        Ok(report)
    }
}
