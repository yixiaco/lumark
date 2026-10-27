//! ZPAQ 创建：把源计划分批追加到标准 JIDAC 归档。
//!
//! 每个文件用独立的 `-to` 映射写入精确的归档路径；分批可以限制单次引擎
//! 调用的耗时，并在批次之间响应取消和上报进度。写入完成后重新列出归档，
//! 确认条目集合与计划完全一致，避免前缀映射失败时把绝对路径写进归档。

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::error::ArchiveError;
use crate::format::{ArchiveTimeSource, CompressionOptions, CompressionPreset, CreateReport};
use crate::path::ArchivePath;
use crate::service::{CreateProgress, build_source_plan, ensure_not_cancelled};

use super::{ZPAQ_BATCH_BYTES, ZPAQ_BATCH_FILES, command, path_string};

/// 一个已校验、等待写入的源文件。
struct PlannedFile {
    /// 归档内统一使用 `/` 的路径。
    archive_path: String,
    /// 实际交给引擎读取的本地路径；归档时间被改写时指向暂存副本。
    source: PathBuf,
    bytes: u64,
}

/// 创建 ZPAQ 归档。
///
/// 输出路径由通用创建流程提供，通常是目标目录内的临时文件；失败时由
/// 上层负责删除半成品。`archive_time` 非原始时间时会把源文件复制到临时
/// 目录并改写 mtime，默认路径不产生额外复制。
pub(crate) fn create<F>(
    output: &Path,
    sources: &[PathBuf],
    options: &CompressionOptions,
    progress: &mut F,
) -> Result<CreateReport, ArchiveError>
where
    F: FnMut(CreateProgress),
{
    let plan = build_source_plan(sources, options)?;
    let total_bytes = plan.total_bytes;
    let mut files = Vec::with_capacity(plan.files.len());
    let mut seen = HashSet::with_capacity(plan.files.len());
    for (relative, source, bytes) in plan.files {
        let archive_path = ArchivePath::parse(&relative)?.display();
        if archive_path.starts_with('-') {
            return Err(ArchiveError::InvalidRequest(format!(
                "ZPAQ 压缩不支持以 - 开头的归档路径：{archive_path}"
            )));
        }
        if !seen.insert(archive_path.clone()) {
            return Err(ArchiveError::InvalidRequest(format!(
                "ZPAQ 压缩存在重复的归档路径：{archive_path}"
            )));
        }
        let source = std::path::absolute(source)?;
        files.push(PlannedFile {
            archive_path,
            source,
            bytes,
        });
    }
    if files.is_empty() {
        return Err(ArchiveError::InvalidRequest(
            "ZPAQ 压缩没有可添加的文件".to_string(),
        ));
    }

    let entries_total = files.len();
    progress(CreateProgress {
        current: "正在准备 ZPAQ 压缩".to_string(),
        bytes_done: 0,
        bytes_total: total_bytes,
        current_bytes_done: 0,
        current_bytes_total: 0,
        output_bytes: 0,
        entries_done: 0,
        entries_total,
    });

    // 只有显式选择“当前/最近时间”时才复制源文件改 mtime；默认原始时间
    // 直接读取源路径，不产生额外磁盘占用。
    let staged_time = match options.archive_time {
        ArchiveTimeSource::Original => None,
        ArchiveTimeSource::Current => Some(SystemTime::now()),
        ArchiveTimeSource::Latest => Some(plan.latest_modified.unwrap_or_else(SystemTime::now)),
    };
    let _staged = match staged_time {
        Some(time) => Some(stage_sources(&mut files, time, &options.cancel)?),
        None => None,
    };

    let archive = path_string(output)?.to_string();
    let password = options.password.as_deref().map(String::as_str);
    let method = method_for(options.preset).to_string();
    let threads = options.threads.clamp(1, 64).to_string();
    let ranges = batch_ranges(&files);
    let mut done_bytes = 0u64;
    let mut done_files = 0usize;
    for (start, end) in ranges {
        ensure_not_cancelled(&options.cancel)?;
        let batch = &files[start..end];
        let mut args = Vec::with_capacity(batch.len() * 2 + 8);
        args.push("add".to_string());
        args.push(archive.clone());
        for file in batch {
            args.push(file.source.to_string_lossy().into_owned());
        }
        args.push("-to".to_string());
        for file in batch {
            args.push(file.archive_path.clone());
        }
        args.push("-method".to_string());
        args.push(method.clone());
        args.push("-threads".to_string());
        args.push(threads.clone());
        if let Some(password) = password {
            args.push("-key".to_string());
            args.push(password.to_string());
        }
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        command::run(&refs, password.is_some())?;

        let batch_bytes = batch.iter().map(|file| file.bytes).sum::<u64>();
        done_bytes = done_bytes.saturating_add(batch_bytes);
        done_files += batch.len();
        progress(CreateProgress {
            current: batch
                .last()
                .map(|file| file.archive_path.clone())
                .unwrap_or_default(),
            bytes_done: done_bytes,
            bytes_total: total_bytes,
            current_bytes_done: batch_bytes,
            current_bytes_total: batch_bytes,
            output_bytes: fs::metadata(output)
                .map(|metadata| metadata.len())
                .unwrap_or(0),
            entries_done: done_files,
            entries_total,
        });
    }

    validate_created(output, &seen, password)?;
    Ok(CreateReport {
        files: entries_total,
        bytes: total_bytes,
        output: output.to_path_buf(),
        cleanup_errors: Vec::new(),
    })
}

