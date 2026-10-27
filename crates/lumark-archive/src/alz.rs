//! ALZ 只读后端。
//!
//! ALZ 的条目头、CP949 文件名、ALZ 改造 bzip2 和 ZipCrypto 都由 `unalz`
//! 解析/解码；本模块只负责把它接到 Lumark 的格式信息、路径安全、覆盖策略、
//! 资源预算和进度模型上。ALZ 创建不在支持范围。

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use unalz::archive::{AlzArchive, AlzFileEntry, CompressionMethod};
use unalz::crypto::ZipCrypto;
use unalz::error::AlzError;

use crate::error::ArchiveError;
use crate::format::{
    ArchiveEntry, ArchiveFormat, ArchiveInfo, EncryptionInfo, ExtractReport, ExtractionBudget,
    ExtractionOptions,
};
use crate::path::{ensure_no_symlink_components, safe_join};
use crate::service::{ensure_budget, ensure_not_cancelled, selected, unique_destination};

/// 打开 ALZ 归档，并把 `unalz` 的错误转换成统一的归档错误。
///
/// `unalz` 的路径接口是 `&str`，因此非 UTF-8 路径会在这里直接失败；这是
/// 当前后端接入选型带来的明确限制。
fn open(path: &Path) -> Result<AlzArchive, ArchiveError> {
    let path_str = path.to_str().ok_or_else(|| {
        ArchiveError::InvalidRequest("ALZ 归档路径必须是有效的 UTF-8 字符串".into())
    })?;
    AlzArchive::open(path_str).map_err(map_alz_error)
}

/// 把 `unalz` 错误映射为界面可识别的归档错误。
fn map_alz_error(error: AlzError) -> ArchiveError {
    match error {
        AlzError::PasswordNotSet => ArchiveError::PasswordRequired,
        AlzError::InvalidPassword => ArchiveError::WrongPassword,
        AlzError::PathTraversal(name) => ArchiveError::UnsafePath(name),
        AlzError::UncompressedSizeExceeded { limit } => {
            ArchiveError::BudgetExceeded(format!("ALZ 解压结果超过 {limit} 字节上限"))
        }
        AlzError::CantOpenFile(error) | AlzError::CantOpenDestFile(error) => {
            ArchiveError::Io(error)
        }
        AlzError::Io(error) => ArchiveError::Io(error),
        other => ArchiveError::Corrupt(other.to_string()),
    }
}

/// 统一条目名分隔符，供归档内路径展示和解压落盘使用。
fn normalized_name(entry: &AlzFileEntry) -> String {
    entry.file_name.replace('\\', "/")
}

/// 把条目里的 DOS 时间转换为 UTC，供文件列表显示。
fn modified_time(entry: &AlzFileEntry) -> Option<DateTime<Utc>> {
    unalz::dostime::dos_datetime_to_systime(entry.file_time_date).map(DateTime::<Utc>::from)
}

/// 解压成功后尽量恢复条目记录的修改时间；失败不影响内容正确性。
fn apply_modified_time(path: &Path, entry: &AlzFileEntry) {
    if let Some(time) = unalz::dostime::dos_datetime_to_systime(entry.file_time_date) {
        let _ = filetime::set_file_mtime(path, filetime::FileTime::from_system_time(time));
    }
}

/// 只读列出 ALZ 条目；加密归档的目录头本身不加密，因此无需密码。
pub(crate) fn list(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
    let archive = open(path)?;
    let mut info = ArchiveInfo {
        format: Some(ArchiveFormat::Alz),
        encrypted: archive.is_encrypted,
        ..Default::default()
    };
    for (index, entry) in archive.entries.iter().enumerate() {
        let name = normalized_name(entry);
        info.entries.push(ArchiveEntry {
            id: index as u64,
            path: name,
            is_dir: entry.is_directory(),
            size: entry.uncompressed_size,
            compressed_size: (entry.compressed_size > 0).then_some(entry.compressed_size),
            modified: modified_time(entry),
            crc32: Some(entry.file_crc),
            method: format!("{:?}", entry.compression_method),
            encrypted: entry.is_encrypted(),
        });
        info.total_size += entry.uncompressed_size;
        info.compressed_size += entry.compressed_size;
    }
    Ok(info)
}

/// 探测 ALZ 是否包含加密条目；ALZ 没有整体加密的目录头。
pub(crate) fn probe_encryption(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
    let archive = open(path)?;
    Ok(EncryptionInfo {
        encrypted: archive.is_encrypted,
        header_encrypted: false,
        format: Some(ArchiveFormat::Alz),
    })
}

