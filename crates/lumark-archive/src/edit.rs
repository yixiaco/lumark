use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use sevenz_rust2::{ArchiveReader, ArchiveWriter, EncoderConfiguration, Password};
use walkdir::WalkDir;
use zeroize::Zeroizing;
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

use crate::error::ArchiveError;
use crate::format::{ArchiveEntry, ArchiveFormat, UpdateMode};
use crate::service::ArchiveService;

/// 要同步到归档的目录根。
///
/// `prefix` 是归档内的目录名（空表示归档根），`directory` 是本地源目录。
/// 同步模式会按该目录的内容新增、更新条目，并删除磁盘上已不存在的条目。
#[derive(Debug, Clone)]
pub struct EditRoot {
    pub prefix: String,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct EditOptions {
    pub source_password: Option<Zeroizing<String>>,
    pub output_password: Option<Zeroizing<String>>,
    pub delete_paths: Vec<String>,
    pub rename: Vec<(String, String)>,
    pub additions: Vec<(String, PathBuf)>,
    /// 需要展开或同步的目录根；文件添加仍使用 `additions`。
    pub roots: Vec<EditRoot>,
    /// 同名条目的替换规则，默认整体替换。
    pub update_mode: UpdateMode,
    pub comment: Option<String>,
    pub keep_backup: bool,
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Clone, Default)]
pub struct EditProgress {
    pub current: String,
    pub entries_done: usize,
    pub entries_total: usize,
    pub bytes_done: u64,
}

#[derive(Debug, Clone)]
pub struct EditReport {
    pub files: usize,
    pub output: PathBuf,
    pub backup: Option<PathBuf>,
    /// 因更新模式判定为未变化而跳过的添加文件数。
    pub skipped: usize,
    /// 同步模式从归档删除的条目数。
    pub deleted: usize,
}

impl ArchiveService {
    /// 把 ZIP/7Z 归档重写到同目录临时文件，并仅在结果校验通过后原子替换原文件。
    pub fn edit<F>(
        path: &Path,
        options: &EditOptions,
        mut progress: F,
    ) -> Result<EditReport, ArchiveError>
    where
        F: FnMut(EditProgress),
    {
        ensure_not_cancelled(&options.cancel)?;
        let format = Self::detect(path);
        if crate::service::is_volume_path(path) {
            return Err(ArchiveError::Corrupt(
                "分卷归档暂不支持原地编辑，请先合并为单文件".into(),
            ));
        }
        if format == ArchiveFormat::Disguised {
            return Err(ArchiveError::InvalidRequest(
                "伪装压缩包请在还原后编辑".into(),
            ));
        }
        if !matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZip) {
            return Err(ArchiveError::UnsupportedFormat(format.label().to_string()));
        }
        let mut addition_names = HashSet::new();
        for (name, _) in &options.additions {
            if name.trim().is_empty() {
                return Err(ArchiveError::Corrupt("添加文件名称不能为空".into()));
            }
            if !addition_names.insert(name.clone()) {
                return Err(ArchiveError::Corrupt(format!("添加文件名称重复：{name}")));
            }
        }
        // 展开目录根并按更新模式过滤；替换模式且没有目录根时不必枚举归档。
        let plan = plan_additions(path, options)?;
        let deleted = plan.deletions.len();
        let skipped = plan.skipped;
        let mut options = options.clone();
        options.additions = plan.additions;
        options.delete_paths.extend(plan.deletions);
        options.roots.clear();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("archive");
        let temp = parent.join(format!(
            ".lumark-edit-{}-{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default(),
            extension
        ));

        let result = match format {
            ArchiveFormat::Zip => write_zip(path, &temp, &options, &mut progress),
            ArchiveFormat::SevenZip => write_seven_zip(path, &temp, &options, &mut progress),
            _ => unreachable!(),
        };
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }

        let verify = Self::list(
            &temp,
            options.output_password.as_deref().map(String::as_str),
        );
        if let Err(error) = verify {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }

        let backup = options.keep_backup.then(|| {
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("archive");
            parent.join(format!("{file_name}.lumark.bak"))
        });
        if let Err(error) = atomic_replace(path, &temp, backup.as_deref()) {
            let _ = fs::remove_file(&temp);
            return Err(ArchiveError::Io(error));
        }
        Ok(EditReport {
            files: verify.unwrap_or_default().entries.len(),
            output: path.to_path_buf(),
            backup,
            skipped,
            deleted,
        })
    }
}