/// 把源文件复制到临时目录并统一改写修改时间。
fn stage_sources(
    files: &mut [PlannedFile],
    time: SystemTime,
    cancel: &Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<tempfile::TempDir, ArchiveError> {
    let directory = tempfile::Builder::new()
        .prefix(".lumark-zpaq-input-")
        .tempdir()?;
    for (index, file) in files.iter_mut().enumerate() {
        ensure_not_cancelled(cancel)?;
        let target = directory.path().join(format!("{index:010}.dat"));
        fs::copy(&file.source, &target)?;
        filetime::set_file_mtime(&target, filetime::FileTime::from_system_time(time))?;
        file.source = target;
    }
    Ok(directory)
}

/// 重新列出刚创建的归档，确认条目集合与计划一致。
fn validate_created(
    output: &Path,
    expected: &HashSet<String>,
    password: Option<&str>,
) -> Result<(), ArchiveError> {
    if !output.is_file() {
        return Err(ArchiveError::InvalidRequest(
            "ZPAQ 创建失败：没有生成归档文件".to_string(),
        ));
    }
    let info = super::list(output, password)?;
    let mut listed = info
        .entries
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.path.clone())
        .collect::<HashSet<_>>();
    if listed.len() != expected.len() {
        return Err(ArchiveError::InvalidRequest(format!(
            "ZPAQ 归档校验失败：计划 {} 个文件，归档内有 {} 个",
            expected.len(),
            listed.len()
        )));
    }
    for path in expected {
        if !listed.remove(path) {
            return Err(ArchiveError::InvalidRequest(format!(
                "ZPAQ 归档校验失败：缺少条目 {path}"
            )));
        }
    }
    if let Some(extra) = listed.into_iter().next() {
        return Err(ArchiveError::InvalidRequest(format!(
            "ZPAQ 归档校验失败：出现未预期条目 {extra}"
        )));
    }
    Ok(())
}

/// 把预设映射为 zpaq 数字方法。
///
/// 4/5 级压缩比更高但耗时成倍增加，当前界面不提供 ZPAQ 专用方法选择，
/// 因此体积优先只到 3 级。
fn method_for(preset: CompressionPreset) -> &'static str {
    match preset {
        CompressionPreset::Speed => "1",
        CompressionPreset::Balanced => "2",
        CompressionPreset::Size => "3",
    }
}

/// 按字节上限和文件数上限切分批次。
///
/// 单个文件超过字节上限时独占一批，避免出现无法推进的空批次。
fn batch_ranges(files: &[PlannedFile]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < files.len() {
        let mut end = start;
        let mut bytes = 0u64;
        while end < files.len() {
            let file = &files[end];
            let too_many = end - start >= ZPAQ_BATCH_FILES;
            let too_large = end > start && bytes.saturating_add(file.bytes) > ZPAQ_BATCH_BYTES;
            if too_many || too_large {
                break;
            }
            bytes = bytes.saturating_add(file.bytes);
            end += 1;
        }
        ranges.push((start, end));
        start = end;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造只关心大小的计划文件。
    fn planned(sizes: &[u64]) -> Vec<PlannedFile> {
        sizes
            .iter()
            .enumerate()
            .map(|(index, bytes)| PlannedFile {
                archive_path: format!("{index}.bin"),
                source: PathBuf::from(format!("{index}.bin")),
                bytes: *bytes,
            })
            .collect()
    }

    /// 三个预设应映射到 1/2/3 级方法。
    #[test]
    fn maps_presets_to_methods() {
        assert_eq!(method_for(CompressionPreset::Speed), "1");
        assert_eq!(method_for(CompressionPreset::Balanced), "2");
        assert_eq!(method_for(CompressionPreset::Size), "3");
    }

    /// 按文件数上限切分，并保持顺序连续。
    #[test]
    fn splits_by_file_count() {
        let files = planned(&[1; 130]);
        let ranges = batch_ranges(&files);
        assert_eq!(ranges.first(), Some(&(0, ZPAQ_BATCH_FILES)));
        assert_eq!(
            ranges.last().map(|(start, end)| (*start, *end)),
            Some((128, 130))
        );
        assert_eq!(
            ranges.iter().map(|(start, end)| end - start).sum::<usize>(),
            130
        );
    }

    /// 超过字节上限的文件独占一批，后续小文件另起一批。
    #[test]
    fn oversized_file_gets_own_batch() {
        let files = planned(&[ZPAQ_BATCH_BYTES + 1, 1, 1]);
        assert_eq!(batch_ranges(&files), vec![(0, 1), (1, 3)]);
    }
}
