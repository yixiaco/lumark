//! ZPAQ 解压：先解到输出目录内的暂存区，再按覆盖策略移动。
//!
//! 直接让引擎写用户目录无法实现“跳过/重命名”策略，也无法在预算超限时
//! 保证不留下半成品；先把选中条目解压到受控临时目录，再统一做路径安全、
//! 预算和覆盖判断，最后移动文件。

use std::fs;
use std::path::Path;

use crate::error::ArchiveError;
use crate::format::{ExtractReport, ExtractionOptions, OverwritePolicy};
use crate::path::{ArchivePath, ensure_no_symlink_components, safe_join};
use crate::service::{
    ExtractProgress, ensure_budget, ensure_not_cancelled, selected, unique_destination,
};

use super::listing::ListedEntry;
use super::{ZPAQ_MAX_FILTERS, path_string, run_with_auth};

/// 把选中条目解压到目标目录。
pub(crate) fn extract<F>(
    path: &Path,
    options: &ExtractionOptions,
    progress: &mut F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(ExtractProgress),
{
    let password = options.password.as_deref().map(String::as_str);
    let (info, entries) = super::list_with_entries(path, password)?;
    fs::create_dir_all(&options.output_dir)?;

    // 预检全部条目，而不是只检查选中项：某些情况下会退化为全量解压，
    // 且引擎在暂存区内的写入路径必须始终受控。
    for entry in &entries {
        let destination = safe_join(&options.output_dir, &entry.path)?;
        ensure_no_symlink_components(&options.output_dir, &destination)?;
    }

    let selection = normalize_selection(&options.selected_paths);
    let planned = plan_entries(options, &entries, &selection)?;
    let total_files = planned.iter().filter(|entry| !entry.is_dir).count();
    let total_bytes = planned
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.size)
        .sum::<u64>();
    progress(ExtractProgress {
        current: "正在解压 ZPAQ 归档".to_string(),
        bytes_done: 0,
        bytes_total: total_bytes,
        entries_done: 0,
        entries_total: total_files,
    });

    let staging = tempfile::Builder::new()
        .prefix(".lumark-zpaq-out-")
        .tempdir_in(&options.output_dir)?;
    let archive = path_string(path)?;
    let mut args = vec!["extract".to_string(), archive.to_string()];
    let filters = filter_paths(&entries, &selection);
    if let Some(filters) = &filters {
        args.extend(filters.iter().cloned());
        args.push("-to".to_string());
        for filter in filters {
            let target = safe_join(staging.path(), filter)?;
            args.push(path_string(&target)?.to_string());
        }
    } else {
        args.push("-to".to_string());
        args.push(path_string(staging.path())?.to_string());
    }
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_with_auth(&refs, info.encrypted, password)?;

    // 引擎返回成功后再用实际暂存大小复核预算：流式归档的声明大小未知，
    // 不能只依赖列表元数据。
    let mut actual = ExtractReport::default();
    for entry in &planned {
        if entry.is_dir {
            continue;
        }
        let staged = safe_join(staging.path(), &entry.path)?;
        let metadata = fs::metadata(&staged).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ArchiveError::Corrupt(format!("ZPAQ 解压缺少条目：{}", entry.path))
            } else {
                ArchiveError::Io(error)
            }
        })?;
        let size = metadata.len();
        ensure_budget(options, &actual, size)?;
        actual.files += 1;
        actual.bytes = actual.bytes.saturating_add(size);
    }

    let mut report = ExtractReport::default();
    let mut processed = 0usize;
    for entry in &planned {
        ensure_not_cancelled(&options.cancel)?;
        let mut destination = safe_join(&options.output_dir, &entry.path)?;
        ensure_no_symlink_components(&options.output_dir, &destination)?;
        if entry.is_dir {
            fs::create_dir_all(&destination)?;
            continue;
        }
        let staged = safe_join(staging.path(), &entry.path)?;
        if destination.exists() {
            match options.overwrite_policy {
                OverwritePolicy::Rename => destination = unique_destination(&destination),
                OverwritePolicy::Skip => {
                    report.skipped += 1;
                    processed += 1;
                    let _ = fs::remove_file(&staged);
                    continue;
                }
                OverwritePolicy::Ask if !options.overwrite => {
                    report.skipped += 1;
                    processed += 1;
                    let _ = fs::remove_file(&staged);
                    continue;
                }
                _ => {}
            }
        }
        let size = fs::metadata(&staged)?.len();
        ensure_budget(options, &report, size)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        move_file(&staged, &destination)?;
        report.files += 1;
        report.bytes = report.bytes.saturating_add(size);
        processed += 1;
        progress(ExtractProgress {
            current: entry.path.clone(),
            bytes_done: report.bytes,
            bytes_total: actual.bytes,
            entries_done: processed,
            entries_total: total_files,
        });
    }
    Ok(report)
}

