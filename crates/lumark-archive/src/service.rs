use std::cell::{Cell, RefCell};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use geezipx_core::archive::ArchiveReader as LegacyReader;
use sevenz_rust2::encoder_options::{Bzip2Options, Lzma2Options, LzmaOptions, PpmdOptions};
use sevenz_rust2::{
    ArchiveReader, ArchiveWriter, EncoderConfiguration, NtTime, Password, SourceReader,
};
use unrar::Archive as UnrarArchive;
use walkdir::WalkDir;
use zeroize::Zeroizing;
use zip::unstable::write::FileOptionsExt;
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

use crate::error::ArchiveError;
use crate::format::{
    ArchiveEntry, ArchiveFormat, ArchiveInfo, CompressionOptions, CreateReport, EncryptionInfo,
    ExtractReport, ExtractionOptions, ZipEncryption, detect_format, detect_magic,
};
use crate::path::{ensure_no_symlink_components, safe_join};

/// 自解压包尾部页脚的魔数，用于识别被拼接的运行时模块。
pub const SFX_MAGIC: [u8; 8] = *b"LMSFX001";

/// 自解压包尾部页脚长度：魔数 8 字节 + 数据段偏移 8 字节 + 数据段长度 8 字节。
pub const SFX_FOOTER_LEN: usize = 24;

#[derive(Debug, Clone)]
pub struct ExtractProgress {
    pub current: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub entries_done: usize,
    pub entries_total: usize,
}

#[derive(Debug, Clone)]
pub struct CreateProgress {
    pub current: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_bytes_done: u64,
    pub current_bytes_total: u64,
    pub output_bytes: u64,
    pub entries_done: usize,
    pub entries_total: usize,
}

pub struct ArchiveService;

impl ArchiveService {
    pub fn detect(path: &Path) -> ArchiveFormat {
        let base = strip_volume_suffix(path);
        let extension_format = detect_format(&base);
        if matches!(
            extension_format,
            ArchiveFormat::Tar
                | ArchiveFormat::TarGz
                | ArchiveFormat::TarBz2
                | ArchiveFormat::TarXz
                | ArchiveFormat::TarZst
        ) {
            return extension_format;
        }
        let mut header = [0u8; 512];
        if let Ok(mut file) = File::open(&base)
            && let Ok(read) = file.read(&mut header)
        {
            let magic = detect_magic(&header[..read]);
            if magic != ArchiveFormat::Unknown {
                return magic;
            }
        }
        extension_format
    }

    pub fn probe_encryption(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
        let (input, _guard) = materialize_input(path)?;
        Self::probe_encryption_direct(&input)
    }

    fn probe_encryption_direct(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
        let format = Self::detect(path);
        match format {
            ArchiveFormat::Zip => {
                let file = File::open(path)?;
                let mut archive = ZipArchive::new(file)?;
                let mut encrypted = false;
                for index in 0..archive.len() {
                    let entry = archive.by_index_raw(index)?;
                    encrypted |= entry.encrypted();
                }
                Ok(EncryptionInfo {
                    encrypted,
                    header_encrypted: false,
                    format: Some(format),
                })
            }
            ArchiveFormat::SevenZip => {
                let reader = ArchiveReader::open(path, Password::empty());
                let (header_encrypted, encrypted) = match reader {
                    Ok(reader) => {
                        let has_aes = reader
                            .archive()
                            .blocks
                            .iter()
                            .flat_map(|block| block.coders.iter())
                            .any(|coder| {
                                coder.encoder_method_id()
                                    == sevenz_rust2::EncoderMethod::ID_AES256_SHA256
                            });
                        (false, has_aes)
                    }
                    Err(_) => (true, true),
                };
                Ok(EncryptionInfo {
                    encrypted,
                    header_encrypted,
                    format: Some(format),
                })
            }
            ArchiveFormat::Rar => Self::probe_rar(path),
            ArchiveFormat::Enc => Ok(EncryptionInfo {
                encrypted: true,
                header_encrypted: false,
                format: Some(format),
            }),
            _ => Ok(EncryptionInfo {
                encrypted: false,
                header_encrypted: false,
                format: Some(format),
            }),
        }
    }

    pub fn list(path: &Path, password: Option<&str>) -> Result<ArchiveInfo, ArchiveError> {
        let (input, _guard) = materialize_input(path)?;
        Self::list_direct(&input, password)
    }

    fn list_direct(path: &Path, password: Option<&str>) -> Result<ArchiveInfo, ArchiveError> {
        let format = Self::detect(path);
        match format {
            ArchiveFormat::Zip => Self::list_zip(path),
            ArchiveFormat::SevenZip => Self::list_seven_zip(path, password.unwrap_or_default()),
            ArchiveFormat::Rar => Self::list_rar(path, password),
            format if is_legacy_archive(format) => Self::list_legacy(path, format),
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst => Self::list_tar(path, format),
            ArchiveFormat::Gzip
            | ArchiveFormat::Bzip2
            | ArchiveFormat::Xz
            | ArchiveFormat::Zstd
            | ArchiveFormat::Brotli
            | ArchiveFormat::Lz4
            | ArchiveFormat::Isz
            | ArchiveFormat::Img
            | ArchiveFormat::Bin
            | ArchiveFormat::Lzip
            | ArchiveFormat::Uu => Self::list_single_stream(path, format),
            ArchiveFormat::UnixCompress => Self::list_unix_compress(path),
            ArchiveFormat::Enc => Self::list_encrypted_container(path, password),
            _ => Err(ArchiveError::UnsupportedFormat(format.label().to_string())),
        }
    }

    pub fn verify_password(path: &Path, password: &str) -> Result<bool, ArchiveError> {
        let (input, _guard) = materialize_input(path)?;
        Self::verify_password_direct(&input, password)
    }

    fn verify_password_direct(path: &Path, password: &str) -> Result<bool, ArchiveError> {
        let format = Self::detect(path);
        match format {
            ArchiveFormat::Zip => {
                let file = File::open(path)?;
                let mut archive = ZipArchive::new(file)?;
                for index in 0..archive.len() {
                    let entry = archive.by_index_raw(index)?;
                    if entry.encrypted() {
                        drop(entry);
                        return match archive.by_index_decrypt(index, password.as_bytes()) {
                            Ok(mut encrypted) => {
                                let mut buffer = [0u8; 1];
                                match encrypted.read(&mut buffer) {
                                    Ok(count) => Ok(count > 0),
                                    Err(_) => Ok(false),
                                }
                            }
                            Err(zip::result::ZipError::InvalidPassword) => Ok(false),
                            Err(error) => Err(ArchiveError::Zip(error)),
                        };
                    }
                }
                Ok(true)
            }
            ArchiveFormat::SevenZip => {
                let mut reader = match ArchiveReader::open(path, Password::from(password)) {
                    Ok(reader) => reader,
                    Err(_) => return Ok(false),
                };
                let mut verified = true;
                let result = reader.for_each_entries(|entry, input| {
                    if entry.is_directory() || !entry.has_stream() || entry.size() == 0 {
                        return Ok(true);
                    }
                    // 7z AES 会在条目数据流读取完毕时校验 MAC/CRC。
                    // 因此只读取一个字节可能错误接受密码。
                    io::copy(input, &mut io::sink())?;
                    verified = true;
                    Ok(false)
                });
                match result {
                    Ok(()) => Ok(verified),
                    Err(_) => Ok(false),
                }
            }
            ArchiveFormat::Rar => match Self::verify_rar(path, password) {
                Ok(verified) => Ok(verified),
                Err(_) => Ok(false),
            },
            ArchiveFormat::Enc => Ok(decrypt_encrypted_container(path, password).is_ok()),
            _ => Ok(true),
        }
    }

    pub fn test<F>(
        path: &Path,
        password: Option<&str>,
        mut progress: F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let (input, _guard) = materialize_input(path)?;
        let format = Self::detect(&input);
        match format {
            ArchiveFormat::Zip => {
                let file = File::open(&input)?;
                let mut archive = ZipArchive::new(file)?;
                let mut report = ExtractReport::default();
                let total = archive.len();
                let bytes_total: u64 = (0..archive.len())
                    .filter_map(|index| archive.by_index_raw(index).ok().map(|entry| entry.size()))
                    .sum();
                for index in 0..archive.len() {
                    let name = archive
                        .by_index_raw(index)?
                        .name()
                        .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
                        .to_string();
                    let is_dir = archive.by_index_raw(index)?.is_dir();
                    if is_dir {
                        continue;
                    }
                    let mut entry = match password {
                        Some(password) => archive.by_index_decrypt(index, password.as_bytes())?,
                        None => archive.by_index(index)?,
                    };
                    let bytes = io::copy(&mut entry, &mut io::sink())?;
                    report.files += 1;
                    report.bytes += bytes;
                    progress(ExtractProgress {
                        current: name,
                        bytes_done: report.bytes,
                        bytes_total,
                        entries_done: report.files,
                        entries_total: total,
                    });
                }
                Ok(report)
            }
            ArchiveFormat::SevenZip => {
                let mut reader =
                    ArchiveReader::open(&input, Password::from(password.unwrap_or_default()))
                        .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
                let total = reader.archive().files.len();
                let bytes_total: u64 = reader
                    .archive()
                    .files
                    .iter()
                    .map(|entry| entry.size())
                    .sum();
                let mut report = ExtractReport::default();
                reader
                    .for_each_entries(|entry, input| {
                        if entry.is_directory() || !entry.has_stream() {
                            return Ok(true);
                        }
                        let bytes = io::copy(input, &mut io::sink()).map_err(|error| {
                            sevenz_rust2::Error::Other(error.to_string().into())
                        })?;
                        report.files += 1;
                        report.bytes += bytes;
                        progress(ExtractProgress {
                            current: entry.name().to_string(),
                            bytes_done: report.bytes,
                            bytes_total,
                            entries_done: report.files,
                            entries_total: total,
                        });
                        Ok(true)
                    })
                    .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
                Ok(report)
            }
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst => {
                let mut reader = tar_reader(&input, format)?;
                let mut report = ExtractReport::default();
                let total = 0;
                for entry in reader.entries()? {
                    let mut entry = entry?;
                    if entry.header().entry_type().is_dir() {
                        continue;
                    }
                    let name = entry.path()?.to_string_lossy().replace('\\', "/");
                    let bytes = io::copy(&mut entry, &mut io::sink())?;
                    report.files += 1;
                    report.bytes += bytes;
                    progress(ExtractProgress {
                        current: name.to_string(),
                        bytes_done: report.bytes,
                        bytes_total: report.bytes,
                        entries_done: report.files,
                        entries_total: total,
                    });
                }
                Ok(report)
            }
            ArchiveFormat::UnixCompress => {
                let (mut archive, entry) = open_unix_compress(&input)?;
                let bytes = archive
                    .read_to(&entry, &mut io::sink())
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
                Ok(ExtractReport {
                    files: 1,
                    bytes,
                    skipped: 0,
                })
            }
            ArchiveFormat::Enc => {
                let Some(password) = password.filter(|value| !value.is_empty()) else {
                    return Err(ArchiveError::PasswordRequired);
                };
                let data = decrypt_encrypted_container(&input, password)?;
                let bytes = data.len() as u64;
                Ok(ExtractReport {
                    files: 1,
                    bytes,
                    skipped: 0,
                })
            }
            _ => {
                let mut input = decompress_reader(&input, format)?;
                let bytes = io::copy(&mut input, &mut io::sink())?;
                Ok(ExtractReport {
                    files: 1,
                    bytes,
                    skipped: 0,
                })
            }
        }
    }

