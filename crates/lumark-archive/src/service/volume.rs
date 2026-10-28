//! 分卷路径、拆分、提交和原子替换辅助。

use super::*;

/// 把文件名拆成主名和完整扩展名，识别 `.tar.gz` 这类复合扩展名。
///
/// 返回 `(stem, extension)`；没有扩展名时 `extension` 为 `None`。冲突重命名
/// 必须保留完整扩展名，否则 `cover.tar.gz` 会变成 `cover.tar (1).gz`，
/// 后续格式识别会把 TAR 变体误判成单文件流。
pub(super) fn split_compound_extension(name: &str) -> (&str, Option<&str>) {
    /// 需要整体保留的复合扩展名。
    const COMPOUND: [&str; 8] = [
        ".tar.lzma",
        ".tar.bz2",
        ".tar.zst",
        ".tar.lz4",
        ".tar.gz",
        ".tar.br",
        ".tar.lz",
        ".tar.xz",
    ];
    let lower = name.to_ascii_lowercase();
    for suffix in COMPOUND {
        if lower.ends_with(suffix) {
            let stem_len = name.len() - suffix.len();
            if stem_len > 0 {
                return (&name[..stem_len], Some(&name[stem_len + 1..]));
            }
        }
    }
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            (stem, Some(extension))
        }
        _ => (name, None),
    }
}

/// 为目标路径生成不冲突的同名文件路径。
pub(crate) fn unique_destination(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return path.to_path_buf();
    };
    let (stem, extension) = split_compound_extension(name);
    for index in 1..10_000 {
        let candidate_name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

/// 去掉 .001 等分卷后缀，得到逻辑归档名。
pub(super) fn strip_volume_suffix(path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return path.to_path_buf();
    };
    let Some((base, suffix)) = name.rsplit_once('.') else {
        return path.to_path_buf();
    };
    if suffix.len() == 3 && suffix.chars().all(|value| value.is_ascii_digit()) {
        path.with_file_name(base)
    } else {
        path.to_path_buf()
    }
}

/// 判断路径是否为分卷成员。
pub(crate) fn is_volume_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, suffix)| suffix == "001")
}

/// 按顺序列出同一归档的全部分卷。
pub(super) fn volume_paths(path: &Path) -> Result<Option<Vec<PathBuf>>, ArchiveError> {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    let Some((base, suffix)) = name.rsplit_once('.') else {
        return Ok(None);
    };
    if suffix != "001" {
        return Ok(None);
    }
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let prefix = format!("{base}.");
    let mut volumes = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let Some(file_name) = entry.file_name().to_str().map(ToString::to_string) else {
            continue;
        };
        let Some(number) = file_name
            .strip_prefix(&prefix)
            .filter(|number| number.len() == 3)
            .and_then(|number| number.parse::<u32>().ok())
        else {
            continue;
        };
        if number > 0 {
            volumes.push((number, entry.path()));
        }
    }
    volumes.sort_by_key(|(number, _)| *number);
    if volumes.first().is_none_or(|(number, _)| *number != 1) {
        return Ok(None);
    }
    Ok(Some(volumes.into_iter().map(|(_, path)| path).collect()))
}

/// 为只接受路径的后端准备物化输入和清理守卫。
pub(super) fn materialize_input(
    path: &Path,
) -> Result<(PathBuf, Option<tempfile::TempDir>), ArchiveError> {
    let Some(volumes) = volume_paths(path)? else {
        return Ok((path.to_path_buf(), None));
    };
    let directory = tempfile::tempdir()?;
    let base = strip_volume_suffix(path);
    let name = base
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive.bin");
    let joined = directory.path().join(name);
    let mut output = File::create(&joined)?;
    for volume in volumes {
        let mut input = File::open(volume)?;
        io::copy(&mut input, &mut output)?;
    }
    output.sync_all()?;
    Ok((joined, Some(directory)))
}

/// 确认删除源文件不会误删输出或工作目录。
pub(super) fn ensure_sources_safe_to_delete(
    sources: &[PathBuf],
    output: &Path,
) -> Result<(), ArchiveError> {
    let output_abs = absolute_for_comparison(output)
        .ok_or_else(|| ArchiveError::Corrupt("无法确认输出归档路径".into()))?;
    for source in sources {
        let source_abs = absolute_for_comparison(source).ok_or_else(|| {
            ArchiveError::Corrupt(format!("无法确认源路径：{}", source.display()))
        })?;
        if source_abs == output_abs || (source_abs.is_dir() && output_abs.starts_with(&source_abs))
        {
            return Err(ArchiveError::Corrupt(
                "输出归档位于待删除的源路径中，已取消删除源文件操作".into(),
            ));
        }
    }
    Ok(())
}

/// 返回用于路径比较的绝对规范化路径。
pub(super) fn absolute_for_comparison(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Some(canonical);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).ok()?;
    Some(parent.join(path.file_name()?))
}

/// 删除源路径并记录失败原因。
pub(super) fn remove_source_paths(sources: &[PathBuf], errors: &mut Vec<String>) {
    for source in sources {
        let result = if source.is_dir() {
            fs::remove_dir_all(source)
        } else {
            fs::remove_file(source)
        };
        if let Err(error) = result {
            errors.push(format!("{}: {error}", source.display()));
        }
    }
}

