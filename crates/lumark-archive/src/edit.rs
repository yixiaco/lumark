use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sevenz_rust2::{ArchiveReader, ArchiveWriter, EncoderConfiguration, Password};
use zeroize::Zeroizing;
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

use crate::error::ArchiveError;
use crate::format::ArchiveFormat;
use crate::service::ArchiveService;

#[derive(Debug, Clone, Default)]
pub struct EditOptions {
    pub source_password: Option<Zeroizing<String>>,
    pub output_password: Option<Zeroizing<String>>,
    pub delete_paths: Vec<String>,
    pub rename: Vec<(String, String)>,
    pub additions: Vec<(String, PathBuf)>,
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
}

impl ArchiveService {
    /// Rewrites a ZIP/7Z archive into a same-directory temporary file and
    /// atomically replaces the original only after the result validates.
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
            ArchiveFormat::Zip => write_zip(path, &temp, options, &mut progress),
            ArchiveFormat::SevenZip => write_seven_zip(path, &temp, options, &mut progress),
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
        })
    }
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