    pub fn extract<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let (input, _guard) = materialize_input(path)?;
        Self::extract_direct(&input, options, progress)
    }

    fn extract_direct<F>(
        path: &Path,
        options: &ExtractionOptions,
        mut progress: F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let format = Self::detect(path);
        match format {
            ArchiveFormat::Zip => Self::extract_zip(path, options, &mut progress),
            ArchiveFormat::SevenZip => Self::extract_seven_zip(path, options, &mut progress),
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst => Self::extract_tar(path, format, options, &mut progress),
            ArchiveFormat::Gzip
            | ArchiveFormat::Bzip2
            | ArchiveFormat::Xz
            | ArchiveFormat::Zstd
            | ArchiveFormat::Brotli
            | ArchiveFormat::Lz4 => {
                Self::extract_single_stream(path, format, options, &mut progress)
            }
            ArchiveFormat::Rar => Self::extract_rar(path, options, &mut progress),
            ArchiveFormat::UnixCompress => {
                Self::extract_unix_compress(path, options, &mut progress)
            }
            ArchiveFormat::Enc => Self::extract_encrypted_container(path, options, &mut progress),
            ArchiveFormat::Isz
            | ArchiveFormat::Img
            | ArchiveFormat::Bin
            | ArchiveFormat::Lzip
            | ArchiveFormat::Uu => {
                Self::extract_legacy_stream(path, format, options, &mut progress)
            }
            format if is_legacy_archive(format) => {
                Self::extract_legacy(path, format, options, &mut progress)
            }
            _ => Err(ArchiveError::UnsupportedFormat(format.label().to_string())),
        }
    }

    pub fn create<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        let split_size = options.split_size.filter(|size| *size >= 64 * 1024);
        if options.delete_source {
            ensure_sources_safe_to_delete(sources, output)?;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let staging = output.with_file_name(format!(
            ".lumark-split-build-{}-{}",
            std::process::id(),
            output
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("archive")
        ));
        let mut report = match Self::create_direct(&staging, sources, options, progress) {
            Ok(report) => report,
            Err(error) => {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
        };
        if options.test_after
            && let Err(error) = Self::test(
                &staging,
                options.password.as_deref().map(String::as_str),
                |_| {},
            )
        {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
        if let Some(split_size) = split_size {
            let volumes = match split_file(&staging, output, split_size) {
                Ok(volumes) => volumes,
                Err(error) => {
                    let _ = fs::remove_file(&staging);
                    return Err(error);
                }
            };
            let _ = fs::remove_file(&staging);
            report.output = volumes
                .first()
                .cloned()
                .unwrap_or_else(|| output.to_path_buf());
            if options.delete_source {
                remove_source_paths(sources, &mut report.cleanup_errors);
            }
            Ok(report)
        } else {
            if let Err(error) = atomic_replace_file(&staging, output) {
                let _ = fs::remove_file(&staging);
                return Err(ArchiveError::Io(error));
            }
            report.output = output.to_path_buf();
            if options.delete_source {
                remove_source_paths(sources, &mut report.cleanup_errors);
            }
            Ok(report)
        }
    }

    /// 生成自解压 ZIP：拼接运行时模块、ZIP 数据和尾部页脚。
    ///
    /// 先在目标目录生成临时 ZIP，成功后再写成单个 EXE；失败只清理临时文件，
    /// 不留下半成品。运行时模块只实现无密码 ZIP 解压，因此这里提前拒绝密码
    /// 和分卷，避免生成打不开的包。
    pub fn create_sfx<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        stub: &Path,
        progress: F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        if options.format != ArchiveFormat::Zip {
            return Err(ArchiveError::UnsupportedFormat(format!(
                "自解压仅支持 ZIP，当前为 {}",
                options.format.label()
            )));
        }
        if options.password.is_some() {
            return Err(ArchiveError::Corrupt(
                "自解压暂不支持密码，请先关闭密码".into(),
            ));
        }
        if options.split_size.is_some_and(|size| size >= 64 * 1024) {
            return Err(ArchiveError::Corrupt("自解压不支持分卷".into()));
        }
        if !stub.is_file() {
            return Err(ArchiveError::Corrupt(format!(
                "找不到自解压运行时模块：{}",
                stub.display()
            )));
        }
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(
            ".lumark-sfx-{}-{}.zip",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        ));
        let mut zip_options = options.clone();
        zip_options.sfx = false;
        let report = match Self::create(&temp, sources, &zip_options, progress) {
            Ok(report) => report,
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
        };
        let composed = (|| -> Result<(), ArchiveError> {
            let mut target = File::create(output)?;
            let mut stub_file = File::open(stub)?;
            let offset = io::copy(&mut stub_file, &mut target)?;
            let mut payload = File::open(&temp)?;
            let length = io::copy(&mut payload, &mut target)?;
            target.write_all(&sfx_footer(offset, length))?;
            target.sync_all()?;
            Ok(())
        })();
        let _ = fs::remove_file(&temp);
        composed?;
        Ok(CreateReport {
            output: output.to_path_buf(),
            ..report
        })
    }

    fn create_direct<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        mut progress: F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        match options.format {
            ArchiveFormat::Zip => Self::create_zip(output, sources, options, &mut progress),
            ArchiveFormat::SevenZip => {
                Self::create_seven_zip(output, sources, options, &mut progress)
            }
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst => Self::create_tar(output, sources, options, &mut progress),
            ArchiveFormat::Gzip
            | ArchiveFormat::Bzip2
            | ArchiveFormat::Xz
            | ArchiveFormat::Zstd
            | ArchiveFormat::Brotli
            | ArchiveFormat::Lz4 => {
                Self::create_single_stream(output, sources, options, &mut progress)
            }
            other => Err(ArchiveError::UnsupportedFormat(other.label().to_string())),
        }
    }

    fn list_zip(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut info = ArchiveInfo {
            format: Some(ArchiveFormat::Zip),
            comment: Some(String::from_utf8_lossy(archive.comment()).into_owned()),
            ..Default::default()
        };
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index)?;
            let name = entry
                .name()
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
                .to_string();
            let size = entry.size();
            let compressed = entry.compressed_size();
            let modified = zip_time_to_chrono(entry.last_modified());
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path: name,
                is_dir: entry.is_dir(),
                size,
                compressed_size: Some(compressed),
                modified,
                crc32: Some(entry.crc32()),
                method: format!("{:?}", entry.compression()),
                encrypted: entry.encrypted(),
            });
            info.total_size += size;
            info.compressed_size += compressed;
            info.encrypted |= entry.encrypted();
        }
        Ok(info)
    }

    fn probe_rar(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
        let listing = match UnrarArchive::new(path).open_for_listing() {
            Ok(listing) => listing,
            Err(error) if rar_error_means_encrypted(&error) => {
                return Ok(EncryptionInfo {
                    encrypted: true,
                    header_encrypted: true,
                    format: Some(ArchiveFormat::Rar),
                });
            }
            Err(error) => return Err(map_rar_error(error)),
        };
        let mut encrypted = false;
        for header in listing {
            let header = match header {
                Ok(header) => header,
                Err(error) if rar_error_means_encrypted(&error) => {
                    return Ok(EncryptionInfo {
                        encrypted: true,
                        header_encrypted: true,
                        format: Some(ArchiveFormat::Rar),
                    });
                }
                Err(error) => return Err(map_rar_error(error)),
            };
            encrypted |= header.is_encrypted();
        }
        Ok(EncryptionInfo {
            encrypted,
            header_encrypted: false,
            format: Some(ArchiveFormat::Rar),
        })
    }

    fn list_rar(path: &Path, password: Option<&str>) -> Result<ArchiveInfo, ArchiveError> {
        let archive = match password.filter(|value| !value.is_empty()) {
            Some(password) => UnrarArchive::with_password(path, password),
            None => UnrarArchive::new(path),
        };
        let listing = archive.open_for_listing().map_err(map_rar_error)?;
        let mut info = ArchiveInfo {
            format: Some(ArchiveFormat::Rar),
            ..Default::default()
        };
        for (index, header) in listing.enumerate() {
            let header = header.map_err(map_rar_error)?;
            let path = header.filename.to_string_lossy().replace('\\', "/");
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path,
                is_dir: header.is_directory(),
                size: header.unpacked_size,
                compressed_size: None,
                modified: None,
                crc32: Some(header.file_crc),
                method: "RAR".to_string(),
                encrypted: header.is_encrypted(),
            });
            info.total_size += header.unpacked_size;
            info.encrypted |= header.is_encrypted();
        }
        Ok(info)
    }

    fn list_legacy(path: &Path, format: ArchiveFormat) -> Result<ArchiveInfo, ArchiveError> {
        let mut reader = open_legacy_reader(path, format)?;
        let entries = reader
            .entries()
            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
        let mut info = ArchiveInfo {
            format: Some(format),
            ..Default::default()
        };
        for (index, entry) in entries.into_iter().enumerate() {
            let modified = entry
                .modified
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds as i64, 0));
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path: entry.path,
                is_dir: entry.is_dir,
                size: entry.size,
                compressed_size: (entry.compressed_size > 0).then_some(entry.compressed_size),
                modified,
                crc32: entry.crc32,
                method: format.label().to_string(),
                encrypted: false,
            });
            info.total_size += entry.size;
            info.compressed_size += entry.compressed_size;
        }
        Ok(info)
    }

    fn extract_legacy<F>(
        path: &Path,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut reader = open_legacy_reader(path, format)?;
        let entries = reader
            .entries()
            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
        let total = entries.len();
        let total_bytes: u64 = entries.iter().map(|entry| entry.size).sum();
        fs::create_dir_all(&options.output_dir)?;
        let mut report = ExtractReport::default();
        for entry in entries {
            ensure_not_cancelled(&options.cancel)?;
            if !selected(&entry.path, &options.selected_paths) {
                continue;
            }
            let mut destination = safe_join(&options.output_dir, &entry.path)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            if entry.is_dir {
                fs::create_dir_all(destination)?;
                continue;
            }
            if destination.exists() {
                match options.overwrite_policy {
                    crate::format::OverwritePolicy::Rename => {
                        destination = unique_destination(&destination);
                    }
                    crate::format::OverwritePolicy::Skip => {
                        report.skipped += 1;
                        continue;
                    }
                    crate::format::OverwritePolicy::Ask if !options.overwrite => {
                        report.skipped += 1;
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&destination)?;
            if let Err(error) = reader.extract(&entry, &mut output) {
                drop(output);
                let _ = fs::remove_file(&destination);
                return Err(ArchiveError::Corrupt(error.to_string()));
            }
            report.files += 1;
            report.bytes += entry.size;
            progress(ExtractProgress {
                current: entry.path,
                bytes_done: report.bytes,
                bytes_total: total_bytes,
                entries_done: report.files,
                entries_total: total,
            });
        }
        Ok(report)
    }

    fn verify_rar(path: &Path, password: &str) -> Result<bool, ArchiveError> {
        let archive = if password.is_empty() {
            UnrarArchive::new(path)
        } else {
            UnrarArchive::with_password(path, password)
        };
        let mut archive = archive.open_for_processing().map_err(map_rar_error)?;
        loop {
            let Some(header) = archive.read_header().map_err(map_rar_error)? else {
                return Ok(true);
            };
            if header.entry().is_directory() || header.entry().unpacked_size == 0 {
                archive = header.skip().map_err(map_rar_error)?;
            } else {
                header.test().map_err(map_rar_error)?;
                return Ok(true);
            }
        }
    }

    fn extract_rar<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let archive = match options.password.as_deref() {
            Some(password) if !password.is_empty() => UnrarArchive::with_password(path, password),
            _ => UnrarArchive::new(path),
        };
        let mut archive = archive.open_for_processing().map_err(map_rar_error)?;
        fs::create_dir_all(&options.output_dir)?;
        let mut report = ExtractReport::default();
        loop {
            ensure_not_cancelled(&options.cancel)?;
            let Some(header) = archive.read_header().map_err(map_rar_error)? else {
                break;
            };
            let name = header.entry().filename.to_string_lossy().replace('\\', "/");
            if !selected(&name, &options.selected_paths) {
                archive = header.skip().map_err(map_rar_error)?;
                continue;
            }
            if header.entry().is_directory() {
                let destination = safe_join(&options.output_dir, &name)?;
                ensure_no_symlink_components(&options.output_dir, &destination)?;
                fs::create_dir_all(destination)?;
                archive = header
                    .skip()
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
                continue;
            }
            let size = header.entry().unpacked_size;
            let mut destination = safe_join(&options.output_dir, &name)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            if destination.exists() {
                match options.overwrite_policy {
                    crate::format::OverwritePolicy::Rename => {
                        destination = unique_destination(&destination);
                    }
                    crate::format::OverwritePolicy::Skip => {
                        report.skipped += 1;
                        archive = header
                            .skip()
                            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
                        continue;
                    }
                    crate::format::OverwritePolicy::Ask if !options.overwrite => {
                        report.skipped += 1;
                        archive = header
                            .skip()
                            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            archive = match header.extract_to(&destination) {
                Ok(archive) => archive,
                Err(error) => {
                    let _ = fs::remove_file(&destination);
                    return Err(ArchiveError::Corrupt(error.to_string()));
                }
            };
            report.files += 1;
            report.bytes += size;
            progress(ExtractProgress {
                current: name,
                bytes_done: report.bytes,
                bytes_total: report.bytes,
                entries_done: report.files,
                entries_total: 0,
            });
        }
        Ok(report)
    }

    fn list_seven_zip(path: &Path, password: &str) -> Result<ArchiveInfo, ArchiveError> {
        let reader = ArchiveReader::open(path, Password::from(password))
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        let archive = reader.archive();
        let header_encrypted = ArchiveReader::open(path, Password::empty()).is_err();
        let entries_encrypted = archive
            .blocks
            .iter()
            .flat_map(|block| block.coders.iter())
            .any(|coder| {
                coder.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256
            });
        let mut info = ArchiveInfo {
            format: Some(ArchiveFormat::SevenZip),
            header_encrypted,
            encrypted: header_encrypted || entries_encrypted,
            solid: archive.is_solid,
            ..Default::default()
        };
        for (index, file) in archive.files.iter().enumerate() {
            let modified: DateTime<Utc> =
                Into::<std::time::SystemTime>::into(file.last_modified_date()).into();
            let size = file.size();
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path: file.name().to_string(),
                is_dir: file.is_directory(),
                size,
                compressed_size: None,
                modified: Some(modified),
                crc32: Some(file.crc as u32),
                method: "7z".to_string(),
                encrypted: entries_encrypted,
            });
            info.total_size += size;
        }
        Ok(info)
    }

    fn list_tar(path: &Path, format: ArchiveFormat) -> Result<ArchiveInfo, ArchiveError> {
        let mut info = ArchiveInfo {
            format: Some(format),
            ..Default::default()
        };
        let mut reader = tar_reader(path, format)?;
        for (index, entry) in reader.entries()?.enumerate() {
            let entry = entry?;
            let path = entry.path()?.to_string_lossy().replace('\\', "/");
            let size = entry.header().size()?;
            let is_dir = entry.header().entry_type().is_dir();
            let modified = entry
                .header()
                .mtime()
                .ok()
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds as i64, 0));
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path,
                is_dir,
                size,
                compressed_size: None,
                modified,
                crc32: None,
                method: "TAR".to_string(),
                encrypted: false,
            });
            info.total_size += size;
        }
        Ok(info)
    }

    fn list_single_stream(path: &Path, format: ArchiveFormat) -> Result<ArchiveInfo, ArchiveError> {
        let size = fs::metadata(path)?.len();
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("data")
            .to_string();
        Ok(ArchiveInfo {
            format: Some(format),
            entries: vec![ArchiveEntry {
                id: 0,
                path: name,
                is_dir: false,
                size: 0,
                compressed_size: Some(size),
                modified: None,
                crc32: None,
                method: format.label().to_string(),
                encrypted: false,
            }],
            total_size: 0,
            compressed_size: size,
            ..Default::default()
        })
    }

    fn list_unix_compress(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let (_, entry) = open_unix_compress(path)?;
        let name = unix_compress_name(path, entry.name());
        let size = entry.original_size();
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::UnixCompress),
            entries: vec![ArchiveEntry {
                id: 0,
                path: name,
                is_dir: false,
                size,
                compressed_size: Some(entry.compressed_size()),
                modified: None,
                crc32: Some(entry.crc() as u32),
                method: entry.compression_method().to_string(),
                encrypted: false,
            }],
            total_size: size,
            compressed_size: entry.compressed_size(),
            ..Default::default()
        })
    }

    fn list_encrypted_container(
        path: &Path,
        password: Option<&str>,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let name = encrypted_container_name(path);
        let compressed_size = fs::metadata(path)?.len();
        let size = match password.filter(|value| !value.is_empty()) {
            Some(password) => decrypt_encrypted_container(path, password)?.len() as u64,
            None => 0,
        };
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Enc),
            entries: vec![ArchiveEntry {
                id: 0,
                path: name,
                is_dir: false,
                size,
                compressed_size: Some(compressed_size),
                modified: None,
                crc32: None,
                method: "AES-256-GCM-SIV".to_string(),
                encrypted: true,
            }],
            encrypted: true,
            total_size: size,
            compressed_size,
            ..Default::default()
        })
    }

    fn extract_encrypted_container<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let Some(password) = options
            .password
            .as_deref()
            .filter(|value| !value.is_empty())
        else {
            return Err(ArchiveError::PasswordRequired);
        };
        let name = encrypted_container_name(path);
        if !selected(&name, &options.selected_paths) {
            return Ok(ExtractReport {
                skipped: 1,
                ..Default::default()
            });
        }
        let data = decrypt_encrypted_container(path, password)?;
        let mut destination = safe_join(&options.output_dir, &name)?;
        ensure_no_symlink_components(&options.output_dir, &destination)?;
        if destination.exists() {
            match options.overwrite_policy {
                crate::format::OverwritePolicy::Rename => {
                    destination = unique_destination(&destination);
                }
                crate::format::OverwritePolicy::Skip => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                crate::format::OverwritePolicy::Ask if !options.overwrite => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        fs::create_dir_all(&options.output_dir)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        ensure_not_cancelled(&options.cancel)?;
        let mut output = CancelWriter {
            inner: File::create(&destination)?,
            cancel: &options.cancel,
        };
        let mut input = data.as_slice();
        let bytes = match copy_cancellable_limited(
            &mut input,
            &mut output,
            &options.cancel,
            options.budget.max_bytes,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                drop(output);
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
        };
        progress(ExtractProgress {
            current: name,
            bytes_done: bytes,
            bytes_total: bytes,
            entries_done: 1,
            entries_total: 1,
        });
        Ok(ExtractReport {
            files: 1,
            bytes,
            skipped: 0,
        })
    }

    fn extract_unix_compress<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let (mut archive, entry) = open_unix_compress(path)?;
        let name = unix_compress_name(path, entry.name());
        if !selected(&name, &options.selected_paths) {
            return Ok(ExtractReport {
                skipped: 1,
                ..Default::default()
            });
        }
        let mut destination = safe_join(&options.output_dir, &name)?;
        ensure_no_symlink_components(&options.output_dir, &destination)?;
        if destination.exists() {
            match options.overwrite_policy {
                crate::format::OverwritePolicy::Rename => {
                    destination = unique_destination(&destination);
                }
                crate::format::OverwritePolicy::Skip => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                crate::format::OverwritePolicy::Ask if !options.overwrite => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        fs::create_dir_all(&options.output_dir)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        ensure_not_cancelled(&options.cancel)?;
        let output = File::create(&destination)?;
        let mut writer = CancelWriter {
            inner: output,
            cancel: &options.cancel,
        };
        let bytes = match archive.read_to(&entry, &mut writer) {
            Ok(bytes) => bytes,
            Err(error) => {
                drop(writer);
                let _ = fs::remove_file(&destination);
                return Err(ArchiveError::Corrupt(error.to_string()));
            }
        };
        ensure_not_cancelled(&options.cancel)?;
        progress(ExtractProgress {
            current: name,
            bytes_done: bytes,
            bytes_total: bytes,
            entries_done: 1,
            entries_total: 1,
        });
        Ok(ExtractReport {
            files: 1,
            bytes,
            skipped: 0,
        })
    }

    fn extract_zip<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;
        let entries_total = archive.len();
        let bytes_total: u64 = (0..archive.len())
            .filter_map(|index| archive.by_index_raw(index).ok().map(|entry| entry.size()))
            .sum();
        let mut report = ExtractReport::default();
        fs::create_dir_all(&options.output_dir)?;
        for index in 0..archive.len() {
            let (name, size, is_dir) = {
                let entry = archive.by_index_raw(index)?;
                (
                    entry
                        .name()
                        .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
                        .to_string(),
                    entry.size(),
                    entry.is_dir(),
                )
            };
            if !selected(&name, &options.selected_paths) {
                continue;
            }
            let mut destination = safe_join(&options.output_dir, &name)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            if is_dir {
                fs::create_dir_all(&destination)?;
                continue;
            }
            if destination.exists() {
                match options.overwrite_policy {
                    crate::format::OverwritePolicy::Rename => {
                        destination = unique_destination(&destination);
                    }
                    crate::format::OverwritePolicy::Skip => {
                        report.skipped += 1;
                        continue;
                    }
                    crate::format::OverwritePolicy::Ask if !options.overwrite => {
                        report.skipped += 1;
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            ensure_budget(options, &report, size)?;
            let mut entry = match options.password.as_deref() {
                Some(password) => archive.by_index_decrypt(index, password.as_bytes())?,
                None => archive.by_index(index)?,
            };
            let mut output = File::create(&destination)?;
            let copied = match copy_cancellable_limited(
                &mut entry,
                &mut output,
                &options.cancel,
                options.budget.max_bytes.saturating_sub(report.bytes),
            ) {
                Ok(copied) => copied,
                Err(error) => {
                    drop(output);
                    let _ = fs::remove_file(&destination);
                    return Err(error);
                }
            };
            report.files += 1;
            report.bytes += copied;
            progress(ExtractProgress {
                current: name,
                bytes_done: report.bytes,
                bytes_total,
                entries_done: report.files,
                entries_total,
            });
        }
        Ok(report)
    }

    fn extract_seven_zip<F>(
        path: &Path,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let password = options
            .password
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or_default();
        let mut reader = ArchiveReader::open(path, Password::from(password))
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        let entries_total = reader.archive().files.len();
        let bytes_total: u64 = reader.archive().files.iter().map(|file| file.size()).sum();
        let mut report = ExtractReport::default();
        fs::create_dir_all(&options.output_dir)?;
        // 预算错误在 sevenz 回调里只能变成字符串错误，先记下原文，循环结束后
        // 还原成 BudgetExceeded，让界面能显示统一的“超出资源预算”提示。
        let budget_error = Rc::new(RefCell::new(None::<String>));
        reader
            .for_each_entries(|entry, input| {
                if is_cancelled(&options.cancel) {
                    return Err(sevenz_rust2::Error::Other("cancelled".into()));
                }
                let name = entry.name().to_string();
                if entry.is_directory() {
                    let path = safe_join(&options.output_dir, &name)
                        .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
                    ensure_no_symlink_components(&options.output_dir, &path)
                        .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
                    fs::create_dir_all(path)?;
                    return Ok(true);
                }
                if !selected(&name, &options.selected_paths) {
                    drain_seven_zip_entry(input, &options.cancel)
                        .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
                    return Ok(true);
                }
                let mut destination = safe_join(&options.output_dir, &name)
                    .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
                ensure_no_symlink_components(&options.output_dir, &destination)
                    .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
                if destination.exists() {
                    match options.overwrite_policy {
                        crate::format::OverwritePolicy::Rename => {
                            destination = unique_destination(&destination);
                        }
                        crate::format::OverwritePolicy::Skip => {
                            drain_seven_zip_entry(input, &options.cancel).map_err(|error| {
                                sevenz_rust2::Error::Other(error.to_string().into())
                            })?;
                            report.skipped += 1;
                            return Ok(true);
                        }
                        crate::format::OverwritePolicy::Ask if !options.overwrite => {
                            drain_seven_zip_entry(input, &options.cancel).map_err(|error| {
                                sevenz_rust2::Error::Other(error.to_string().into())
                            })?;
                            report.skipped += 1;
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Err(error) = ensure_budget(options, &report, entry.size()) {
                    *budget_error.borrow_mut() = Some(error.to_string());
                    return Err(sevenz_rust2::Error::Other(error.to_string().into()));
                }
                let mut output = CancelWriter {
                    inner: File::create(&destination)?,
                    cancel: &options.cancel,
                };
                // 单文件内部按块上报，避免大条目在解压时进度条停住。
                let base = report.bytes;
                let current = name.clone();
                let remaining = options.budget.max_bytes.saturating_sub(report.bytes);
                let entry_bytes = match copy_entry_with_progress(
                    input,
                    &mut output,
                    &options.cancel,
                    remaining,
                    |done| {
                        progress(ExtractProgress {
                            current: current.clone(),
                            bytes_done: base + done,
                            bytes_total,
                            entries_done: report.files,
                            entries_total,
                        });
                    },
                ) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        drop(output);
                        let _ = fs::remove_file(&destination);
                        if let ArchiveError::BudgetExceeded(message) = &error {
                            *budget_error.borrow_mut() = Some(message.clone());
                        }
                        return Err(sevenz_rust2::Error::Other(error.to_string().into()));
                    }
                };
                report.files += 1;
                report.bytes += entry_bytes;
                progress(ExtractProgress {
                    current: name,
                    bytes_done: report.bytes,
                    bytes_total,
                    entries_done: report.files,
                    entries_total,
                });
                Ok(true)
            })
            .map_err(|error| match budget_error.borrow().clone() {
                Some(message) => ArchiveError::BudgetExceeded(message),
                None => ArchiveError::SevenZ(error.to_string()),
            })?;
        Ok(report)
    }

    fn extract_tar<F>(
        path: &Path,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut reader = tar_reader(path, format)?;
        let entries_total = 0;
        let bytes_total = 0;
        let mut report = ExtractReport::default();
        fs::create_dir_all(&options.output_dir)?;
        for entry in reader.entries()? {
            ensure_not_cancelled(&options.cancel)?;
            let mut entry = entry?;
            let path_string = entry.path()?.to_string_lossy().replace('\\', "/");
            if !selected(&path_string, &options.selected_paths) {
                continue;
            }
            let mut destination = safe_join(&options.output_dir, &path_string)?;
            ensure_no_symlink_components(&options.output_dir, &destination)?;
            if entry.header().entry_type().is_dir() {
                fs::create_dir_all(&destination)?;
                continue;
            }
            if destination.exists() {
                match options.overwrite_policy {
                    crate::format::OverwritePolicy::Rename => {
                        destination = unique_destination(&destination);
                    }
                    crate::format::OverwritePolicy::Skip => {
                        report.skipped += 1;
                        continue;
                    }
                    crate::format::OverwritePolicy::Ask if !options.overwrite => {
                        report.skipped += 1;
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            ensure_budget(options, &report, entry.size())?;
            let mut output = File::create(&destination)?;
            let copied = match copy_cancellable_limited(
                &mut entry,
                &mut output,
                &options.cancel,
                options.budget.max_bytes.saturating_sub(report.bytes),
            ) {
                Ok(copied) => copied,
                Err(error) => {
                    drop(output);
                    let _ = fs::remove_file(&destination);
                    return Err(error);
                }
            };
            report.files += 1;
            report.bytes += copied;
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

    fn extract_single_stream<F>(
        path: &Path,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("data")
            .to_string();
        let mut destination = options.output_dir.join(&name);
        if destination.exists() {
            match options.overwrite_policy {
                crate::format::OverwritePolicy::Rename => {
                    destination = unique_destination(&destination);
                }
                crate::format::OverwritePolicy::Skip => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                crate::format::OverwritePolicy::Ask if !options.overwrite => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        fs::create_dir_all(&options.output_dir)?;
        let mut input = decompress_reader(path, format)?;
        let mut output = File::create(&destination)?;
        let bytes = match copy_cancellable_limited(
            &mut input,
            &mut output,
            &options.cancel,
            options.budget.max_bytes,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                drop(output);
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
        };
        progress(ExtractProgress {
            current: name,
            bytes_done: bytes,
            bytes_total: bytes,
            entries_done: 1,
            entries_total: 1,
        });
        Ok(ExtractReport {
            files: 1,
            bytes,
            skipped: 0,
        })
    }

    fn extract_legacy_stream<F>(
        path: &Path,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("data");
        let name = match format {
            ArchiveFormat::Isz => format!("{stem}.iso"),
            ArchiveFormat::Img => format!("{stem}.img"),
            ArchiveFormat::Lzip => stem.to_string(),
            _ => format!("{stem}.bin"),
        };
        fs::create_dir_all(&options.output_dir)?;
        let mut destination = options.output_dir.join(&name);
        ensure_no_symlink_components(&options.output_dir, &destination)?;
        if destination.exists() {
            match options.overwrite_policy {
                crate::format::OverwritePolicy::Rename => {
                    destination = unique_destination(&destination);
                }
                crate::format::OverwritePolicy::Skip => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                crate::format::OverwritePolicy::Ask if !options.overwrite => {
                    return Ok(ExtractReport {
                        skipped: 1,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        let mut output = File::create(&destination)?;
        let result = match format {
            ArchiveFormat::Isz => {
                let mut input = BufReader::new(File::open(path)?);
                geezipx_core::archive::isz::isz_decompress(&mut input, &mut output)
            }
            ArchiveFormat::Img => {
                let mut input = BufReader::new(File::open(path)?);
                geezipx_core::archive::img::img_decompress(&mut input, &mut output)
            }
            ArchiveFormat::Bin => {
                let mut input = BufReader::new(File::open(path)?);
                geezipx_core::archive::bin::bin_decompress(&mut input, &mut output)
            }
            ArchiveFormat::Lzip => {
                let mut input = BufReader::new(File::open(path)?);
                if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("lzma"))
                {
                    geezipx_core::archive::xz::lzma_decompress(&mut input, &mut output)
                } else {
                    geezipx_core::archive::lz::lz_decompress(&mut input, &mut output)
                }
            }
            ArchiveFormat::Uu => {
                if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("xxe"))
                {
                    geezipx_core::archive::xxe::xxe_decode_to_writer(path, &mut output)
                } else {
                    geezipx_core::archive::uu::uu_decode_to_writer(path, &mut output)
                }
            }
            _ => unreachable!(),
        };
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                drop(output);
                let _ = fs::remove_file(&destination);
                return Err(ArchiveError::Corrupt(error.to_string()));
            }
        };
        progress(ExtractProgress {
            current: name,
            bytes_done: bytes,
            bytes_total: bytes,
            entries_done: 1,
            entries_total: 1,
        });
        Ok(ExtractReport {
            files: 1,
            bytes,
            skipped: 0,
        })
    }

    fn create_zip<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(output)?;
        let mut writer = ZipWriter::new(file);
        if let Some(comment) = &options.comment {
            writer.set_comment(comment.as_str())?;
        }
        let plan = build_source_plan(sources, options)?;
        let bytes_total = plan.total_bytes;
        let entries_total = plan.files.len();
        let fixed_time = match options.archive_time {
            crate::format::ArchiveTimeSource::Current => Some(SystemTime::now()),
            crate::format::ArchiveTimeSource::Original => None,
            crate::format::ArchiveTimeSource::Latest => {
                plan.latest_modified.or_else(|| Some(SystemTime::now()))
            }
        };
        let mut report = CreateReport {
            output: output.to_path_buf(),
            ..Default::default()
        };
        for (relative, source, _) in plan.files {
            ensure_not_cancelled(&options.cancel)?;
            let method = if options
                .store_only_patterns
                .iter()
                .any(|pattern| glob_match(pattern, &relative))
            {
                CompressionMethod::Stored
            } else {
                zip_method(options)
            };
            let mut file_options = SimpleFileOptions::default()
                .compression_method(method)
                .compression_level(options.level)
                .large_file(fs::metadata(&source)?.len() > u32::MAX as u64);
            if let Some(password) = options.password.as_deref() {
                file_options = match options.zip_encryption {
                    ZipEncryption::Aes256 => {
                        file_options.with_aes_encryption(AesMode::Aes256, password.as_str())
                    }
                    ZipEncryption::ZipCrypto => {
                        file_options.with_deprecated_encryption(password.as_bytes())?
                    }
                };
            }
            let entry_time = fixed_time.or_else(|| {
                fs::metadata(&source)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
            });
            if let Some(time) = entry_time.and_then(zip_datetime) {
                file_options = file_options.last_modified_time(time);
            }
            writer.start_file(relative.clone(), file_options)?;
            let mut input = File::open(&source)?;
            let file_total = input.metadata()?.len();
            let mut file_done = 0u64;
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut last_report = Instant::now() - Duration::from_secs(1);
            loop {
                ensure_not_cancelled(&options.cancel)?;
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                writer.write_all(&buffer[..read])?;
                file_done += read as u64;
                report.bytes += read as u64;
                if last_report.elapsed() >= Duration::from_millis(100) {
                    last_report = Instant::now();
                    progress(CreateProgress {
                        current: relative.clone(),
                        bytes_done: report.bytes,
                        bytes_total,
                        current_bytes_done: file_done,
                        current_bytes_total: file_total,
                        output_bytes: fs::metadata(output).map(|meta| meta.len()).unwrap_or(0),
                        entries_done: report.files,
                        entries_total,
                    });
                }
            }
            report.files += 1;
            progress(CreateProgress {
                current: relative,
                bytes_done: report.bytes,
                bytes_total,
                current_bytes_done: file_total,
                current_bytes_total: file_total,
                output_bytes: fs::metadata(output).map(|meta| meta.len()).unwrap_or(0),
                entries_done: report.files,
                entries_total,
            });
        }
        writer.finish()?;
        Ok(report)
    }

    fn create_seven_zip<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let level = options.level.unwrap_or(6).clamp(0, 9) as u32;
        // 多线程 LZMA2 会提前读走一整段在途缓冲，按读取字节上报会在几秒内
        // 冲到 100% 然后长时间不动。未加密的 LZMA2 多线程流每完成一个编码
        // 块才向输出写一次，可以据此换算真实进度；加密后输出按 AES 分块写出，
        // 无法区分编码块边界，仍退回读取进度。
        let track_chunks = matches!(
            options.sevenz_compression,
            crate::format::SevenZCompression::Lzma2
        ) && options.threads > 1
            && options.password.is_none();
        // lzma-rust2 的 4–9 级预设除字典和 nice_len 外参数相同；这两项在显式
        // 设置时会被用户值覆盖，未设置时 7/9 级的取值也一致。显式设置字典时
        // 把建块级别压到 7，避免 8/9 级预设字典把多线程块抬到 32/64 MiB。
        // 块越小，进度更新越细、在途缓冲越少。
        let mt_level = if options.dictionary_size.is_some() {
            level.min(7)
        } else {
            level
        };
        let chunk_bytes = if track_chunks {
            LZMA2_CHUNK_BYTES
                .max(lzma2_preset_dictionary(mt_level))
                .max(options.dictionary_size.map(u64::from).unwrap_or(0))
        } else {
            0
        };
        let plan = build_source_plan(sources, options)?;
        let bytes_total = plan.total_bytes;
        let entries_total = plan.files.len();
        let progress_cell = Rc::new(RefCell::new(&mut *progress));
        let chunk_state = Rc::new(RefCell::new(ChunkProgressState {
            active: false,
            base: 0,
            total: 0,
            chunk_bytes,
            chunks: 0,
            entries_before: 0,
            entries_total,
            bytes_total,
            output_bytes: 0,
            spans: Vec::new(),
        }));
        let mut writer = ArchiveWriter::new(ChunkTrackingWriter {
            inner: File::create(output)?,
            state: chunk_state.clone(),
            progress: progress_cell.clone(),
        })
        .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        let mut methods = match options.sevenz_compression {
            crate::format::SevenZCompression::Lzma2 => {
                let mut lzma2 = if options.threads > 1 {
                    Lzma2Options::from_level_mt(mt_level, options.threads, LZMA2_CHUNK_BYTES)
                } else {
                    Lzma2Options::from_level(level)
                };
                if let Some(dictionary_size) = options.dictionary_size {
                    lzma2.set_dictionary_size(dictionary_size);
                }
                if let Some(word_size) = options.word_size {
                    lzma2.set_nice_len(word_size);
                }
                vec![EncoderConfiguration::from(lzma2)]
            }
            crate::format::SevenZCompression::Lzma => {
                let mut lzma = LzmaOptions::from_level(level);
                if let Some(dictionary_size) = options.dictionary_size {
                    lzma.set_dictionary_size(dictionary_size);
                }
                if let Some(word_size) = options.word_size {
                    lzma.set_nice_len(word_size);
                }
                vec![EncoderConfiguration::from(lzma)]
            }
            crate::format::SevenZCompression::Ppmd => {
                let ppmd = PpmdOptions::from_order_memory_size(
                    options.word_size.unwrap_or(32),
                    options.dictionary_size.unwrap_or(16 * 1024 * 1024),
                );
                vec![EncoderConfiguration::from(ppmd)]
            }
            crate::format::SevenZCompression::Bzip2 => {
                vec![EncoderConfiguration::from(Bzip2Options::from_level(level))]
            }
        };
        if let Some(password) = options.password.as_deref() {
            methods.push(
                sevenz_rust2::encoder_options::AesEncoderOptions::new(Password::from(
                    password.as_str(),
                ))
                .into(),
            );
        }
        writer.set_content_methods(methods);
        writer.set_encrypt_header(options.password.is_some() && options.encrypt_header);
        let fixed_time = match options.archive_time {
            crate::format::ArchiveTimeSource::Current => Some(SystemTime::now()),
            crate::format::ArchiveTimeSource::Original => None,
            crate::format::ArchiveTimeSource::Latest => {
                plan.latest_modified.or_else(|| Some(SystemTime::now()))
            }
        };
        let mut report = CreateReport {
            output: output.to_path_buf(),
            ..Default::default()
        };
        if options.solid && plan.files.len() > 1 {
            // 固实块会先打开块内全部源文件、再统一压缩；这里按界面上的“固实块
            // 大小”和文件数上限切块：块内仍保留跨文件压缩，块外不会一次占用
            // 十几万个句柄和成倍的内存。
            let block_bytes = options
                .solid_block_size
                .filter(|size| *size >= 1024 * 1024)
                .unwrap_or(DEFAULT_SOLID_BLOCK_BYTES);
            let entries_done = Rc::new(Cell::new(0usize));
            let mut handled_bytes = 0u64;
            let mut handled_files = 0usize;
            let mut last_file = String::new();
            let mut last_size = 0u64;
            for block in solid_blocks(&plan.files, block_bytes, SOLID_BLOCK_MAX_FILES) {
                ensure_not_cancelled(&options.cancel)?;
                let mut entries = Vec::with_capacity(block.len());
                let mut readers: Vec<SourceReader<Box<dyn Read>>> = Vec::with_capacity(block.len());
                let mut spans = Vec::with_capacity(block.len());
                let mut block_offset = 0u64;
                for (relative, source, _) in block {
                    let mut entry = sevenz_rust2::ArchiveEntry::from_path(source, relative.clone());
                    if let Some(time) = fixed_time {
                        apply_entry_time(&mut entry, time);
                    }
                    let file_bytes = fs::metadata(source)?.len();
                    entries.push(entry);
                    if track_chunks {
                        readers.push(SourceReader::new(Box::new(CancelReader {
                            inner: File::open(source)?,
                            cancel: options.cancel.clone(),
                        })
                            as Box<dyn Read>));
                    } else {
                        readers.push(SourceReader::new(Box::new(SharedProgressReader {
                            inner: File::open(source)?,
                            current: relative.clone(),
                            total: file_bytes,
                            done: 0,
                            overall_base: handled_bytes + block_offset,
                            bytes_total,
                            entries_done: entries_done.clone(),
                            entries_total,
                            output: output.to_path_buf(),
                            cancel: options.cancel.clone(),
                            progress: progress_cell.clone(),
                            last_report: Instant::now() - Duration::from_secs(1),
                            finished: false,
                            pending: false,
                        })
                            as Box<dyn Read>));
                    }
                    spans.push(ProgressSpan {
                        name: relative.clone(),
                        start: block_offset,
                        end: block_offset + file_bytes,
                    });
                    block_offset += file_bytes;
                    last_file = relative.clone();
                    last_size = file_bytes;
                }
                if track_chunks {
                    // 先报一次块起点，界面在第一个编码块完成前也能显示文件。
                    let (current, current_done, current_total, done_entries, output_bytes) = {
                        let mut state = chunk_state.borrow_mut();
                        state.begin(handled_bytes, block_offset, handled_files, spans);
                        let (current, current_done, current_total, done_entries) = state.locate(0);
                        (
                            current,
                            current_done,
                            current_total,
                            done_entries,
                            state.output_bytes,
                        )
                    };
                    (progress_cell.borrow_mut())(CreateProgress {
                        current,
                        bytes_done: handled_bytes,
                        bytes_total,
                        current_bytes_done: current_done,
                        current_bytes_total: current_total,
                        output_bytes,
                        entries_done: done_entries,
                        entries_total,
                    });
                }
                let pushed = writer.push_archive_entries(entries, readers);
                if track_chunks {
                    chunk_state.borrow_mut().active = false;
                }
                pushed.map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
                handled_bytes += block_offset;
                handled_files += block.len();
            }
            report.files = entries_total;
            report.bytes = bytes_total;
            (progress_cell.borrow_mut())(CreateProgress {
                current: last_file,
                bytes_done: bytes_total,
                bytes_total,
                current_bytes_done: last_size,
                current_bytes_total: last_size,
                output_bytes: fs::metadata(output).map(|meta| meta.len()).unwrap_or(0),
                entries_done: entries_total,
                entries_total,
            });
        } else {
            for (relative, source, _) in plan.files {
                ensure_not_cancelled(&options.cancel)?;
                let mut entry = sevenz_rust2::ArchiveEntry::from_path(&source, relative.clone());
                if let Some(time) = fixed_time {
                    apply_entry_time(&mut entry, time);
                }
                let file_bytes = fs::metadata(&source)?.len();
                let mut reported_complete = false;
                if track_chunks {
                    {
                        let mut state = chunk_state.borrow_mut();
                        state.begin(
                            report.bytes,
                            file_bytes,
                            report.files,
                            vec![ProgressSpan {
                                name: relative.clone(),
                                start: 0,
                                end: file_bytes,
                            }],
                        );
                    }
                    let mut reader = CancelReader {
                        inner: File::open(&source)?,
                        cancel: options.cancel.clone(),
                    };
                    let pushed = writer.push_archive_entry(entry, Some(&mut reader));
                    let chunks = {
                        let mut state = chunk_state.borrow_mut();
                        state.active = false;
                        state.chunks
                    };
                    pushed.map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
                    // 最后一个编码块已经上报过 100%，不再补发重复的完成事件；
                    // 空文件没有编码块，仍走下面的完成事件。
                    reported_complete =
                        chunks > 0 && chunks.saturating_mul(chunk_bytes) >= file_bytes;
                } else {
                    let mut reader = ProgressReader {
                        inner: File::open(&source)?,
                        current: relative.clone(),
                        total: file_bytes,
                        done: 0,
                        overall_base: report.bytes,
                        bytes_total,
                        entries_done: report.files,
                        entries_total,
                        output: output.to_path_buf(),
                        cancel: options.cancel.clone(),
                        progress: progress_cell.clone(),
                        last_report: Instant::now() - Duration::from_secs(1),
                        pending: false,
                    };
                    writer
                        .push_archive_entry(entry, Some(&mut reader))
                        .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
                }
                report.files += 1;
                report.bytes += file_bytes;
                if !reported_complete {
                    (progress_cell.borrow_mut())(CreateProgress {
                        current: relative,
                        bytes_done: report.bytes,
                        bytes_total,
                        current_bytes_done: file_bytes,
                        current_bytes_total: file_bytes,
                        output_bytes: fs::metadata(output).map(|meta| meta.len()).unwrap_or(0),
                        entries_done: report.files,
                        entries_total,
                    });
                }
            }
        }
        writer
            .finish()
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        Ok(report)
    }

    fn create_tar<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let level = options.level.unwrap_or(6).clamp(1, 9) as u32;
        let mut report = CreateReport {
            output: output.to_path_buf(),
            ..Default::default()
        };
        match options.format {
            ArchiveFormat::Tar => {
                let mut builder = tar::Builder::new(File::create(output)?);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.finish()?;
            }
            ArchiveFormat::TarGz => {
                let encoder = flate2::write::GzEncoder::new(
                    File::create(output)?,
                    flate2::Compression::new(level),
                );
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarBz2 => {
                let encoder = bzip2::write::BzEncoder::new(
                    File::create(output)?,
                    bzip2::Compression::new(level),
                );
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarXz => {
                let encoder = xz2::write::XzEncoder::new(File::create(output)?, level);
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarZst => {
                let encoder =
                    zstd::stream::write::Encoder::new(File::create(output)?, level as i32)?;
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            _ => {
                return Err(ArchiveError::UnsupportedFormat(
                    options.format.label().to_string(),
                ));
            }
        }
        Ok(report)
    }

    fn create_single_stream<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        let source = sources
            .first()
            .filter(|path| path.is_file())
            .ok_or_else(|| ArchiveError::Corrupt("单文件流格式只支持一个源文件".into()))?;
        if sources.len() != 1 {
            return Err(ArchiveError::Corrupt(
                "GZ/BZ2/XZ/ZST 写入只支持一个源文件".into(),
            ));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let total = fs::metadata(source)?.len();
        let mut input = File::open(source)?;
        let destination = File::create(output)?;
        let level = options.level.unwrap_or(6).clamp(1, 9) as u32;
        let copied = match options.format {
            ArchiveFormat::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(destination, flate2::Compression::new(level));
                let copied = copy_with_progress(&mut input, &mut encoder, total, source, progress)?;
                encoder.finish()?;
                copied
            }
            ArchiveFormat::Bzip2 => {
                let mut encoder =
                    bzip2::write::BzEncoder::new(destination, bzip2::Compression::new(level));
                let copied = copy_with_progress(&mut input, &mut encoder, total, source, progress)?;
                encoder.finish()?;
                copied
            }
            ArchiveFormat::Xz => {
                let mut encoder = xz2::write::XzEncoder::new(destination, level);
                let copied = copy_with_progress(&mut input, &mut encoder, total, source, progress)?;
                encoder.finish()?;
                copied
            }
            ArchiveFormat::Zstd => {
                let mut encoder = zstd::stream::write::Encoder::new(destination, level as i32)?;
                let copied = copy_with_progress(&mut input, &mut encoder, total, source, progress)?;
                encoder.finish()?;
                copied
            }
            ArchiveFormat::Brotli => {
                let mut encoder = brotli::CompressorWriter::new(destination, 64 * 1024, level, 22);
                let copied = copy_with_progress(&mut input, &mut encoder, total, source, progress)?;
                encoder.into_inner();
                copied
            }
            ArchiveFormat::Lz4 => {
                let mut encoder = lz4_flex::frame::FrameEncoder::new(destination);
                let copied = copy_with_progress(&mut input, &mut encoder, total, source, progress)?;
                encoder
                    .finish()
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
                copied
            }
            _ => {
                return Err(ArchiveError::UnsupportedFormat(
                    options.format.label().to_string(),
                ));
            }
        };
        Ok(CreateReport {
            files: 1,
            bytes: copied,
            output: output.to_path_buf(),
            cleanup_errors: Vec::new(),
        })
    }
}

/// 固实块内单个文件在块内字节轴上的位置。
struct ProgressSpan {
    name: String,
    start: u64,
    end: u64,
}

/// 一次编码阶段的真实进度换算状态。
///
/// 多线程 LZMA2 每完成一个块才向输出写一次，`chunks` 记录已写出的块数；
/// `locate` 把“已压缩字节”换算成当前文件和已完成条目，避免直接使用读取
/// 位置导致进度提前冲到 100%。
struct ChunkProgressState {
    active: bool,
    base: u64,
    total: u64,
    chunk_bytes: u64,
    chunks: u64,
    entries_before: usize,
    entries_total: usize,
    bytes_total: u64,
    output_bytes: u64,
    spans: Vec<ProgressSpan>,
}

impl ChunkProgressState {
    /// 开始一个新的固实块或单文件阶段，并重置块内计数。
    fn begin(&mut self, base: u64, total: u64, entries_before: usize, spans: Vec<ProgressSpan>) {
        self.active = true;
        self.base = base;
        self.total = total;
        self.chunks = 0;
        self.entries_before = entries_before;
        self.spans = spans;
    }

    /// 把块内已压缩字节换算成（文件名、文件内已完成、文件总大小、总完成条目数）。
    fn locate(&self, done: u64) -> (String, u64, u64, usize) {
        let done = done.min(self.total);
        let index = self.spans.partition_point(|span| span.end <= done);
        let entries_done = self.entries_before + index;
        if let Some(span) = self.spans.get(index) {
            (
                span.name.clone(),
                done.saturating_sub(span.start),
                span.end.saturating_sub(span.start),
                entries_done,
            )
        } else if let Some(span) = self.spans.last() {
            let size = span.end.saturating_sub(span.start);
            (
                span.name.clone(),
                size,
                size,
                self.entries_before + self.spans.len(),
            )
        } else {
            (String::new(), 0, 0, entries_done)
        }
    }
}

/// 包装 7Z 输出文件：统计写入字节，并在每个编码块完成时上报真实进度。
///
/// 多线程 LZMA2 的块结果按顺序各写一次，因此 `write` 的调用次数就是已完成
/// 块数。只在 `active` 阶段计数，`finish()` 写文件头时不会误报。
struct ChunkTrackingWriter<'a, F>
where
    F: FnMut(CreateProgress),
{
    inner: File,
    state: Rc<RefCell<ChunkProgressState>>,
    progress: Rc<RefCell<&'a mut F>>,
}

impl<F> Write for ChunkTrackingWriter<'_, F>
where
    F: FnMut(CreateProgress),
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        // 一次性写完，保证调用方的 write_all 只触发一次，和编码块一一对应。
        self.inner.write_all(buffer)?;
        let mut state = self.state.borrow_mut();
        state.output_bytes = state.output_bytes.saturating_add(buffer.len() as u64);
        // 1 字节的 LZMA2 流结束标记不是编码块，不能计入已完成块数。
        if !state.active || buffer.len() < 3 {
            return Ok(buffer.len());
        }
        state.chunks = state.chunks.saturating_add(1);
        let done = (state.chunks.saturating_mul(state.chunk_bytes)).min(state.total);
        let (current, current_done, current_total, entries_done) = state.locate(done);
        let event = CreateProgress {
            current,
            bytes_done: state.base + done,
            bytes_total: state.bytes_total,
            current_bytes_done: current_done,
            current_bytes_total: current_total,
            output_bytes: state.output_bytes,
            entries_done,
            entries_total: state.entries_total,
        };
        drop(state);
        (self.progress.borrow_mut())(event);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<F> Seek for ChunkTrackingWriter<'_, F>
where
    F: FnMut(CreateProgress),
{
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

/// 只负责取消检查的读取器；进度由输出写入器按完成块上报。
struct CancelReader {
    inner: File,
    cancel: Option<Arc<AtomicBool>>,
}

impl Read for CancelReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(&self.cancel) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "compression cancelled",
            ));
        }
        self.inner.read(buffer)
    }
}

/// 在 7Z 编码器读取源文件时按块上报当前文件进度并响应取消。
struct ProgressReader<'a, F>
where
    F: FnMut(CreateProgress),
{
    inner: File,
    current: String,
    total: u64,
    done: u64,
    overall_base: u64,
    bytes_total: u64,
    entries_done: usize,
    entries_total: usize,
    output: PathBuf,
    cancel: Option<Arc<AtomicBool>>,
    progress: Rc<RefCell<&'a mut F>>,
    last_report: Instant,
    /// 上一次 read 返回的数据还没上报；下次 read 时它已经被写入编码器。
    pending: bool,
}

impl<F> Read for ProgressReader<'_, F>
where
    F: FnMut(CreateProgress),
{
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(&self.cancel) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "compression cancelled",
            ));
        }
        // 调用方在上一次 read 之后已经把这些字节写进编码器，这里才把
        // 它们算作已完成字节，避免进度按读取位置提前。
        if self.pending
            && self.done < self.total
            && self.last_report.elapsed() >= Duration::from_millis(100)
        {
            self.pending = false;
            self.last_report = Instant::now();
            (self.progress.borrow_mut())(CreateProgress {
                current: self.current.clone(),
                bytes_done: self.overall_base + self.done,
                bytes_total: self.bytes_total,
                current_bytes_done: self.done,
                current_bytes_total: self.total,
                output_bytes: fs::metadata(&self.output)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                entries_done: self.entries_done,
                entries_total: self.entries_total,
            });
        }
        let read = self.inner.read(buffer)?;
        if read > 0 {
            self.done += read as u64;
            self.pending = true;
        }
        Ok(read)
    }
}

/// 固实压缩共用的进度读取器。
///
/// 固实块会先一次性提交多个文件，再统一压缩；每个读取器共享同一个进度
/// 回调和已完成条目计数，因此压缩过程中仍能按实际读取字节更新总进度。
struct SharedProgressReader<'a, F>
where
    F: FnMut(CreateProgress),
{
    inner: File,
    current: String,
    total: u64,
    done: u64,
    overall_base: u64,
    bytes_total: u64,
    entries_done: Rc<Cell<usize>>,
    entries_total: usize,
    output: PathBuf,
    cancel: Option<Arc<AtomicBool>>,
    progress: Rc<RefCell<&'a mut F>>,
    last_report: Instant,
    finished: bool,
    /// 上一次 read 返回的数据还没上报；下次 read 时它已经被写入编码器。
    pending: bool,
}

impl<F> Read for SharedProgressReader<'_, F>
where
    F: FnMut(CreateProgress),
{
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(&self.cancel) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "compression cancelled",
            ));
        }
        // 只有等调用方把上一批数据写进编码器后，才把它们计入已完成字节；
        // 文件末尾由 finished_now 事件一次性补齐。
        if self.pending
            && self.done < self.total
            && self.last_report.elapsed() >= Duration::from_millis(100)
        {
            self.pending = false;
            self.last_report = Instant::now();
            (self.progress.borrow_mut())(CreateProgress {
                current: self.current.clone(),
                bytes_done: self.overall_base + self.done,
                bytes_total: self.bytes_total,
                current_bytes_done: self.done,
                current_bytes_total: self.total,
                output_bytes: fs::metadata(&self.output)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                entries_done: self.entries_done.get(),
                entries_total: self.entries_total,
            });
        }
        let read = self.inner.read(buffer)?;
        if read > 0 {
            self.done += read as u64;
            self.pending = true;
        }
        let finished_now = read == 0 && !self.finished;
        if finished_now {
            self.finished = true;
            self.entries_done
                .set(self.entries_done.get().saturating_add(1));
            self.last_report = Instant::now();
            (self.progress.borrow_mut())(CreateProgress {
                current: self.current.clone(),
                bytes_done: self.overall_base + self.done,
                bytes_total: self.bytes_total,
                current_bytes_done: self.done,
                current_bytes_total: self.total,
                output_bytes: fs::metadata(&self.output)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                entries_done: self.entries_done.get(),
                entries_total: self.entries_total,
            });
        }
        Ok(read)
    }
}

