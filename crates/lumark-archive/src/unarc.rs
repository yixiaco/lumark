//! `unarc-rs` 传统格式只读后端。
//!
//! 这里覆盖 ZOO、HA、HYP、SQ、SQZ、UC2、ICE、PACKICE 和 TAR.Z。它们通常
//! 没有可靠魔数，只能按扩展名识别；解码、CRC 校验和格式细节交给
//! `unarc-rs`，本模块负责路径安全、覆盖策略、预算和进度。
//!
//! 局限：`unarc-rs` 的读取接口按整条目返回 `Vec<u8>`，因此无法在条目
//! 内部逐块上报进度或取消；预算只能在读取前按声明大小、读取后按实际长度
//! 检查。

use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::Path;

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use unarc_rs::unified::{
    ArchiveEntry as UnarcEntry, ArchiveFormat as UnarcFormat, ArchiveOptions, UnifiedArchive,
};

use crate::error::ArchiveError;
use crate::format::{
    ArchiveEntry, ArchiveFormat, ArchiveInfo, ExtractReport, ExtractionBudget, ExtractionOptions,
};
use crate::path::{ensure_no_symlink_components, safe_join};
use crate::service::{ensure_budget, ensure_not_cancelled, selected, unique_destination};

/// `unarc-rs` 的归档句柄类型。
type Archive = UnifiedArchive<BufReader<File>>;

/// 判断格式是否由本后端处理。
pub(crate) fn is_supported(format: ArchiveFormat) -> bool {
    unarc_format(format).is_some()
}

/// 把 Lumark 格式映射为 `unarc-rs` 的格式枚举。
fn unarc_format(format: ArchiveFormat) -> Option<UnarcFormat> {
    match format {
        ArchiveFormat::TarZ => Some(UnarcFormat::TarZ),
        ArchiveFormat::Zoo => Some(UnarcFormat::Zoo),
        ArchiveFormat::Ha => Some(UnarcFormat::Ha),
        ArchiveFormat::Hyp => Some(UnarcFormat::Hyp),
        ArchiveFormat::Sq => Some(UnarcFormat::Sq),
        ArchiveFormat::Sqz => Some(UnarcFormat::Sqz),
        ArchiveFormat::Uc2 => Some(UnarcFormat::Uc2),
        ArchiveFormat::Ice => Some(UnarcFormat::Ice),
        ArchiveFormat::PackIce => Some(UnarcFormat::PackIce),
        _ => None,
    }
}

/// 读取时统一开启 CRC 校验。
fn archive_options() -> ArchiveOptions {
    ArchiveOptions::new().with_verify_crc(true)
}

/// 打开传统格式归档。
fn open(path: &Path, format: ArchiveFormat) -> Result<Archive, ArchiveError> {
    let format = unarc_format(format)
        .ok_or_else(|| ArchiveError::UnsupportedFormat(format.label().to_string()))?;
    let file = File::open(path)?;
    UnifiedArchive::open_with_format_and_options(BufReader::new(file), format, archive_options())
        .map_err(map_unarc_error)
}

/// 把 `unarc-rs` 错误映射为统一的归档错误。
fn map_unarc_error(error: unarc_rs::error::ArchiveError) -> ArchiveError {
    use unarc_rs::error::ArchiveError as UnarcError;

    match error {
        UnarcError::Io(error) => ArchiveError::Io(error),
        UnarcError::PasswordRequired { .. } | UnarcError::EncryptionRequired { .. } => {
            ArchiveError::PasswordRequired
        }
        UnarcError::InvalidPassword { .. } => ArchiveError::WrongPassword,
        UnarcError::UnsupportedFormat(name) => ArchiveError::UnsupportedFormat(name),
        UnarcError::UnsupportedMethod { format, method } => {
            ArchiveError::UnsupportedFormat(format!("{format}：{method}"))
        }
        other => ArchiveError::Corrupt(other.to_string()),
    }
}

/// 统一条目名分隔符。
fn normalized_name(name: &str) -> String {
    name.replace('\\', "/")
}

/// 把 DOS 时间按本地时区解释后转换为 UTC，和 ZIP 条目时间语义一致。
fn dos_time_to_utc(time: unarc_rs::date_time::DosDateTime) -> Option<DateTime<Utc>> {
    let date = NaiveDate::from_ymd_opt(
        i32::from(time.year()),
        u32::from(time.month()),
        u32::from(time.day()),
    )?;
    let naive = date.and_hms_opt(
        u32::from(time.hour()),
        u32::from(time.minute()),
        u32::from(time.second()),
    )?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|value| value.with_timezone(&Utc))
}

/// 读取下一条目录项，并容忍传统格式末尾的 56/57 字节终止记录。
///
/// 部分老式归档（ZOO 等）在最后一条记录后还会写一个不完整的目录项头；
/// `unarc-rs` 的 `entries()` 会把这段尾部当成新条目并报 EOF。这里只在已经
/// 成功读取过条目后把 `UnexpectedEof` 当作正常结束，避免列表和解压被
/// 一个合法的尾部标记打断，同时保留“空/彻底损坏归档仍然报错”的语义。
fn next_entry(archive: &mut Archive, processed: usize) -> Result<Option<UnarcEntry>, ArchiveError> {
    match archive.next_entry() {
        Ok(entry) => Ok(entry),
        Err(unarc_rs::error::ArchiveError::Io(error))
            if processed > 0 && error.kind() == std::io::ErrorKind::UnexpectedEof =>
        {
            Ok(None)
        }
        Err(error) => Err(map_unarc_error(error)),
    }
}

