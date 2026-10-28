//! SquashFS 4 只读列表、解压和完整性测试。

use super::*;

use std::io::BufReader;

use crate::format::OverwritePolicy;
use backhand::{FilesystemReader, InnerNode};

impl ArchiveService {
    /// 列出 SquashFS 中的普通文件和目录。
    pub(crate) fn list_squashfs(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let filesystem = squashfs_reader(path)?;
        let mut total_size = 0u64;
        let mut entries = Vec::new();
        for node in filesystem.files() {
            let path_string = squash_path(&node.fullpath);
            if path_string.is_empty() {
                continue;
            }
            match &node.inner {
                InnerNode::File(file) => {
                    let size = file.file_len() as u64;
                    total_size = total_size.saturating_add(size);
                    entries.push(ArchiveEntry {
                        id: entries.len() as u64,
                        path: path_string,
                        is_dir: false,
                        size,
                        compressed_size: None,
                        modified: None,
                        crc32: None,
                        method: "SquashFS".to_string(),
                        encrypted: false,
                    });
                }
                InnerNode::Dir(_) => {
                    entries.push(ArchiveEntry {
                        id: entries.len() as u64,
                        path: path_string,
                        is_dir: true,
                        size: 0,
                        compressed_size: None,
                        modified: None,
                        crc32: None,
                        method: "SquashFS".to_string(),
                        encrypted: false,
                    });
                }
                _ => {}
            }
        }
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::SquashFs),
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

    /// 解压 SquashFS 普通文件，跳过符号链接和设备节点。
    pub(crate) fn extract_squashfs<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let filesystem = squashfs_reader(path)?;
        fs::create_dir_all(&options.output_dir)?;
        let entries_total = filesystem
            .files()
            .filter(|node| {
                matches!(&node.inner, InnerNode::File(_))
                    && selected(&squash_path(&node.fullpath), &options.selected_paths)
            })
            .count();
        let bytes_total = filesystem
            .files()
            .filter_map(|node| match &node.inner {
                InnerNode::File(file) => {
                    let path = squash_path(&node.fullpath);
                    selected(&path, &options.selected_paths).then_some(file.file_len() as u64)
                }
                _ => None,
            })
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在解压 SquashFS".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for node in filesystem.files() {
            ensure_not_cancelled(&options.cancel)?;
            let path_string = squash_path(&node.fullpath);
            if path_string.is_empty() || !selected(&path_string, &options.selected_paths) {
                continue;
            }
            match &node.inner {
                InnerNode::Dir(_) => {}
                InnerNode::File(file) => {
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
                    let mut reader = filesystem.file(file).reader();
                    let mut output = File::create(&destination)?;
                    let mut buffer = [0u8; 64 * 1024];
                    loop {
                        ensure_not_cancelled(&options.cancel)?;
                        let read = reader.read(&mut buffer)?;
                        if read == 0 {
                            break;
                        }
                        ensure_budget(options, &report, read as u64)?;
                        output.write_all(&buffer[..read])?;
                        report.bytes = report.bytes.saturating_add(read as u64);
                        progress(ExtractProgress {
                            current: path_string.clone(),
                            bytes_done: report.bytes,
                            bytes_total,
                            entries_done: report.files,
                            entries_total,
                        });
                    }
                    report.files += 1;
                }
                _ => {}
            }
        }
        Ok(report)
    }

    /// 完整读取 SquashFS 普通文件，验证数据块。
    pub(crate) fn test_squashfs<F>(
        path: &Path,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let filesystem = squashfs_reader(path)?;
        let entries_total = filesystem
            .files()
            .filter(|node| matches!(&node.inner, InnerNode::File(_)))
            .count();
        let bytes_total = filesystem
            .files()
            .filter_map(|node| match &node.inner {
                InnerNode::File(file) => Some(file.file_len() as u64),
                _ => None,
            })
            .sum::<u64>();
        let mut report = ExtractReport::default();
        progress(ExtractProgress {
            current: "正在测试 SquashFS".to_string(),
            bytes_done: 0,
            bytes_total,
            entries_done: 0,
            entries_total,
        });
        for node in filesystem.files() {
            let InnerNode::File(file) = &node.inner else {
                continue;
            };
            let mut reader = filesystem.file(file).reader();
            let copied = io::copy(&mut reader, &mut io::sink())?;
            report.bytes = report.bytes.saturating_add(copied);
            report.files += 1;
            progress(ExtractProgress {
                current: squash_path(&node.fullpath),
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total,
            });
        }
        Ok(report)
    }
}

/// 打开 SquashFS 并建立文件系统视图。
fn squashfs_reader(path: &Path) -> Result<FilesystemReader<'static>, ArchiveError> {
    FilesystemReader::from_reader(BufReader::new(File::open(path)?))
        .map_err(|error| ArchiveError::Corrupt(format!("SquashFS 解析失败：{error}")))
}

/// 把 SquashFS 绝对路径转换成归档相对路径。
fn squash_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}