/// 把系统时间转换为 ZIP 可写入的本地时间。
fn zip_datetime(time: SystemTime) -> Option<zip::DateTime> {
    let local: DateTime<chrono::Local> = time.into();
    zip::DateTime::try_from(local.naive_local()).ok()
}

/// 把系统时间写入 7Z 条目的修改时间字段。
fn apply_entry_time(entry: &mut sevenz_rust2::ArchiveEntry, time: SystemTime) {
    if let Ok(date) = NtTime::try_from(time) {
        entry.last_modified_date = date;
        entry.has_last_modified_date = true;
    }
}

fn zip_method(options: &CompressionOptions) -> CompressionMethod {
    match options.zip_compression {
        crate::format::ZipCompression::Deflate => CompressionMethod::Deflated,
        crate::format::ZipCompression::Bzip2 => CompressionMethod::Bzip2,
        crate::format::ZipCompression::Lzma => CompressionMethod::Lzma,
    }
}

fn zip_time_to_chrono(value: Option<zip::DateTime>) -> Option<DateTime<Utc>> {
    let value = value?;
    let date = NaiveDate::from_ymd_opt(
        value.year().into(),
        value.month().into(),
        value.day().into(),
    )?;
    let time = date.and_hms_opt(
        value.hour().into(),
        value.minute().into(),
        value.second().into(),
    )?;
    // ZIP 的 DOS 时间字段不带时区，写入时按本地时间记录；读取必须同样按
    // 本地时间解释，否则条目时间会整体偏移，更新模式的新旧比较也会失真。
    let local = Local.from_local_datetime(&time).earliest()?;
    Some(local.with_timezone(&Utc))
}

