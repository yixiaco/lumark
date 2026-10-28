//! RAR 和传统只读格式的列表、解压与测试。

use super::*;

/// 判断 UnRAR 错误是否表示归档加密。
pub(crate) fn rar_error_means_encrypted(error: &unrar::error::UnrarError) -> bool {
    use unrar::error::{Code, When};

    matches!(error.code, Code::MissingPassword | Code::BadPassword)
        || (error.when == When::Open && error.code == Code::UnknownFormat)
}

/// 把 UnRAR 错误映射为统一归档错误。
pub(crate) fn map_rar_error(error: unrar::error::UnrarError) -> ArchiveError {
    match error.code {
        unrar::error::Code::MissingPassword => ArchiveError::PasswordRequired,
        unrar::error::Code::BadPassword => ArchiveError::WrongPassword,
        _ => ArchiveError::Corrupt(error.to_string()),
    }
}

/// 按格式打开传统只读归档读取器。
pub(crate) fn open_legacy_reader(
    path: &Path,
    format: ArchiveFormat,
) -> Result<Box<dyn LegacyReader>, ArchiveError> {
    use geezipx_core::archive::{
        ace::AceReader, arc::ArcReader, arj::ArjReader, asar::AsarReader, cab::CabReader,
        cpio::CpioReader, deb::DebReader, iso::IsoReader, lzh::LzhReader, udf::UdfReader,
        wim::WimReader,
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
        other => {
            return Err(ArchiveError::UnsupportedFormat(other.label().to_string()));
        }
    };
    Ok(reader)
}

impl ArchiveService {
    /// 探测 RAR 是否加密。
    pub(crate) fn probe_rar(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
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

    /// 解压传统只读归档。
    pub(crate) fn extract_legacy<F>(
        path: &Path,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let reader = open_legacy_reader(path, format)?;
        Self::extract_legacy_with(reader, options, progress)
    }

    /// 用已打开的读取器解压传统归档。
    pub(crate) fn extract_legacy_with<F>(
        mut reader: Box<dyn LegacyReader>,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
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
            // 传统只读适配器只能按条目声明大小做预算预检；真正的写入量仍由
            // 上游解码器决定，因此这里不做 `copy_cancellable_limited` 的兜底。
            ensure_budget(options, &report, entry.size)?;
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

    /// 验证 RAR 密码。
    pub(crate) fn verify_rar(path: &Path, password: &str) -> Result<bool, ArchiveError> {
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

    /// 解压 RAR 条目，处理覆盖、预算和取消。
    pub(crate) fn extract_rar<F>(
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
            // RAR 解压由 UnRAR 直接写文件，只能按头部声明大小预检预算；
            // 真正的写入量由后端保证。
            ensure_budget(options, &report, size)?;
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

    /// 用 UnRAR 的完整性测试模式校验 RAR，不写出文件。
    ///
    /// `test` 只需要知道成功/失败和条目统计，因此复用默认资源预算防止
    /// 异常归档把测试任务拖成不可中断的长任务。
    pub(crate) fn test_rar<F>(path: &Path, progress: &mut F) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut archive = UnrarArchive::new(path)
            .open_for_processing()
            .map_err(map_rar_error)?;
        let budget = ExtractionBudget::default();
        let mut report = ExtractReport::default();
        loop {
            let Some(header) = archive.read_header().map_err(map_rar_error)? else {
                break;
            };
            let name = header.entry().filename.to_string_lossy().replace('\\', "/");
            if header.entry().is_directory() {
                archive = header.skip().map_err(map_rar_error)?;
                continue;
            }
            let size = header.entry().unpacked_size;
            if report.files >= budget.max_entries {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "条目数超过上限 {}",
                    budget.max_entries
                )));
            }
            if report.bytes.saturating_add(size) > budget.max_bytes {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "解压总量将超过 {} 字节上限",
                    budget.max_bytes
                )));
            }
            archive = header.test().map_err(map_rar_error)?;
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

    /// 用传统只读适配器把全部条目解压到 sink，完成完整性测试。
    ///
    /// 这些适配器没有独立的 test API；写出到 `io::sink()` 可以触发解码和
    /// CRC 校验，同时不占用磁盘。预算只按条目声明大小累计。
    pub(crate) fn test_legacy<F>(
        path: &Path,
        format: ArchiveFormat,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let reader = open_legacy_reader(path, format)?;
        Self::test_legacy_with(reader, progress)
    }

    /// 用已打开的读取器完整测试传统归档。
    pub(crate) fn test_legacy_with<F>(
        mut reader: Box<dyn LegacyReader>,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let entries = reader
            .entries()
            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
        let total = entries.len();
        let total_bytes: u64 = entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.size)
            .sum();
        let budget = ExtractionBudget::default();
        let mut report = ExtractReport::default();
        for entry in entries {
            if entry.is_dir {
                continue;
            }
            if report.files >= budget.max_entries {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "条目数超过上限 {}",
                    budget.max_entries
                )));
            }
            if report.bytes.saturating_add(entry.size) > budget.max_bytes {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "解压总量将超过 {} 字节上限",
                    budget.max_bytes
                )));
            }
            reader
                .extract(&entry, &mut io::sink())
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
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
}