/// 用第一个加密条目的 ZipCrypto 校验头验证密码。
///
/// ALZ 的加密头只提供 1 字节校验，和 ZIP 传统加密一致；这里不尝试解压
/// 全部条目，避免密码校验退化成完整解压。
pub(crate) fn verify_password(path: &Path, password: &str) -> Result<bool, ArchiveError> {
    let archive = open(path)?;
    let Some(entry) = archive.entries.iter().find(|entry| entry.is_encrypted()) else {
        return Ok(true);
    };
    let Some(header) = entry.enc_check else {
        return Ok(false);
    };
    let password = unalz::encoding::password_to_cp949(password);
    let mut crypto = ZipCrypto::new(&password);
    Ok(crypto.check_header(
        &header,
        entry.file_crc,
        entry.file_time_date,
        entry.has_data_descriptor(),
    ))
}

/// 把条目数据解压到指定 writer，返回 `unalz` 计算出的 CRC32。
///
/// `limit` 同时传给后端解码器和写入器：后端负责按声明大小截断，写入器
/// 负责在声明大小不可信时兜底，二者超限都会让解压失败。
fn decompress_entry<W: Write>(
    archive: &mut AlzArchive,
    entry: &AlzFileEntry,
    output: &mut W,
    limit: u64,
    password: Option<&str>,
) -> Result<u32, ArchiveError> {
    let mut crypto = if entry.is_encrypted() {
        let Some(password) = password.filter(|value| !value.is_empty()) else {
            return Err(ArchiveError::PasswordRequired);
        };
        let Some(header) = entry.enc_check else {
            return Err(ArchiveError::Corrupt("ALZ 加密条目缺少校验头".into()));
        };
        let password = unalz::encoding::password_to_cp949(password);
        let mut crypto = ZipCrypto::new(&password);
        if !crypto.check_header(
            &header,
            entry.file_crc,
            entry.file_time_date,
            entry.has_data_descriptor(),
        ) {
            return Err(ArchiveError::WrongPassword);
        }
        // 校验头会消耗密钥状态；重新初始化并消费 12 字节头，才能对齐数据区。
        let mut crypto = ZipCrypto::new(&password);
        let mut header_copy = header;
        crypto.decrypt(&mut header_copy);
        Some(crypto)
    } else {
        None
    };

    archive
        .reader
        .seek(SeekFrom::Start(entry.data_pos))
        .map_err(ArchiveError::Io)?;
    let mut limited = (&mut archive.reader).take(entry.compressed_size);
    let result = match entry.compression_method {
        CompressionMethod::Store => unalz::decompress::raw::extract_raw(
            &mut limited,
            output,
            entry.compressed_size,
            limit,
            crypto.as_mut(),
        ),
        CompressionMethod::Deflate => unalz::decompress::deflate::extract_deflate(
            &mut limited,
            output,
            entry.compressed_size,
            limit,
            crypto.as_mut(),
        ),
        CompressionMethod::Bzip2 => unalz::decompress::bzip2::extract_bzip2(
            &mut limited,
            output,
            entry.compressed_size,
            limit,
            crypto.as_mut(),
        ),
        CompressionMethod::Unknown(method) => {
            return Err(ArchiveError::Corrupt(format!(
                "ALZ 条目使用不支持的压缩方法：{method}"
            )));
        }
    };
    result.map_err(map_alz_error)
}

/// 解压 ALZ 到目标目录，复用统一的路径、覆盖策略和资源预算。
pub(crate) fn extract<F>(
    path: &Path,
    options: &ExtractionOptions,
    mut progress: F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(crate::service::ExtractProgress),
{
    let mut archive = open(path)?;
    let entries = archive.entries.clone();
    let total = entries.len();
    let total_bytes: u64 = entries.iter().map(|entry| entry.uncompressed_size).sum();
    let password = options
        .password
        .as_ref()
        .map(|value| value.as_str())
        .filter(|value| !value.is_empty());
    if archive.is_encrypted && password.is_none() {
        return Err(ArchiveError::PasswordRequired);
    }
    fs::create_dir_all(&options.output_dir)?;
    let mut report = ExtractReport::default();
    for entry in entries {
        ensure_not_cancelled(&options.cancel)?;
        let name = normalized_name(&entry);
        if !selected(&name, &options.selected_paths) {
            continue;
        }
        let mut destination = safe_join(&options.output_dir, &name)?;
        ensure_no_symlink_components(&options.output_dir, &destination)?;
        if entry.is_directory() {
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
        // 既按声明大小做累计预算检查，也把剩余预算交给解码器和写入器兜底；
        // ALZ 的 data descriptor 条目可能声明 0 字节，不能只依赖声明值。
        ensure_budget(options, &report, entry.uncompressed_size)?;
        let remaining = options.budget.max_bytes.saturating_sub(report.bytes);
        let mut output = CountingWriter::new(File::create(&destination)?, remaining);
        let result = decompress_entry(&mut archive, &entry, &mut output, remaining, password);
        let exceeded = output.exceeded;
        let written = output.written;
        let mut file = output.into_inner();
        match result {
            Ok(crc) if crc == entry.file_crc => {
                if let Err(error) = file.flush() {
                    drop(file);
                    let _ = fs::remove_file(&destination);
                    return Err(ArchiveError::Io(error));
                }
                drop(file);
                apply_modified_time(&destination, &entry);
            }
            Ok(crc) => {
                drop(file);
                let _ = fs::remove_file(&destination);
                return Err(ArchiveError::Corrupt(format!(
                    "ALZ 条目 CRC 不匹配：{name}（期望 {:08x}，实际 {crc:08x}）",
                    entry.file_crc
                )));
            }
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&destination);
                return Err(if exceeded {
                    ArchiveError::BudgetExceeded(format!("ALZ 解压超出剩余预算 {remaining} 字节"))
                } else {
                    error
                });
            }
        }
        report.files += 1;
        report.bytes += written;
        progress(crate::service::ExtractProgress {
            current: name,
            bytes_done: report.bytes,
            bytes_total: total_bytes,
            entries_done: report.files,
            entries_total: total,
        });
    }
    Ok(report)
}