fn selected(name: &str, selected_paths: &[String]) -> bool {
    if selected_paths.is_empty() {
        return true;
    }
    let name = name.replace('\\', "/");
    selected_paths.iter().any(|selected| {
        let selected = selected.replace('\\', "/");
        name == selected || name.starts_with(&format!("{selected}/"))
    })
}

struct SourcePlan {
    /// 归档内相对路径、源文件路径和规划时的字节数。
    files: Vec<(String, PathBuf, u64)>,
    total_bytes: u64,
    latest_modified: Option<SystemTime>,
}

/// 组装自解压包尾部页脚：魔数 + 数据段偏移 + 数据段长度。
fn sfx_footer(offset: u64, length: u64) -> [u8; SFX_FOOTER_LEN] {
    let mut footer = [0u8; SFX_FOOTER_LEN];
    footer[..8].copy_from_slice(&SFX_MAGIC);
    footer[8..16].copy_from_slice(&offset.to_le_bytes());
    footer[16..24].copy_from_slice(&length.to_le_bytes());
    footer
}

fn build_source_plan(
    sources: &[PathBuf],
    options: &CompressionOptions,
) -> Result<SourcePlan, ArchiveError> {
    let base = common_base(sources);
    let mut files = Vec::new();
    let mut total_bytes = 0;
    let mut latest_modified: Option<SystemTime> = None;
    for source in sources {
        for entry in WalkDir::new(source).follow_links(false) {
            let entry = entry.map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            if entry.file_type().is_dir() {
                continue;
            }
            let path = entry.path();
            let mut relative = path
                .strip_prefix(&base)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            relative = match options.path_mode {
                crate::format::PathMode::NoRoot => relative
                    .split_once('/')
                    .map(|(_, remainder)| remainder.to_string())
                    .unwrap_or(relative),
                crate::format::PathMode::Flatten => path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(ToString::to_string)
                    .unwrap_or(relative),
                crate::format::PathMode::Relative => relative,
            };
            if options
                .exclude_patterns
                .iter()
                .any(|pattern| glob_match(pattern, &relative))
            {
                continue;
            }
            let metadata = entry.metadata().ok();
            let raw_modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok());
            if let Some(time) = raw_modified
                && latest_modified.is_none_or(|latest| time > latest)
            {
                latest_modified = Some(time);
            }
            let modified = raw_modified.map(DateTime::<Utc>::from);
            if options
                .modified_after
                .is_some_and(|after| modified.is_none_or(|value| value < after))
                || options
                    .modified_before
                    .is_some_and(|before| modified.is_none_or(|value| value > before))
            {
                continue;
            }
            let file_bytes = metadata.map(|metadata| metadata.len()).unwrap_or(0);
            total_bytes += file_bytes;
            files.push((relative, path.to_path_buf(), file_bytes));
        }
    }
    Ok(SourcePlan {
        files,
        total_bytes,
        latest_modified,
    })
}

