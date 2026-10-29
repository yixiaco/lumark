//! NSIS 安装包的列表、解压和完整性测试。

use super::*;

use crate::format::OverwritePolicy;
use ::nsis::NsisInstaller;
use std::fs::File;
use std::io::Read;

/// NSIS 首个头部前后的标准与旧版标记。
const MARKERS: [&[u8]; 2] = [b"NullsoftInst", b"nsisinstall\0"];

/// 流式检查文件是否包含 NSIS 头部标记。
///
/// NSIS 解析器会在 PE 覆盖数据中逐字节搜索头部，普通大 EXE 因此要承受
/// 完整读入和长时间扫描。先按固定标记过滤，只有真正带 NSIS 数据的外壳
/// 才进入完整解析；缓冲区保留标记长度减一的重叠，避免跨块漏检。
pub(super) fn contains_marker(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let overlap = MARKERS.iter().map(|marker| marker.len()).max().unwrap_or(1) - 1;
    let mut buffer = vec![0u8; 64 * 1024 + overlap];
    let mut carry = 0usize;
    loop {
        let Ok(read) = file.read(&mut buffer[carry..]) else {
            return false;
        };
        let filled = carry + read;
        let bytes = &buffer[..filled];
        if MARKERS
            .iter()
            .any(|marker| bytes.windows(marker.len()).any(|window| window == *marker))
        {
            return true;
        }
        if read == 0 {
            return false;
        }
        let keep = overlap.min(filled);
        buffer.copy_within(filled - keep..filled, 0);
        carry = keep;
    }
}

impl ArchiveService {
    /// 列出 NSIS 安装包内嵌文件。
    pub(crate) fn list_nsis(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let bytes = fs::read(path)?;
        let installer = NsisInstaller::from_bytes(&bytes)
            .map_err(|error| ArchiveError::Corrupt(format!("NSIS 解析失败：{error}")))?;
        let mut total_size = 0u64;
        let mut entries = Vec::new();
        for file in installer.files() {
            let file =
                file.map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件读取失败：{error}")))?;
            let name = file
                .name()
                .map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件名读取失败：{error}")))?
                .to_string();
            let path = nsis_path(&name);
            if path.is_empty() {
                continue;
            }
            let data = file
                .decompress()
                .map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件解压失败：{error}")))?;
            total_size = total_size.saturating_add(data.len() as u64);
            entries.push(ArchiveEntry {
                id: entries.len() as u64,
                path,
                is_dir: false,
                size: data.len() as u64,
                compressed_size: Some(file.data().len() as u64),
                modified: None,
                crc32: None,
                method: "NSIS".to_string(),
                encrypted: false,
            });
        }
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Nsis),
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

    /// 解压 NSIS 安装包内嵌文件。
    pub(crate) fn extract_nsis<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let bytes = fs::read(path)?;
        let installer = NsisInstaller::from_bytes(&bytes)
            .map_err(|error| ArchiveError::Corrupt(format!("NSIS 解析失败：{error}")))?;
        fs::create_dir_all(&options.output_dir)?;
        let files = installer
            .files()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件读取失败：{error}")))?;
        let selected_files = files
            .iter()
            .filter_map(|file| {
                let name = file.name().ok()?.to_string();
                let path = nsis_path(&name);
                (!path.is_empty() && selected(&path, &options.selected_paths))
                    .then_some((file, path))
            })
            .collect::<Vec<_>>();
        let bytes_total = selected_files
            .iter()
            .filter_map(|(file, _)| file.decompress().ok())
            .map(|data| data.len() as u64)
            .sum::<u64>();
        let entries_total = selected_files.len();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 NSIS".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for (file, path_string) in selected_files {
            ensure_not_cancelled(&options.cancel)?;
            let data = file
                .decompress()
                .map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件解压失败：{error}")))?;
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
            ensure_budget(options, &report, data.len() as u64)?;
            fs::write(&destination, &data)?;
            report.files += 1;
            report.bytes = report.bytes.saturating_add(data.len() as u64);
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

    /// 完整读取 NSIS 内嵌文件，验证压缩数据。
    pub(crate) fn test_nsis<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let bytes = fs::read(path)?;
        let installer = NsisInstaller::from_bytes(&bytes)
            .map_err(|error| ArchiveError::Corrupt(format!("NSIS 解析失败：{error}")))?;
        let mut report = ExtractReport::default();
        let mut entries_total = 0usize;
        let mut bytes_total = 0u64;
        for file in installer.files() {
            let file =
                file.map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件读取失败：{error}")))?;
            if !nsis_path(
                &file
                    .name()
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
                    .to_string(),
            )
            .is_empty()
            {
                entries_total += 1;
                bytes_total = bytes_total
                    .saturating_add(file.decompress().map(|data| data.len() as u64).unwrap_or(0));
            }
        }
        progress(ExtractProgress {
            current: "正在测试 NSIS".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for file in installer.files() {
            let file =
                file.map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件读取失败：{error}")))?;
            let data = file
                .decompress()
                .map_err(|error| ArchiveError::Corrupt(format!("NSIS 文件解压失败：{error}")))?;
            report.bytes = report.bytes.saturating_add(data.len() as u64);
            report.files += 1;
            progress(ExtractProgress {
                current: nsis_path(
                    &file
                        .name()
                        .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
                        .to_string(),
                ),
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total,
            });
        }
        Ok(report)
    }
}

/// 把 NSIS 安装路径转换成归档相对路径。
fn nsis_path(name: &str) -> String {
    let normalized = name.replace('\\', "/");
    let normalized = normalized
        .strip_prefix("$INSTDIR/")
        .or_else(|| normalized.strip_prefix("$INSTDIR"))
        .unwrap_or(&normalized);
    normalized
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::contains_marker;

    /// 标记跨越读取缓冲区时仍应被识别，避免漏掉真实安装包。
    #[test]
    fn detects_marker_across_buffer_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("installer.exe");
        let mut bytes = vec![0u8; 64 * 1024 - 5];
        bytes.extend_from_slice(b"NullsoftInst");
        std::fs::write(&path, bytes).unwrap();
        assert!(contains_marker(&path));
    }

    /// 普通可执行文件没有 NSIS 标记时直接跳过完整解析。
    #[test]
    fn ignores_executable_without_marker() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.exe");
        std::fs::write(&path, vec![0u8; 128 * 1024]).unwrap();
        assert!(!contains_marker(&path));
    }
}
