//! EGG 归档的列表、解压和完整性测试。

use super::*;

use std::io::BufReader;

use crate::format::OverwritePolicy;
use unegg::archive::EggArchive;
use unegg::extract::extract_entry;
use unegg::volume::MultiVolumeReader;

/// EGG 单文件或分卷读取器的统一接口。
trait EggReadSeek: Read + Seek {}

impl<T: Read + Seek> EggReadSeek for T {}

impl ArchiveService {
    /// 列出 EGG 归档条目。
    pub(crate) fn list_egg(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let archive = open_egg(path)?;
        let mut total_size = 0u64;
        let entries = archive
            .entries
            .iter()
            .enumerate()
            .map(|(id, entry)| {
                let is_dir = entry.is_directory();
                let size = if is_dir { 0 } else { entry.uncompressed_size };
                total_size = total_size.saturating_add(size);
                let method = entry
                    .blocks
                    .first()
                    .map_or("Store", |block| block.compression_method.name());
                ArchiveEntry {
                    id: id as u64,
                    path: entry.file_name.clone(),
                    is_dir,
                    size,
                    compressed_size: Some(
                        entry
                            .blocks
                            .iter()
                            .map(|block| block.compressed_size as u64)
                            .sum(),
                    ),
                    modified: None,
                    crc32: None,
                    method: method.to_string(),
                    encrypted: entry.encrypt_info.is_some(),
                }
            })
            .collect();
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Egg),
            id: 0,
            entries,
            encrypted: archive.is_encrypted,
            header_encrypted: false,
            solid: archive.is_solid,
            comment: None,
            total_size,
            compressed_size: fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
        })
    }

    /// 解压 EGG 归档。
    pub(crate) fn extract_egg<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        extract_egg_to(path, &options.output_dir, options, progress)
    }

    /// 完整读取 EGG 归档；使用临时目录承载解压输出。
    pub(crate) fn test_egg<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let directory = std::env::temp_dir().join(format!(
            "lumark-egg-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&directory)?;
        let options = ExtractionOptions {
            output_dir: directory.clone(),
            overwrite: true,
            ..Default::default()
        };
        let result = extract_egg_to(path, &directory, &options, progress);
        let _ = fs::remove_dir_all(directory);
        result
    }
}

/// 打开 EGG 归档。
fn open_egg(path: &Path) -> Result<EggArchive<Box<dyn EggReadSeek>>, ArchiveError> {
    let reader: Box<dyn EggReadSeek> = match MultiVolumeReader::try_open(path)
        .map_err(|error| ArchiveError::Corrupt(format!("EGG 分卷发现失败：{error}")))?
    {
        Some(reader) => Box::new(reader),
        None => Box::new(BufReader::new(File::open(path)?)),
    };
    EggArchive::open(reader)
        .map_err(|error| ArchiveError::Corrupt(format!("EGG 解析失败：{error}")))
}

/// 把选定条目解压到目标目录。
fn extract_egg_to<F>(
    path: &Path,
    output_dir: &Path,
    options: &ExtractionOptions,
    progress: &mut F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(ExtractProgress),
{
    let mut archive = open_egg(path)?;
    fs::create_dir_all(output_dir)?;
    let entries = archive.entries.clone();
    let selected_entries = entries
        .iter()
        .filter(|entry| selected(&entry.file_name, &options.selected_paths))
        .collect::<Vec<_>>();
    let entries_total = selected_entries
        .iter()
        .filter(|entry| !entry.is_directory())
        .count();
    let bytes_total = selected_entries
        .iter()
        .filter(|entry| !entry.is_directory())
        .map(|entry| entry.uncompressed_size)
        .sum::<u64>();
    let mut report = ExtractReport::default();
    progress(ExtractProgress {
        current: "正在解压 EGG".to_string(),
        bytes_done: 0,
        bytes_total,
        entries_done: 0,
        entries_total,
    });
    for entry in entries {
        ensure_not_cancelled(&options.cancel)?;
        if !selected(&entry.file_name, &options.selected_paths) {
            continue;
        }
        if entry.is_directory() {
            let directory = safe_join(output_dir, &entry.file_name)?;
            ensure_no_symlink_components(output_dir, &directory)?;
            fs::create_dir_all(directory)?;
            continue;
        }
        let mut target = safe_join(output_dir, &entry.file_name)?;
        ensure_no_symlink_components(output_dir, &target)?;
        if target.exists() {
            match options.overwrite_policy {
                OverwritePolicy::Rename => target = unique_destination(&target),
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
        }
        ensure_budget(options, &report, entry.uncompressed_size)?;
        let mut extracted = entry.clone();
        if target != safe_join(output_dir, &entry.file_name)? {
            extracted.file_name = target
                .strip_prefix(output_dir)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| entry.file_name.clone());
        }
        extract_entry(
            &mut archive,
            &extracted,
            output_dir,
            options.password.as_deref().map(String::as_str),
            false,
        )
        .map_err(|error| ArchiveError::Corrupt(format!("EGG 条目解压失败：{error}")))?;
        report.files += 1;
        report.bytes = report.bytes.saturating_add(entry.uncompressed_size);
        progress(ExtractProgress {
            current: entry.file_name,
            bytes_done: report.bytes,
            bytes_total,
            entries_done: report.files,
            entries_total,
        });
    }
    Ok(report)
}