/// 多线程 LZMA2 的目标块大小：块越小，进度更新越连续、在途缓冲越少。
///
/// 库会把块大小钳到不小于字典大小；调用方显式设置字典时，建块级别还会再
/// 限制到 7 级，避免 8/9 级预设字典把块抬到 32/64 MiB。
const LZMA2_CHUNK_BYTES: u64 = 16 * 1024 * 1024;

/// lzma-rust2 预设级别对应的字典大小，用于推算多线程 LZMA2 的实际块大小。
///
/// 固定依赖 sevenz-rust2 0.24.1 / lzma-rust2 0.23.0；升级压缩库时需要同步
/// 检查这张表与 `from_level_mt` 的钳制逻辑。
fn lzma2_preset_dictionary(level: u32) -> u64 {
    const PRESET_DICTIONARY: [u64; 10] = [
        256 * 1024,
        1024 * 1024,
        2 * 1024 * 1024,
        4 * 1024 * 1024,
        4 * 1024 * 1024,
        8 * 1024 * 1024,
        8 * 1024 * 1024,
        16 * 1024 * 1024,
        32 * 1024 * 1024,
        64 * 1024 * 1024,
    ];
    PRESET_DICTIONARY[level.min(9) as usize]
}

/// 固实块的默认字节上限；调用方没有设置“固实块大小”时使用 2 GiB。
const DEFAULT_SOLID_BLOCK_BYTES: u64 = 2048 * 1024 * 1024;

