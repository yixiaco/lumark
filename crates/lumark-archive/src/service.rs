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
use geezipx_core::archive::ArchiveWriter as LegacyWriter;
use geezipx_core::archive::lzh::{LzhCompressionMethod, LzhWriter};
use lzma_rust2::{LzipOptions, LzipWriter, LzmaOptions, LzmaWriter};
use sevenz_rust2::encoder_options::{
    Bzip2Options, Lzma2Options, LzmaOptions as SevenZLzmaOptions, PpmdOptions,
};
use sevenz_rust2::{
    ArchiveReader, ArchiveWriter, EncoderConfiguration, NtTime, Password, SourceReader,
};
use unrar::Archive as UnrarArchive;
use walkdir::WalkDir;
use zeroize::Zeroizing;
use zip::unstable::write::FileOptionsExt;
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

use crate::disguise::{DisguiseFileOptions, DisguiseInfo, DisguiseOptions, DisguisePayloadKind};
use crate::error::ArchiveError;
use crate::format::{
    ArchiveEntry, ArchiveFormat, ArchiveInfo, CompressionOptions, CreateReport, EncryptionInfo,
    ExtractReport, ExtractionBudget, ExtractionOptions, ZipEncryption, detect_format, detect_magic,
};
use crate::path::{ensure_no_symlink_components, safe_join};

/// 自解压包尾部页脚的魔数，用于识别被拼接的运行时模块。
pub const SFX_MAGIC: [u8; 8] = *b"LMSFX001";

/// 自解压包尾部页脚长度：魔数 8 字节 + 数据段偏移 8 字节 + 数据段长度 8 字节。
pub const SFX_FOOTER_LEN: usize = 24;

/// LZH 写入器会把单个条目读入内存；超过该上限时提前拒绝，避免 OOM。
const LZH_MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

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