/// 预检选中条目的预算，并返回需要移动的条目。
fn plan_entries<'a>(
    options: &ExtractionOptions,
    entries: &'a [ListedEntry],
    selection: &[String],
) -> Result<Vec<&'a ListedEntry>, ArchiveError> {
    let mut checked = ExtractReport::default();
    let mut planned = Vec::new();
    for entry in entries {
        if !selected(&entry.path, selection) {
            continue;
        }
        if !entry.is_dir {
            ensure_budget(options, &checked, entry.size)?;
            checked.files += 1;
            checked.bytes = checked.bytes.saturating_add(entry.size);
        }
        planned.push(entry);
    }
    Ok(planned)
}

/// 归一化选择项：去重、去掉被父目录覆盖的子项，并统一使用 `/`。
fn normalize_selection(paths: &[String]) -> Vec<String> {
    let mut selection: Vec<String> = Vec::new();
    for path in paths {
        let path = path.replace('\\', "/");
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            continue;
        }
        let covered = selection
            .iter()
            .any(|parent| path == parent || path.starts_with(&format!("{parent}/")));
        if covered {
            continue;
        }
        selection.retain(|child| !child.starts_with(&format!("{path}/")));
        if !selection.iter().any(|item| item == path) {
            selection.push(path.to_string());
        }
    }
    selection
}

/// 判断能否把选择项安全地传给引擎的过滤参数。
///
/// 以下情况退回“全量解压后只移动选中项”：选择过多会让引擎的前缀扫描
/// 退化；归档内存在反斜杠路径时前缀匹配不可靠；以 `-` 开头的名字会被
/// 引擎当成选项。
fn filter_paths(entries: &[ListedEntry], selection: &[String]) -> Option<Vec<String>> {
    if selection.is_empty() || selection.len() > ZPAQ_MAX_FILTERS {
        return None;
    }
    if selection.iter().any(|path| path.starts_with('-')) {
        return None;
    }
    if entries.iter().any(|entry| entry.raw_path.contains('\\')) {
        return None;
    }
    if selection
        .iter()
        .any(|path| ArchivePath::parse(path).is_err())
    {
        return None;
    }
    Some(selection.to_vec())
}

/// 优先在同卷内重命名；跨卷时退回复制并保留修改时间。
fn move_file(from: &Path, to: &Path) -> Result<(), ArchiveError> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(rename_error) => match fs::copy(from, to) {
            Ok(_) => {
                if let Ok(metadata) = fs::metadata(from) {
                    let _ = filetime::set_file_mtime(
                        to,
                        filetime::FileTime::from_last_modification_time(&metadata),
                    );
                }
                fs::remove_file(from)?;
                Ok(())
            }
            Err(_) => Err(ArchiveError::Io(rename_error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 父目录选择应覆盖子项，重复项只保留一次。
    #[test]
    fn normalizes_nested_selection() {
        let paths = vec![
            "dir/".to_string(),
            "dir/a.txt".to_string(),
            r"dir\b.txt".to_string(),
            "other.txt".to_string(),
            "other.txt".to_string(),
        ];
        assert_eq!(
            normalize_selection(&paths),
            vec!["dir".to_string(), "other.txt".to_string()]
        );
    }

    /// 选择项覆盖反斜杠路径时应归一化后再比较。
    #[test]
    fn normalizes_backslash_selection() {
        assert_eq!(
            normalize_selection(&[r"dir\file.txt".to_string()]),
            vec!["dir/file.txt".to_string()]
        );
    }
}