/// 单个固实块最多包含的文件数，作为超大目录的兜底限制。
///
/// 正常情况下由“固实块大小”决定块边界；这个上限只在文件极多且极小时生效，
/// 避免一个块一次性打开几十万个句柄。
const SOLID_BLOCK_MAX_FILES: usize = 16384;

/// 把待压缩文件切成固实块，同时受字节上限和文件数上限约束。
///
/// 7Z 的固实块需要先打开块内全部源文件再统一压缩，块越大占用越多；单独
/// 一个文件已经超过字节上限时让它独占一块，否则整个块会被它无限拉长。
fn solid_blocks(
    files: &[(String, PathBuf, u64)],
    block_bytes: u64,
    block_files: usize,
) -> Vec<&[(String, PathBuf, u64)]> {
    let mut blocks = Vec::new();
    let mut start = 0;
    let mut block_size = 0u64;
    for (index, (_, _, file_bytes)) in files.iter().enumerate() {
        let filled = index - start;
        let too_large = block_size.saturating_add(*file_bytes) > block_bytes;
        let too_many = filled + 1 > block_files;
        if filled > 0 && (too_large || too_many) {
            blocks.push(&files[start..index]);
            start = index;
            block_size = 0;
        }
        block_size = block_size.saturating_add(*file_bytes);
    }
    if start < files.len() {
        blocks.push(&files[start..]);
    }
    blocks
}