/// 一次归档编辑的准备结果：要写入的文件、跳过的数量和需要删除的条目。
struct EditPlan {
    additions: Vec<(String, PathBuf)>,
    skipped: usize,
    deletions: Vec<String>,
}

/// 展开目录根并按更新模式生成添加项、跳过数和同步删除列表。
///
/// 显式添加项优先于目录展开结果，避免同一文件被同一批次写两次；同步模式
/// 在枚举归档条目后，把磁盘上已不存在的条目加入删除列表。
fn plan_additions(archive: &Path, options: &EditOptions) -> Result<EditPlan, ArchiveError> {
    let mut additions = options.additions.clone();
    let mut names: HashSet<String> = additions.iter().map(|(name, _)| name.clone()).collect();
    for (name, source) in expand_roots(&options.roots)? {
        if names.insert(name.clone()) {
            additions.push((name, source));
        }
    }
    let needs_entries = options.update_mode != UpdateMode::Replace || !options.roots.is_empty();
    if !needs_entries {
        return Ok(EditPlan {
            additions,
            skipped: 0,
            deletions: Vec::new(),
        });
    }
    let info = ArchiveService::list(
        archive,
        options.source_password.as_deref().map(String::as_str),
    )?;
    let existing: HashMap<&str, Option<DateTime<Utc>>> = info
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.modified))
        .collect();
    let mode = options.update_mode;
    let mut kept = Vec::new();
    let mut skipped = 0;
    for (name, source) in additions {
        let Some(archived) = existing.get(name.as_str()) else {
            // 仅刷新模式不引入新条目；替换、更新和同步都会加入新文件。
            if mode == UpdateMode::Freshen {
                skipped += 1;
            } else {
                kept.push((name, source));
            }
            continue;
        };
        if mode == UpdateMode::Freshen || source_is_newer(*archived, &source) {
            kept.push((name, source));
        } else {
            skipped += 1;
        }
    }
    let deletions = if mode == UpdateMode::Sync {
        sync_deletions(&options.roots, &info.entries)
    } else {
        Vec::new()
    };
    Ok(EditPlan {
        additions: kept,
        skipped,
        deletions,
    })
}