/// 伪装包的打开方式：读取视图，或已经物化到临时目录的副本。
pub(crate) enum DisguisedInput {
    /// 需要随机访问或流式读取的后端直接读取解密视图，不复制载荷。
    View(crate::disguise::PayloadReader),
    /// 只接受路径的后端使用物化副本。
    Materialized(PathBuf),
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
                | ArchiveFormat::TarBr
                | ArchiveFormat::TarLz4
                | ArchiveFormat::TarLz
                | ArchiveFormat::TarLzma
                | ArchiveFormat::TarZ
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
        // 头部没有魔数时再探测伪装尾部。只有归档载荷才返回 `Disguised`；
        // 原始文件伪装保持普通文件判定，界面通过 `probe_disguise` 提供还原。
        match crate::disguise::probe(&base) {
            Ok(Some(info)) if info.payload_kind == DisguisePayloadKind::Archive => {
                ArchiveFormat::Disguised
            }
            // 魔数命中但字段损坏的伪装文件仍交给打开流程，让用户看到具体原因。
            Err(ArchiveError::Corrupt(_)) | Err(ArchiveError::UnsafePath(_)) => {
                ArchiveFormat::Disguised
            }
            _ => extension_format,
        }
    }

    pub fn probe_encryption(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
        let (input, _guard) = materialize_input(path)?;
        Self::probe_encryption_direct(&input)
    }

    fn probe_encryption_direct(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
        let format = Self::detect(path);
        match format {
            ArchiveFormat::Zip => {
                let encrypted = Self::probe_zip(File::open(path)?)?;
                Ok(EncryptionInfo {
                    encrypted,
                    header_encrypted: false,
                    format: Some(format),
                })
            }
            ArchiveFormat::SevenZip => {
                let (header_encrypted, encrypted) = Self::probe_seven_zip(File::open(path)?)?;
                Ok(EncryptionInfo {
                    encrypted,
                    header_encrypted,
                    format: Some(format),
                })
            }
            ArchiveFormat::Disguised => Self::probe_encryption_disguised(path),
            ArchiveFormat::Rar => Self::probe_rar(path),
            ArchiveFormat::Zpaq => crate::zpaq::probe_encryption(path),
            ArchiveFormat::Alz => crate::alz::probe_encryption(path),
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

    /// 探测 ZIP 归档是否包含加密条目。
    fn probe_zip<R: Read + Seek>(reader: R) -> Result<bool, ArchiveError> {
        let mut archive = ZipArchive::new(reader)?;
        let mut encrypted = false;
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index)?;
            encrypted |= entry.encrypted();
        }
        Ok(encrypted)
    }

    /// 探测 7Z 归档的头加密和条目加密状态。
    fn probe_seven_zip<R: Read + Seek>(reader: R) -> Result<(bool, bool), ArchiveError> {
        let reader = match ArchiveReader::new(reader, Password::empty()) {
            Ok(reader) => reader,
            Err(_) => return Ok((true, true)),
        };
        let has_aes = reader
            .archive()
            .blocks
            .iter()
            .flat_map(|block| block.coders.iter())
            .any(|coder| {
                coder.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256
            });
        Ok((false, has_aes))
    }

    /// 判断伪装包内层格式是否支持不复制载荷的读取视图。
    ///
    /// ZIP/7Z 需要随机访问，TAR/单文件流只要顺序读取，LZH 读取器本身
    /// 接受任意 `Read + Seek + Send`；其余后端只接受路径，需要走物化。
    fn view_capable(format: ArchiveFormat) -> bool {
        matches!(
            format,
            ArchiveFormat::Zip
                | ArchiveFormat::SevenZip
                | ArchiveFormat::Tar
                | ArchiveFormat::TarGz
                | ArchiveFormat::TarBz2
                | ArchiveFormat::TarXz
                | ArchiveFormat::TarZst
                | ArchiveFormat::TarBr
                | ArchiveFormat::TarLz4
                | ArchiveFormat::TarLz
                | ArchiveFormat::TarLzma
                | ArchiveFormat::Gzip
                | ArchiveFormat::Bzip2
                | ArchiveFormat::Xz
                | ArchiveFormat::Zstd
                | ArchiveFormat::Brotli
                | ArchiveFormat::Lz4
                | ArchiveFormat::Lzh
        )
    }

    pub fn list(path: &Path, password: Option<&str>) -> Result<ArchiveInfo, ArchiveError> {
        let (input, _guard) = materialize_input(path)?;
        Self::list_direct(&input, password)
    }

    fn list_direct(path: &Path, password: Option<&str>) -> Result<ArchiveInfo, ArchiveError> {
        let format = Self::detect(path);
        match format {
            ArchiveFormat::Disguised => Self::list_disguised(path, password),
            ArchiveFormat::Zip => Self::list_zip(File::open(path)?),
            ArchiveFormat::SevenZip => {
                let header_encrypted = ArchiveReader::open(path, Password::empty()).is_err();
                Self::list_seven_zip(
                    File::open(path)?,
                    password.unwrap_or_default(),
                    header_encrypted,
                )
            }
            ArchiveFormat::Rar => Self::list_rar(path, password),
            ArchiveFormat::Zpaq => crate::zpaq::list(path, password),
            ArchiveFormat::Alz => crate::alz::list(path),
            format if is_legacy_archive(format) => Self::list_legacy(path, format),
            format if crate::unarc::is_supported(format) => crate::unarc::list(path, format),
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst
            | ArchiveFormat::TarBr
            | ArchiveFormat::TarLz4
            | ArchiveFormat::TarLz
            | ArchiveFormat::TarLzma => Self::list_tar(Box::new(File::open(path)?), format),
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
            ArchiveFormat::Disguised => Self::verify_password_disguised(path, password),
            ArchiveFormat::Zip => Self::verify_zip(File::open(path)?, password),
            ArchiveFormat::SevenZip => Self::verify_seven_zip(File::open(path)?, password),
            ArchiveFormat::Rar => match Self::verify_rar(path, password) {
                Ok(verified) => Ok(verified),
                Err(_) => Ok(false),
            },
            ArchiveFormat::Zpaq => crate::zpaq::verify_password(path, password),
            ArchiveFormat::Alz => crate::alz::verify_password(path, password),
            ArchiveFormat::Enc => Ok(decrypt_encrypted_container(path, password).is_ok()),
            _ => Ok(true),
        }
    }

    /// 校验 ZIP 密码：读取第一个加密条目的一个字节，按 CRC/MAC 判定。
    fn verify_zip<R: Read + Seek>(reader: R, password: &str) -> Result<bool, ArchiveError> {
        let mut archive = ZipArchive::new(reader)?;
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

    /// 校验 7Z 密码：完整读取条目流以触发 AES MAC/CRC 校验。
    fn verify_seven_zip<R: Read + Seek>(reader: R, password: &str) -> Result<bool, ArchiveError> {
        let mut reader = match ArchiveReader::new(reader, Password::from(password)) {
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
            ArchiveFormat::Disguised => Self::test_disguised(&input, password, progress),
            ArchiveFormat::Zpaq => crate::zpaq::test(&input, password, progress),
            ArchiveFormat::Zip => Self::test_zip(File::open(&input)?, password, &mut progress),
            ArchiveFormat::SevenZip => {
                Self::test_seven_zip(File::open(&input)?, password, &mut progress)
            }
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst
            | ArchiveFormat::TarBr
            | ArchiveFormat::TarLz4
            | ArchiveFormat::TarLz
            | ArchiveFormat::TarLzma => {
                Self::test_tar(Box::new(File::open(&input)?), format, &mut progress)
            }
            ArchiveFormat::Alz => crate::alz::test(&input, password, progress),
            ArchiveFormat::Rar => Self::test_rar(&input, &mut progress),
            format if crate::unarc::is_supported(format) => {
                crate::unarc::test(&input, format, &mut progress)
            }
            format if is_legacy_archive(format) => Self::test_legacy(&input, format, &mut progress),
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
            _ => Self::test_single_stream(Box::new(File::open(&input)?), format, &mut progress),
        }
    }

    /// 校验 ZIP 归档：逐条目读取到 `sink`，同时触发 CRC 校验。
    fn test_zip<R: Read + Seek, F>(
        reader: R,
        password: Option<&str>,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut archive = ZipArchive::new(reader)?;
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

    /// 校验 7Z 归档：逐条目读取到 `sink`，触发 AES MAC/CRC 校验。
    fn test_seven_zip<R: Read + Seek, F>(
        reader: R,
        password: Option<&str>,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut reader = ArchiveReader::new(reader, Password::from(password.unwrap_or_default()))
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
                let bytes = io::copy(input, &mut io::sink())
                    .map_err(|error| sevenz_rust2::Error::Other(error.to_string().into()))?;
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

    /// 校验 TAR 系列：逐条目读取到 `sink`。
    fn test_tar<F>(
        reader: Box<dyn Read>,
        format: ArchiveFormat,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut reader = tar_reader(reader, format)?;
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

    /// 校验单文件流：解压到 `sink`，同时消耗完整数据流。
    fn test_single_stream(
        reader: Box<dyn Read>,
        format: ArchiveFormat,
        _progress: &mut impl FnMut(ExtractProgress),
    ) -> Result<ExtractReport, ArchiveError> {
        let mut reader = decompress_reader(reader, format)?;
        let bytes = io::copy(&mut reader, &mut io::sink())?;
        Ok(ExtractReport {
            files: 1,
            bytes,
            skipped: 0,
        })
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
            ArchiveFormat::Disguised => Self::extract_disguised(path, options, progress),
            ArchiveFormat::Zip => Self::extract_zip(File::open(path)?, options, &mut progress),
            ArchiveFormat::SevenZip => {
                Self::extract_seven_zip(File::open(path)?, options, &mut progress)
            }
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZst
            | ArchiveFormat::TarBr
            | ArchiveFormat::TarLz4
            | ArchiveFormat::TarLz
            | ArchiveFormat::TarLzma => {
                Self::extract_tar(Box::new(File::open(path)?), format, options, &mut progress)
            }
            ArchiveFormat::Gzip
            | ArchiveFormat::Bzip2
            | ArchiveFormat::Xz
            | ArchiveFormat::Zstd
            | ArchiveFormat::Brotli
            | ArchiveFormat::Lz4 => {
                let name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("data")
                    .to_string();
                Self::extract_single_stream(
                    &name,
                    Box::new(File::open(path)?),
                    format,
                    options,
                    &mut progress,
                )
            }
            ArchiveFormat::Rar => Self::extract_rar(path, options, &mut progress),
            ArchiveFormat::Zpaq => crate::zpaq::extract(path, options, &mut progress),
            ArchiveFormat::Alz => crate::alz::extract(path, options, &mut progress),
            format if crate::unarc::is_supported(format) => {
                crate::unarc::extract(path, format, options, &mut progress)
            }
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
        if let Some(disguise) = &options.disguise {
            return Self::create_disguised(output, sources, options, disguise, progress);
        }
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
        if options.disguise.is_some() {
            return Err(ArchiveError::InvalidRequest(
                "伪装与自解压不能同时启用".into(),
            ));
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
            | ArchiveFormat::TarZst
            | ArchiveFormat::TarBr
            | ArchiveFormat::TarLz4
            | ArchiveFormat::TarLz
            | ArchiveFormat::TarLzma => Self::create_tar(output, sources, options, &mut progress),
            ArchiveFormat::Gzip
            | ArchiveFormat::Bzip2
            | ArchiveFormat::Xz
            | ArchiveFormat::Zstd
            | ArchiveFormat::Brotli
            | ArchiveFormat::Lz4 => {
                Self::create_single_stream(output, sources, options, &mut progress)
            }
            ArchiveFormat::Lzh => Self::create_lzh(output, sources, options, &mut progress),
            ArchiveFormat::Zpaq => crate::zpaq::create(output, sources, options, &mut progress),
            other => Err(ArchiveError::UnsupportedFormat(other.label().to_string())),
        }
    }
}

mod extract;

mod create;

mod progress;
pub(crate) use progress::*;

mod volume;
pub(crate) use volume::*;

mod util;
pub(crate) use util::*;
mod sfx;
use sfx::sfx_footer;
mod legacy;
use legacy::*;
mod disguise;
mod list;
#[cfg(test)]
mod tests;