fn common_base(sources: &[PathBuf]) -> PathBuf {
    let Some(first) = sources.first() else {
        return PathBuf::from(".");
    };
    let mut base = if first.is_dir() {
        first.parent().unwrap_or(first).to_path_buf()
    } else {
        first.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    for source in sources.iter().skip(1) {
        while !source.starts_with(&base) {
            if !base.pop() {
                return PathBuf::from(".");
            }
        }
    }
    base
}

fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.replace('\\', "/").to_lowercase();
    let value = value.replace('\\', "/").to_lowercase();
    if let Ok(glob) = globset::Glob::new(&pattern) {
        glob.compile_matcher().is_match(&value)
    } else {
        false
    }
}

fn tar_reader(
    path: &Path,
    format: ArchiveFormat,
) -> Result<tar::Archive<Box<dyn Read>>, ArchiveError> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::GzDecoder::new(file)),
        ArchiveFormat::TarBz2 => Box::new(bzip2::read::BzDecoder::new(file)),
        ArchiveFormat::TarXz => Box::new(xz2::read::XzDecoder::new(file)),
        ArchiveFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(file)?),
        _ => Box::new(file),
    };
    Ok(tar::Archive::new(reader))
}

fn append_tar_sources<F>(
    builder: &mut tar::Builder<impl Write>,
    sources: &[PathBuf],
    options: &CompressionOptions,
    progress: &mut F,
    report: &mut CreateReport,
) -> Result<(), ArchiveError>
where
    F: FnMut(CreateProgress),
{
    let plan = build_source_plan(sources, options)?;
    let entries_total = plan.files.len();
    let mut done = 0;
    for (relative, source, _) in plan.files {
        ensure_not_cancelled(&options.cancel)?;
        builder.append_path_with_name(&source, &relative)?;
        let file_bytes = fs::metadata(&source)?.len();
        done += file_bytes;
        report.files += 1;
        report.bytes = done;
        progress(CreateProgress {
            current: relative,
            bytes_done: done,
            bytes_total: plan.total_bytes,
            current_bytes_done: file_bytes,
            current_bytes_total: file_bytes,
            output_bytes: 0,
            entries_done: report.files,
            entries_total,
        });
    }
    Ok(())
}

fn copy_with_progress<F, W: Write>(
    input: &mut File,
    output: &mut W,
    total: u64,
    name: &Path,
    progress: &mut F,
) -> Result<u64, ArchiveError>
where
    F: FnMut(CreateProgress),
{
    let mut done = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        done += read as u64;
        progress(CreateProgress {
            current: name.display().to_string(),
            bytes_done: done,
            bytes_total: total,
            current_bytes_done: done,
            current_bytes_total: total,
            output_bytes: 0,
            entries_done: 0,
            entries_total: 1,
        });
    }
    Ok(done)
}

fn decompress_reader(path: &Path, format: ArchiveFormat) -> Result<Box<dyn Read>, ArchiveError> {
    let file = File::open(path)?;
    Ok(match format {
        ArchiveFormat::Gzip => Box::new(flate2::read::GzDecoder::new(file)),
        ArchiveFormat::Bzip2 => Box::new(bzip2::read::BzDecoder::new(file)),
        ArchiveFormat::Xz => Box::new(xz2::read::XzDecoder::new(file)),
        ArchiveFormat::Zstd => Box::new(zstd::stream::read::Decoder::new(file)?),
        ArchiveFormat::Brotli => Box::new(brotli::Decompressor::new(file, 64 * 1024)),
        ArchiveFormat::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(file)),
        other => return Err(ArchiveError::UnsupportedFormat(other.label().to_string())),
    })
}

fn open_unix_compress(
    path: &Path,
) -> Result<
    (
        unarc_rs::unified::UnifiedArchive<BufReader<File>>,
        unarc_rs::unified::ArchiveEntry,
    ),
    ArchiveError,
> {
    let file = File::open(path)?;
    let mut archive = unarc_rs::unified::UnifiedArchive::open_with_format(
        BufReader::new(file),
        unarc_rs::unified::ArchiveFormat::Z,
    )
    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
    let entry = archive
        .next_entry()
        .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
        .ok_or_else(|| ArchiveError::Corrupt("Z 文件不包含可解压数据".into()))?;
    Ok((archive, entry))
}

fn decrypt_encrypted_container(
    path: &Path,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, ArchiveError> {
    let mut input = File::open(path)?;
    let mut data = Zeroizing::new(Vec::new());
    geezipx_core::archive::aes::aes_decrypt(&mut input, &mut *data, password)
        .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
    Ok(data)
}

fn encrypted_container_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("output")
        .to_string()
}

fn unix_compress_name(path: &Path, embedded: &str) -> String {
    if !embedded.trim().is_empty() {
        return embedded.replace('\\', "/");
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    if name.len() > 2
        && name
            .get(name.len() - 2..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".Z"))
    {
        name.get(..name.len() - 2).unwrap_or(name).to_string()
    } else {
        name.to_string()
    }
}