/// 把目录根展开成归档内的文件名列表。
///
/// 归档名使用 `/` 分隔，目录根的名称作为前缀保留，与压缩时“保留根目录”
/// 的命名一致；符号链接不跟随，避免同步遍历到归档之外。
fn expand_roots(roots: &[EditRoot]) -> Result<Vec<(String, PathBuf)>, ArchiveError> {
    let mut files = Vec::new();
    for root in roots {
        if !root.directory.is_dir() {
            return Err(ArchiveError::Corrupt(format!(
                "同步源目录不存在：{}",
                root.directory.display()
            )));
        }
        let prefix = root.prefix.trim_end_matches('/');
        for entry in WalkDir::new(&root.directory).follow_links(false) {
            let entry = entry.map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            if entry.file_type().is_dir() {
                continue;
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(&root.directory)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let name = if prefix.is_empty() {
                relative
            } else {
                format!("{prefix}/{relative}")
            };
            files.push((name, path.to_path_buf()));
        }
    }
    Ok(files)
}

/// 计算同步模式需要从归档删除的条目。
///
/// 只处理目录根前缀下的条目；把归档内的 POSIX 相对路径逐级拼到本地目录
/// 后再判断存在性，避免分隔符和大小写差异造成误删。
fn sync_deletions(roots: &[EditRoot], entries: &[ArchiveEntry]) -> Vec<String> {
    let mut deleted = Vec::new();
    for root in roots {
        let prefix = root.prefix.trim_end_matches('/');
        for entry in entries {
            let relative = if prefix.is_empty() {
                entry.path.as_str()
            } else if entry.path == prefix {
                ""
            } else if let Some(rest) = entry.path.strip_prefix(&format!("{prefix}/")) {
                rest
            } else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            let mut disk = root.directory.clone();
            for part in relative.split('/') {
                disk.push(part);
            }
            if !disk.exists() {
                deleted.push(entry.path.clone());
            }
        }
    }
    deleted.sort();
    deleted.dedup();
    deleted
}

/// 判断磁盘文件是否比归档条目更新。
///
/// ZIP 的 DOS 时间只有 2 秒精度，比较时留出容差，避免把同一文件的微秒
/// 差异当成更新；任一侧缺少时间戳时按“需要更新”返回，宁可多写一次也不
/// 静默保留旧内容。
fn source_is_newer(archived: Option<DateTime<Utc>>, source: &Path) -> bool {
    let Ok(disk) = fs::metadata(source).and_then(|metadata| metadata.modified()) else {
        return true;
    };
    let Some(archived) = archived else {
        return true;
    };
    DateTime::<Utc>::from(disk) > archived + chrono::Duration::seconds(2)
}

fn write_zip<F>(
    source: &Path,
    output: &Path,
    options: &EditOptions,
    progress: &mut F,
) -> Result<(), ArchiveError>
where
    F: FnMut(EditProgress),
{
    let file = File::open(source)?;
    let mut archive = ZipArchive::new(file)?;
    let entries_total = archive.len() + options.additions.len();
    let output_file = File::create(output)?;
    let mut writer = ZipWriter::new(output_file);
    let addition_names: HashSet<&str> = options
        .additions
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    if let Some(comment) = &options.comment {
        writer.set_comment(comment.as_str())?;
    } else if !archive.comment().is_empty() {
        writer.set_comment(String::from_utf8_lossy(archive.comment()).into_owned())?;
    }
    let mut done = 0;
    let mut bytes_done = 0u64;
    for index in 0..archive.len() {
        ensure_not_cancelled(&options.cancel)?;
        let raw = archive.by_index_raw(index)?;
        let old_name = raw
            .name()
            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
            .to_string();
        if is_deleted(&old_name, &options.delete_paths) {
            continue;
        }
        let new_name = rename_for(&old_name, &options.rename);
        if addition_names.contains(new_name.as_str()) {
            done += 1;
            continue;
        }
        let is_dir = raw.is_dir();
        let method = raw.compression();
        let modified = raw.last_modified();
        drop(raw);

        let mut file_options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(archive.by_index_raw(index)?.size() > u32::MAX as u64);
        if let Some(modified) = modified {
            file_options = file_options.last_modified_time(modified);
        }
        if let Some(password) = options.output_password.as_deref() {
            file_options = file_options.with_aes_encryption(AesMode::Aes256, password);
        }
        if is_dir {
            writer.add_directory(new_name.clone(), file_options)?;
        } else {
            writer.start_file(new_name.clone(), file_options)?;
            let mut input = match options.source_password.as_deref() {
                Some(password) => archive.by_index_decrypt(index, password.as_bytes())?,
                None => archive.by_index(index)?,
            };
            bytes_done += copy_cancellable_edit(&mut input, &mut writer, &options.cancel)?;
        }
        done += 1;
        progress(EditProgress {
            current: new_name,
            entries_done: done,
            entries_total,
            bytes_done,
        });
    }

    for (name, source) in &options.additions {
        ensure_not_cancelled(&options.cancel)?;
        let mut input = File::open(source)?;
        let mut file_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .large_file(fs::metadata(source)?.len() > u32::MAX as u64);
        if let Some(password) = options.output_password.as_deref() {
            file_options = file_options.with_aes_encryption(AesMode::Aes256, password);
        }
        writer.start_file(name, file_options)?;
        bytes_done += copy_cancellable_edit(&mut input, &mut writer, &options.cancel)?;
        done += 1;
        progress(EditProgress {
            current: name.clone(),
            entries_done: done,
            entries_total,
            bytes_done,
        });
    }
    writer.finish()?;
    Ok(())
}

fn write_seven_zip<F>(
    source: &Path,
    output: &Path,
    options: &EditOptions,
    progress: &mut F,
) -> Result<(), ArchiveError>
where
    F: FnMut(EditProgress),
{
    let source_password = options
        .source_password
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let mut reader = ArchiveReader::open(source, Password::from(source_password))
        .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
    let entries_total = reader.archive().files.len() + options.additions.len();
    let mut writer =
        ArchiveWriter::create(output).map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
    let addition_names: HashSet<&str> = options
        .additions
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let mut methods = vec![EncoderConfiguration::new(
        sevenz_rust2::EncoderMethod::LZMA2,
    )];
    if let Some(password) = options.output_password.as_deref() {
        methods.push(
            sevenz_rust2::encoder_options::AesEncoderOptions::new(Password::from(
                password.as_str(),
            ))
            .into(),
        );
    }
    writer.set_content_methods(methods);
    writer.set_encrypt_header(options.output_password.is_some());
    let mut done = 0usize;
    let mut bytes_done = 0u64;
    let scan_result = reader.for_each_entries(|entry, input| {
        if is_cancelled(&options.cancel) {
            return Err(sevenz_rust2::Error::Other("cancelled".into()));
        }
        let old_name = entry.name().to_string();
        if is_deleted(&old_name, &options.delete_paths) {
            if entry.has_stream() && !entry.is_directory() {
                io::copy(input, &mut io::sink())
                    .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
            }
            return Ok(true);
        }
        let mut updated = entry.clone();
        updated.name = rename_for(&old_name, &options.rename);
        if addition_names.contains(updated.name.as_str()) {
            if entry.has_stream() && !entry.is_directory() {
                io::copy(input, &mut io::sink())
                    .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
            }
            done += 1;
            return Ok(true);
        }
        if entry.is_directory() || !entry.has_stream() {
            writer
                .push_archive_entry(updated, None::<&mut dyn Read>)
                .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
        } else {
            writer
                .push_archive_entry(updated, Some(input))
                .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
            bytes_done += entry.size();
        }
        done += 1;
        progress(EditProgress {
            current: rename_for(&old_name, &options.rename),
            entries_done: done,
            entries_total,
            bytes_done,
        });
        Ok(true)
    });
    if let Err(error) = scan_result {
        if is_cancelled(&options.cancel) {
            return Err(ArchiveError::Cancelled);
        }
        return Err(ArchiveError::SevenZ(error.to_string()));
    }
    ensure_not_cancelled(&options.cancel)?;

    for (name, source) in &options.additions {
        ensure_not_cancelled(&options.cancel)?;
        let entry = sevenz_rust2::ArchiveEntry::from_path(source, name.clone());
        let mut input = File::open(source)?;
        writer
            .push_archive_entry(entry, Some(&mut input))
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        done += 1;
        bytes_done += fs::metadata(source)?.len();
        progress(EditProgress {
            current: name.clone(),
            entries_done: done,
            entries_total,
            bytes_done,
        });
    }
    writer
        .finish()
        .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
    Ok(())
}

fn is_cancelled(flag: &Option<Arc<AtomicBool>>) -> bool {
    flag.as_ref()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn ensure_not_cancelled(flag: &Option<Arc<AtomicBool>>) -> Result<(), ArchiveError> {
    if is_cancelled(flag) {
        Err(ArchiveError::Cancelled)
    } else {
        Ok(())
    }
}

fn copy_cancellable_edit<R: Read + ?Sized, W: std::io::Write>(
    input: &mut R,
    output: &mut W,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        ensure_not_cancelled(cancel)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        total += read as u64;
    }
    Ok(total)
}

fn rename_for(name: &str, rename: &[(String, String)]) -> String {
    rename
        .iter()
        .find(|(old, _)| old == name)
        .map(|(_, new)| new.clone())
        .unwrap_or_else(|| name.to_string())
}

fn is_deleted(name: &str, deleted: &[String]) -> bool {
    deleted.iter().any(|item| {
        let item = item.replace('\\', "/");
        name == item || name.starts_with(&format!("{item}/"))
    })
}

#[cfg(windows)]
fn atomic_replace(path: &Path, temp: &Path, backup: Option<&Path>) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let path_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let temp_wide: Vec<u16> = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let backup_wide = backup.map(|backup| {
        backup
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    });
    let ok = unsafe {
        ReplaceFileW(
            path_wide.as_ptr(),
            temp_wide.as_ptr(),
            backup_wide
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
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

#[cfg(not(windows))]
fn atomic_replace(path: &Path, temp: &Path, backup: Option<&Path>) -> io::Result<()> {
    if let Some(backup) = backup {
        fs::copy(path, backup)?;
    }
    fs::rename(temp, path)
}