/// 收集全部目录项，供列表和进度总量使用。
fn collect_entries(archive: &mut Archive) -> Result<Vec<UnarcEntry>, ArchiveError> {
    let mut entries = Vec::new();
    while let Some(entry) = next_entry(archive, entries.len())? {
        archive.skip(&entry).map_err(map_unarc_error)?;
        entries.push(entry);
    }
    Ok(entries)
}

/// 只读列出传统格式条目。
pub(crate) fn list(path: &Path, format: ArchiveFormat) -> Result<ArchiveInfo, ArchiveError> {
    let mut archive = open(path, format)?;
    let entries = collect_entries(&mut archive)?;
    let mut info = ArchiveInfo {
        format: Some(format),
        ..Default::default()
    };
    for (index, entry) in entries.into_iter().enumerate() {
        let name = normalized_name(entry.name());
        let is_dir = entry.is_directory();
        let size = entry.original_size();
        let compressed = entry.compressed_size();
        let crc = entry.crc();
        info.entries.push(ArchiveEntry {
            id: index as u64,
            path: name,
            is_dir,
            size,
            compressed_size: (compressed > 0).then_some(compressed),
            modified: entry.modified_time().and_then(dos_time_to_utc),
            crc32: (crc != 0).then_some(crc as u32),
            method: entry.compression_method().to_string(),
            encrypted: false,
        });
        info.total_size += size;
        info.compressed_size += compressed;
    }
    Ok(info)
}

/// 解压传统格式到目标目录。
///
/// 第一遍只读取条目元数据，用于给出总进度；随后重新打开归档按顺序读取。
/// 选中条目读取后不再 `skip`，避免顺序格式（如 HA）被重复跳过；未选中
/// 的文件和目录必须显式 `skip` 才能前进到下一个头。
pub(crate) fn extract<F>(
    path: &Path,
    format: ArchiveFormat,
    options: &ExtractionOptions,
    mut progress: F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(crate::service::ExtractProgress),
{
    let mut listing = open(path, format)?;
    let listed = collect_entries(&mut listing)?;
    let total = listed.len();
    let total_bytes: u64 = listed.iter().map(|entry| entry.original_size()).sum();
    drop(listing);

    let mut archive = open(path, format)?;
    fs::create_dir_all(&options.output_dir)?;
    let mut report = ExtractReport::default();
    let mut processed = 0;
    while let Some(entry) = next_entry(&mut archive, processed)? {
        processed += 1;
        ensure_not_cancelled(&options.cancel)?;
        let name = normalized_name(entry.name());
        let size = entry.original_size();
        if !selected(&name, &options.selected_paths) {
            archive.skip(&entry).map_err(map_unarc_error)?;
            continue;
        }
        let mut destination = safe_join(&options.output_dir, &name)?;
        ensure_no_symlink_components(&options.output_dir, &destination)?;
        if entry.is_directory() {
            fs::create_dir_all(&destination)?;
            archive.skip(&entry).map_err(map_unarc_error)?;
            continue;
        }
        if destination.exists() {
            match options.overwrite_policy {
                crate::format::OverwritePolicy::Rename => {
                    destination = unique_destination(&destination);
                }
                crate::format::OverwritePolicy::Skip => {
                    report.skipped += 1;
                    archive.skip(&entry).map_err(map_unarc_error)?;
                    continue;
                }
                crate::format::OverwritePolicy::Ask if !options.overwrite => {
                    report.skipped += 1;
                    archive.skip(&entry).map_err(map_unarc_error)?;
                    continue;
                }
                _ => {}
            }
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        ensure_budget(options, &report, size)?;
        let remaining = options.budget.max_bytes.saturating_sub(report.bytes);
        let data = archive
            .read_with_options(&entry, &archive_options())
            .map_err(map_unarc_error)?;
        // 声明大小可能为 0 或与实际不符；实际长度再多做一次预算校验。
        if data.len() as u64 > remaining {
            let _ = fs::remove_file(&destination);
            return Err(ArchiveError::BudgetExceeded(format!(
                "解压数据超过剩余预算 {remaining} 字节"
            )));
        }
        let bytes = data.len() as u64;
        let mut output = File::create(&destination)?;
        if let Err(error) = output.write_all(&data) {
            drop(output);
            let _ = fs::remove_file(&destination);
            return Err(ArchiveError::Io(error));
        }
        report.files += 1;
        report.bytes += bytes;
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

/// 完整测试传统格式：读取所有条目、校验 CRC，并使用默认资源预算。
pub(crate) fn test<F>(
    path: &Path,
    format: ArchiveFormat,
    mut progress: F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(crate::service::ExtractProgress),
{
    let mut listing = open(path, format)?;
    let listed = collect_entries(&mut listing)?;
    let total = listed.len();
    let total_bytes: u64 = listed.iter().map(|entry| entry.original_size()).sum();
    drop(listing);

    let mut archive = open(path, format)?;
    let budget = ExtractionBudget::default();
    let mut report = ExtractReport::default();
    let mut processed = 0;
    while let Some(entry) = next_entry(&mut archive, processed)? {
        processed += 1;
        if entry.is_directory() {
            archive.skip(&entry).map_err(map_unarc_error)?;
            continue;
        }
        {
            if report.files >= budget.max_entries {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "条目数超过上限 {}",
                    budget.max_entries
                )));
            }
            if report.bytes.saturating_add(entry.original_size()) > budget.max_bytes {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "解压总量将超过 {} 字节上限",
                    budget.max_bytes
                )));
            }
        }
        let name = normalized_name(entry.name());
        let data = archive
            .read_with_options(&entry, &archive_options())
            .map_err(map_unarc_error)?;
        let bytes = data.len() as u64;
        report.files += 1;
        report.bytes += bytes;
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