fn is_cancelled(flag: &Option<Arc<AtomicBool>>) -> bool {
    flag.as_ref()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn rar_error_means_encrypted(error: &unrar::error::UnrarError) -> bool {
    use unrar::error::{Code, When};

    matches!(error.code, Code::MissingPassword | Code::BadPassword)
        || (error.when == When::Open && error.code == Code::UnknownFormat)
}

fn map_rar_error(error: unrar::error::UnrarError) -> ArchiveError {
    match error.code {
        unrar::error::Code::MissingPassword => ArchiveError::PasswordRequired,
        unrar::error::Code::BadPassword => ArchiveError::WrongPassword,
        _ => ArchiveError::Corrupt(error.to_string()),
    }
}

fn ensure_not_cancelled(flag: &Option<Arc<AtomicBool>>) -> Result<(), ArchiveError> {
    if is_cancelled(flag) {
        Err(ArchiveError::Cancelled)
    } else {
        Ok(())
    }
}

fn is_legacy_archive(format: ArchiveFormat) -> bool {
    matches!(
        format,
        ArchiveFormat::Arc
            | ArchiveFormat::Arj
            | ArchiveFormat::Ace
            | ArchiveFormat::Cab
            | ArchiveFormat::Cpio
            | ArchiveFormat::Iso
            | ArchiveFormat::Udf
            | ArchiveFormat::Wim
            | ArchiveFormat::Asar
            | ArchiveFormat::Zpaq
            | ArchiveFormat::Lzh
            | ArchiveFormat::Deb
    )
}

fn open_legacy_reader(
    path: &Path,
    format: ArchiveFormat,
) -> Result<Box<dyn LegacyReader>, ArchiveError> {
    use geezipx_core::archive::{
        ace::AceReader, arc::ArcReader, arj::ArjReader, asar::AsarReader, cab::CabReader,
        cpio::CpioReader, deb::DebReader, iso::IsoReader, lzh::LzhReader, udf::UdfReader,
        wim::WimReader, zpaq::ZpaqReader,
    };

    let reader: Box<dyn LegacyReader> = match format {
        ArchiveFormat::Ace => Box::new(AceReader::new(path)),
        ArchiveFormat::Arc => Box::new(ArcReader::new(path)),
        ArchiveFormat::Arj => Box::new(ArjReader::new(path)),
        ArchiveFormat::Asar => Box::new(AsarReader::new(path)),
        ArchiveFormat::Cab => Box::new(CabReader::new(path)),
        ArchiveFormat::Cpio => Box::new(CpioReader::new(path)),
        ArchiveFormat::Deb => Box::new(DebReader::new(BufReader::new(File::open(path)?))),
        ArchiveFormat::Iso => Box::new(IsoReader::new(BufReader::new(File::open(path)?))),
        ArchiveFormat::Lzh => Box::new(LzhReader::new(BufReader::new(File::open(path)?))),
        ArchiveFormat::Udf => Box::new(
            UdfReader::new(path).map_err(|error| ArchiveError::Corrupt(error.to_string()))?,
        ),
        ArchiveFormat::Wim => Box::new(
            WimReader::open(path).map_err(|error| ArchiveError::Corrupt(error.to_string()))?,
        ),
        ArchiveFormat::Zpaq => Box::new(ZpaqReader::new(path)),
        other => {
            return Err(ArchiveError::UnsupportedFormat(other.label().to_string()));
        }
    };
    Ok(reader)
}

fn copy_cancellable<R: Read + ?Sized, W: Write>(
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

/// 边复制边按 100 毫秒节流回调已复制字节。
///
/// `for_each_entries` 只在条目结束时给一次回调，单个大文件解压时进度条会
/// 长时间停在起点；这里按块上报，让解压进度和压缩进度一样可见。`remaining`
/// 是本次条目还能写出的字节数，超出时返回资源预算错误。
fn copy_entry_with_progress<R: Read + ?Sized, W: Write>(
    input: &mut R,
    output: &mut W,
    cancel: &Option<Arc<AtomicBool>>,
    remaining: u64,
    mut progress: impl FnMut(u64),
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    let mut last_report = Instant::now() - Duration::from_secs(1);
    loop {
        ensure_not_cancelled(cancel)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if total.saturating_add(read as u64) > remaining {
            return Err(ArchiveError::BudgetExceeded(format!(
                "解压数据超过剩余预算 {remaining} 字节"
            )));
        }
        output.write_all(&buffer[..read])?;
        total += read as u64;
        if last_report.elapsed() >= Duration::from_millis(100) {
            last_report = Instant::now();
            progress(total);
        }
    }
    Ok(total)
}

/// 解压预算检查：条目数和累计字节。
///
/// 条目大小来自归档元数据，可能小于实际解压量；真正的写入上限由
/// `copy_cancellable_limited` 在复制时兜底。
fn ensure_budget(
    options: &ExtractionOptions,
    report: &ExtractReport,
    next_bytes: u64,
) -> Result<(), ArchiveError> {
    let budget = &options.budget;
    if report.files >= budget.max_entries {
        return Err(ArchiveError::BudgetExceeded(format!(
            "条目数超过上限 {}",
            budget.max_entries
        )));
    }
    if report.bytes.saturating_add(next_bytes) > budget.max_bytes {
        return Err(ArchiveError::BudgetExceeded(format!(
            "解压总量将超过 {} 字节上限",
            budget.max_bytes
        )));
    }
    Ok(())
}

/// 在剩余预算内复制数据；超出时立即失败，避免先写满磁盘再报错。
fn copy_cancellable_limited<R: Read + ?Sized, W: Write>(
    input: &mut R,
    output: &mut W,
    cancel: &Option<Arc<AtomicBool>>,
    remaining: u64,
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        ensure_not_cancelled(cancel)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if total.saturating_add(read as u64) > remaining {
            return Err(ArchiveError::BudgetExceeded(format!(
                "解压数据超过剩余预算 {remaining} 字节"
            )));
        }
        output.write_all(&buffer[..read])?;
        total += read as u64;
    }
    Ok(total)
}

fn drain_seven_zip_entry(
    input: &mut dyn Read,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<u64, ArchiveError> {
    let mut sink = io::sink();
    copy_cancellable(input, &mut sink, cancel)
}

struct CancelWriter<'a> {
    inner: File,
    cancel: &'a Option<Arc<AtomicBool>>,
}

impl Write for CancelWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if is_cancelled(self.cancel) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "task cancelled"));
        }
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn unique_destination(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..10_000 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

fn strip_volume_suffix(path: &Path) -> PathBuf {
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

pub(crate) fn is_volume_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, suffix)| suffix == "001")
}

fn volume_paths(path: &Path) -> Result<Option<Vec<PathBuf>>, ArchiveError> {
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

fn materialize_input(path: &Path) -> Result<(PathBuf, Option<tempfile::TempDir>), ArchiveError> {
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

fn ensure_sources_safe_to_delete(sources: &[PathBuf], output: &Path) -> Result<(), ArchiveError> {
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

fn absolute_for_comparison(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Some(canonical);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).ok()?;
    Some(parent.join(path.file_name()?))
}

fn remove_source_paths(sources: &[PathBuf], errors: &mut Vec<String>) {
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

fn split_file(source: &Path, base: &Path, split_size: u64) -> Result<Vec<PathBuf>, ArchiveError> {
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

fn commit_split_volumes(
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

fn restore_split_backups(backups: &[(PathBuf, PathBuf)]) {
    for (target, backup) in backups.iter().rev() {
        let _ = fs::rename(backup, target);
    }
}

fn sibling_volume_paths(path: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
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

#[cfg(windows)]
fn atomic_replace_file(temp: &Path, output: &Path) -> io::Result<()> {
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

#[cfg(not(windows))]
fn atomic_replace_file(temp: &Path, output: &Path) -> io::Result<()> {
    fs::rename(temp, output)
}

#[cfg(test)]
mod solid_block_tests {
    use super::{DEFAULT_SOLID_BLOCK_BYTES, solid_blocks};
    use std::path::PathBuf;

    /// 生成只带大小信息的占位文件列表，测试不落盘。
    fn files(sizes: &[u64]) -> Vec<(String, PathBuf, u64)> {
        sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                (
                    format!("f{index}"),
                    PathBuf::from(format!("f{index}")),
                    *size,
                )
            })
            .collect()
    }

    /// 文件数超过上限时按上限切块。
    #[test]
    fn splits_by_file_count() {
        let plan = files(&[1; 5]);
        let blocks = solid_blocks(&plan, DEFAULT_SOLID_BLOCK_BYTES, 2);
        assert_eq!(
            blocks.iter().map(|block| block.len()).collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
    }

    /// 累计字节超过上限时切块，超过上限的单文件独占一块。
    #[test]
    fn splits_by_bytes_and_keeps_oversized_file_alone() {
        let plan = files(&[600, 600, 5000, 100]);
        let blocks = solid_blocks(&plan, 1000, 100);
        assert_eq!(
            blocks.iter().map(|block| block.len()).collect::<Vec<_>>(),
            vec![1, 1, 1, 1]
        );
    }

    /// 小目录仍然只有一个固实块，保持既有压缩结果。
    #[test]
    fn keeps_small_plan_in_one_block() {
        let plan = files(&[10, 20, 30]);
        let blocks = solid_blocks(&plan, DEFAULT_SOLID_BLOCK_BYTES, 4096);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].len(), 3);
    }
}

#[cfg(test)]
mod progress_tests {
    use std::io::{Read, sink};
    use std::time::Duration;

    /// 慢速读取器：每次只返回一小块并停顿，模拟大文件解压的分块读取。
    struct SlowReader {
        remaining: usize,
    }

    impl Read for SlowReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(30));
            let size = buffer.len().min(self.remaining).min(1024);
            if size == 0 {
                return Ok(0);
            }
            self.remaining -= size;
            Ok(size)
        }
    }

    /// 已完成字节落在一段文件中间时，应换算成该文件的内部进度。
    #[test]
    fn chunk_state_maps_done_bytes_to_current_file() {
        let mut state = super::ChunkProgressState {
            active: false,
            base: 100,
            total: 0,
            chunk_bytes: 16,
            chunks: 0,
            entries_before: 2,
            entries_total: 5,
            bytes_total: 130,
            output_bytes: 0,
            spans: Vec::new(),
        };
        state.begin(
            100,
            30,
            2,
            vec![
                super::ProgressSpan {
                    name: "a.bin".to_string(),
                    start: 0,
                    end: 10,
                },
                super::ProgressSpan {
                    name: "b.bin".to_string(),
                    start: 10,
                    end: 30,
                },
            ],
        );
        let (name, done, total, entries) = state.locate(4);
        assert_eq!((name.as_str(), done, total, entries), ("a.bin", 4, 10, 2));
        let (name, done, total, entries) = state.locate(16);
        assert_eq!((name.as_str(), done, total, entries), ("b.bin", 6, 20, 3));
        let (name, done, total, entries) = state.locate(30);
        assert_eq!((name.as_str(), done, total, entries), ("b.bin", 20, 20, 4));
    }

    /// 单个大条目在复制过程中应多次回调进度，而不是只在结束时回调一次。
    #[test]
    fn copy_with_progress_reports_inside_single_file() {
        let mut input = SlowReader {
            remaining: 16 * 1024,
        };
        let mut output = sink();
        let mut reports = Vec::new();
        let total =
            super::copy_entry_with_progress(&mut input, &mut output, &None, u64::MAX, |done| {
                reports.push(done);
            })
            .unwrap();
        assert_eq!(total, 16 * 1024);
        assert!(reports.len() >= 2, "单文件内部应多次上报进度：{reports:?}");
        assert!(reports.iter().all(|done| *done > 0 && *done <= total));
    }
}
