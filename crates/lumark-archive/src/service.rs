use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, Utc};
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
        let bytes = match copy_cancellable(&mut input, &mut output, &options.cancel) {
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
            let mut entry = match options.password.as_deref() {
                Some(password) => archive.by_index_decrypt(index, password.as_bytes())?,
                None => archive.by_index(index)?,
            };
            let mut output = File::create(&destination)?;
            if let Err(error) = copy_cancellable(&mut entry, &mut output, &options.cancel) {
                drop(output);
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
            report.files += 1;
            report.bytes += size;
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
                let mut output = CancelWriter {
                    inner: File::create(&destination)?,
                    cancel: &options.cancel,
                };
                if let Err(error) = io::copy(input, &mut output) {
                    drop(output);
                    let _ = fs::remove_file(&destination);
                    return Err(sevenz_rust2::Error::Other(error.to_string().into()));
                }
                report.files += 1;
                report.bytes += entry.size();
                progress(ExtractProgress {
                    current: name,
                    bytes_done: report.bytes,
                    bytes_total,
                    entries_done: report.files,
                    entries_total,
                });
                Ok(true)
            })
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
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
            let mut output = File::create(&destination)?;
            if let Err(error) = copy_cancellable(&mut entry, &mut output, &options.cancel) {
                drop(output);
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
            report.files += 1;
            report.bytes += entry.size();
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
        let bytes = match copy_cancellable(&mut input, &mut output, &options.cancel) {
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
        for (relative, source) in plan.files {
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
        let mut writer = ArchiveWriter::create(output)
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        let level = options.level.unwrap_or(6).clamp(0, 9) as u32;
        let mut methods = match options.sevenz_compression {
            crate::format::SevenZCompression::Lzma2 => {
                let mut lzma2 = if options.threads > 1 {
                    Lzma2Options::from_level_mt(level, options.threads, 64 * 1024 * 1024)
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
        if options.solid && plan.files.len() > 1 {
            let mut entries = Vec::with_capacity(plan.files.len());
            let mut readers = Vec::with_capacity(plan.files.len());
            for (relative, source) in plan.files {
                ensure_not_cancelled(&options.cancel)?;
                let mut entry = sevenz_rust2::ArchiveEntry::from_path(&source, relative.clone());
                if let Some(time) = fixed_time {
                    apply_entry_time(&mut entry, time);
                }
                entries.push(entry);
                readers.push(SourceReader::new(File::open(&source)?));
                report.files += 1;
                let file_bytes = fs::metadata(&source)?.len();
                report.bytes += file_bytes;
                progress(CreateProgress {
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
            writer
                .push_archive_entries(entries, readers)
                .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        } else {
            for (relative, source) in plan.files {
                ensure_not_cancelled(&options.cancel)?;
                let mut entry = sevenz_rust2::ArchiveEntry::from_path(&source, relative.clone());
                if let Some(time) = fixed_time {
                    apply_entry_time(&mut entry, time);
                }
                let file_bytes = fs::metadata(&source)?.len();
                {
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
                        progress: &mut *progress,
                        last_report: Instant::now() - Duration::from_secs(1),
                    };
                    writer
                        .push_archive_entry(entry, Some(&mut reader))
                        .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
                }
                report.files += 1;
                report.bytes += file_bytes;
                progress(CreateProgress {
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
    progress: &'a mut F,
    last_report: Instant,
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
        let read = self.inner.read(buffer)?;
        self.done += read as u64;
        if self.last_report.elapsed() >= Duration::from_millis(100) {
            self.last_report = Instant::now();
            (self.progress)(CreateProgress {
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
    Some(DateTime::<Utc>::from_naive_utc_and_offset(time, Utc))
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
    files: Vec<(String, PathBuf)>,
    total_bytes: u64,
    latest_modified: Option<SystemTime>,
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
            total_bytes += metadata.map(|metadata| metadata.len()).unwrap_or(0);
            files.push((relative, path.to_path_buf()));
        }
    }
    Ok(SourcePlan {
        files,
        total_bytes,
        latest_modified,
    })
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
    for (relative, source) in plan.files {
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