/// 完整测试 ALZ：解压到 sink、校验 CRC，并使用默认资源预算。
pub(crate) fn test<F>(
    path: &Path,
    password: Option<&str>,
    mut progress: F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(crate::service::ExtractProgress),
{
    let mut archive = open(path)?;
    let entries = archive.entries.clone();
    let total = entries.len();
    let total_bytes: u64 = entries.iter().map(|entry| entry.uncompressed_size).sum();
    let password = password.filter(|value| !value.is_empty());
    if archive.is_encrypted && password.is_none() {
        return Err(ArchiveError::PasswordRequired);
    }
    let budget = ExtractionBudget::default();
    let mut report = ExtractReport::default();
    for entry in entries {
        if entry.is_directory() {
            continue;
        }
        if report.files >= budget.max_entries {
            return Err(ArchiveError::BudgetExceeded(format!(
                "条目数超过上限 {}",
                budget.max_entries
            )));
        }
        if report.bytes.saturating_add(entry.uncompressed_size) > budget.max_bytes {
            return Err(ArchiveError::BudgetExceeded(format!(
                "解压总量将超过 {} 字节上限",
                budget.max_bytes
            )));
        }
        let name = normalized_name(&entry);
        let remaining = budget.max_bytes.saturating_sub(report.bytes);
        let mut output = CountingWriter::new(io::sink(), remaining);
        let result = decompress_entry(&mut archive, &entry, &mut output, remaining, password);
        let exceeded = output.exceeded;
        let written = output.written;
        match result {
            Ok(crc) if crc == entry.file_crc => {}
            Ok(crc) => {
                return Err(ArchiveError::Corrupt(format!(
                    "ALZ 条目 CRC 不匹配：{name}（期望 {:08x}，实际 {crc:08x}）",
                    entry.file_crc
                )));
            }
            Err(_) if exceeded => {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "ALZ 解压超出剩余预算 {remaining} 字节"
                )));
            }
            Err(error) => return Err(error),
        }
        report.files += 1;
        report.bytes += written;
        progress(crate::service::ExtractProgress {
            current: name,
            bytes_done: report.bytes,
            bytes_total: total_bytes,
            entries_done: report.files,
            entries_total: total,
        });
    }
    Ok(report)
}

/// 限制写出字节数并记录实际写入量的 writer。
///
/// `unalz` 的 `max_output` 已经按声明大小截断；这里的限制用于应对声明大小
/// 为零或被伪造的条目，保证不突破调用方给出的剩余预算。
struct CountingWriter<W> {
    inner: W,
    written: u64,
    limit: u64,
    exceeded: bool,
}

impl<W: Write> CountingWriter<W> {
    /// 创建带字节上限的 writer。
    fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            written: 0,
            limit,
            exceeded: false,
        }
    }

    /// 取回底层 writer，便于 flush 或 drop。
    fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.written.saturating_add(buffer.len() as u64) > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("ALZ 解压超出资源预算"));
        }
        let written = self.inner.write(buffer)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::CountingWriter;
    use std::io::Write;

    /// 验证 writer 在超过上限时拒绝写入并标记超限。
    #[test]
    fn counting_writer_rejects_over_budget() {
        let mut writer = CountingWriter::new(Vec::new(), 3);
        writer.write_all(b"abc").unwrap();
        assert!(writer.write_all(b"d").is_err());
        assert!(writer.exceeded);
        assert_eq!(writer.written, 3);
    }
}