/// 按目标大小把归档拆成 .001 分卷。
pub(super) fn split_file(
    source: &Path,
    base: &Path,
    split_size: u64,
) -> Result<Vec<PathBuf>, ArchiveError> {
    let mut input = File::open(source)?;
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let name = base
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    let mut remaining = input.metadata()?.len();
    if remaining == 0 {
        return Err(ArchiveError::Corrupt("不能拆分空归档".into()));
    }
    let volume_count = remaining.div_ceil(split_size);
    if volume_count > 999 {
        return Err(ArchiveError::Corrupt("分卷数量超过 999".into()));
    }
    let targets: Vec<PathBuf> = (1..=volume_count)
        .map(|index| parent.join(format!("{name}.{index:03}")))
        .collect();
    for target in &targets {
        if target.exists() && !target.is_file() {
            return Err(ArchiveError::Corrupt(format!(
                "分卷目标不是普通文件：{}",
                target.display()
            )));
        }
    }

    let token = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    );
    let mut staged = Vec::with_capacity(targets.len());
    let result = (|| {
        for index in 1..=volume_count {
            let destination = parent.join(format!(".lumark-split-{token}-{index:03}"));
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            staged.push(destination);
            let chunk = remaining.min(split_size);
            let copied = io::copy(&mut (&mut input).take(chunk), &mut output)?;
            if copied != chunk {
                return Err(ArchiveError::Corrupt("分卷写入不完整".into()));
            }
            output.sync_all()?;
            remaining = remaining.saturating_sub(copied);
            if remaining == 0 {
                break;
            }
        }
        if remaining != 0 || staged.len() != targets.len() {
            return Err(ArchiveError::Corrupt("分卷写入未完成".into()));
        }
        commit_split_volumes(&staged, &targets, parent, &token)?;
        Ok(())
    })();
    if let Err(error) = result {
        for path in &staged {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(targets)
}

/// 把临时分卷提交到目标位置并清理旧卷。
pub(super) fn commit_split_volumes(
    staged: &[PathBuf],
    targets: &[PathBuf],
    parent: &Path,
    token: &str,
) -> Result<(), ArchiveError> {
    let Some(first_target) = targets.first() else {
        return Ok(());
    };
    let existing = sibling_volume_paths(first_target)?;
    for path in &existing {
        if !path.is_file() {
            return Err(ArchiveError::Corrupt(format!(
                "分卷目标不是普通文件：{}",
                path.display()
            )));
        }
    }

    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (index, target) in existing.iter().enumerate() {
        let backup = parent.join(format!(".lumark-split-backup-{token}-{index:03}"));
        if let Err(error) = fs::rename(target, &backup) {
            restore_split_backups(&backups);
            return Err(ArchiveError::Io(error));
        }
        backups.push((target.clone(), backup));
    }

    let mut committed = Vec::with_capacity(staged.len());
    for (source, target) in staged.iter().zip(targets) {
        if let Err(error) = fs::rename(source, target) {
            for path in &committed {
                let _ = fs::remove_file(path);
            }
            restore_split_backups(&backups);
            return Err(ArchiveError::Io(error));
        }
        committed.push(target.clone());
    }
    for (_, backup) in backups {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

/// 提交失败时恢复原分卷。
pub(super) fn restore_split_backups(backups: &[(PathBuf, PathBuf)]) {
    for (target, backup) in backups.iter().rev() {
        let _ = fs::rename(backup, target);
    }
}

/// 列出目标归档同目录的现有分卷。
pub(super) fn sibling_volume_paths(path: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return Ok(Vec::new());
    };
    let Some((base, suffix)) = name.rsplit_once('.') else {
        return Ok(Vec::new());
    };
    if suffix.len() != 3 || !suffix.chars().all(|value| value.is_ascii_digit()) {
        return Ok(Vec::new());
    }
    let prefix = format!("{base}.");
    let mut volumes = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let Some(file_name) = entry.file_name().to_str().map(ToString::to_string) else {
            continue;
        };
        let Some(number) = file_name
            .strip_prefix(&prefix)
            .filter(|number| number.len() == 3)
            .and_then(|number| number.parse::<u32>().ok())
        else {
            continue;
        };
        if number > 0 {
            volumes.push((number, entry.path()));
        }
    }
    volumes.sort_by_key(|(number, _)| *number);
    Ok(volumes.into_iter().map(|(_, path)| path).collect())
}

/// 原子替换文件，失败时保留旧文件。
#[cfg(windows)]
pub(crate) fn atomic_replace_file(temp: &Path, output: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    if !output.exists() {
        return fs::rename(temp, output);
    }
    let output_wide: Vec<u16> = output
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let temp_wide: Vec<u16> = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let ok = unsafe {
        ReplaceFileW(
            output_wide.as_ptr(),
            temp_wide.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// 原子替换文件，失败时保留旧文件。
#[cfg(not(windows))]
pub(crate) fn atomic_replace_file(temp: &Path, output: &Path) -> io::Result<()> {
    fs::rename(temp, output)
}
