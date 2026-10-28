use super::*;

impl ArchiveService {
    pub(crate) fn extract_encrypted_container<F>(
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

    pub(crate) fn extract_unix_compress<F>(
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

    pub(crate) fn extract_zip<R: Read + Seek, F>(
        reader: R,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut archive = ZipArchive::new(reader)?;
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

    pub(crate) fn extract_seven_zip<R: Read + Seek, F>(
        reader: R,
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
        let mut reader = ArchiveReader::new(reader, Password::from(password))
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

    pub(crate) fn extract_tar<F>(
        reader: Box<dyn Read>,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut reader = tar_reader(reader, format)?;
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

    pub(crate) fn extract_single_stream<F>(
        name: &str,
        reader: Box<dyn Read>,
        format: ArchiveFormat,
        options: &ExtractionOptions,
        progress: &mut F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let name = name.to_string();
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
        let mut input = decompress_reader(reader, format)?;
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

    pub(crate) fn extract_legacy_stream<F>(
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
}
