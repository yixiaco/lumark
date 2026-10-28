use super::*;

/// 递归统计文件夹内普通文件的总字节数。
///
/// 跳过符号链接以避免目录环和重复计数；无权限读取的子项直接跳过，
/// 因此计算总会返回一个可展示的近似值而不是让界面长时间无反馈。
pub(super) fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            total += directory_size(&entry.path());
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
        }
    }
    total
}

/// 把归档解压进度换算为弹窗进度条百分比。
///
/// 优先按已处理字节统计；条目数只用于文本展示，否则单个大文件会长时间
/// 停在 0%，多个大小不一的文件又会随条目跳变。
pub(super) fn extract_progress_percent(progress: &lumark_archive::ExtractProgress) -> f32 {
    if progress.bytes_total > 0 {
        progress.bytes_done as f32 * 100. / progress.bytes_total as f32
    } else if progress.entries_total > 0 {
        progress.entries_done as f32 * 100. / progress.entries_total as f32
    } else {
        0.
    }
}

/// 生成临时解压弹窗的状态文本。
pub(super) fn extract_progress_message(progress: &lumark_archive::ExtractProgress) -> String {
    if progress.entries_total > 0 {
        format!(
            "正在解压 {}/{}：{}",
            progress.entries_done, progress.entries_total, progress.current
        )
    } else if !progress.current.is_empty() {
        format!("正在解压：{}", progress.current)
    } else {
        "正在解压…".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::extract_progress_percent;
    use lumark_archive::ExtractProgress;

    /// 解压百分比优先按字节，单个大文件不会一直停在 0%。
    #[test]
    fn extract_percent_prefers_bytes() {
        let progress = ExtractProgress {
            current: "large.bin".to_string(),
            bytes_done: 50,
            bytes_total: 100,
            entries_done: 0,
            entries_total: 1,
        };
        assert!((extract_progress_percent(&progress) - 50.0).abs() < f32::EPSILON);
    }
}
