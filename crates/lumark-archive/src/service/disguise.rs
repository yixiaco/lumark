//! 伪装归档的列表、解压、测试、密码校验和创建。

use super::*;

impl ArchiveService {
    /// 解析伪装包内层格式：优先用尾部保存的原文件名，必要时读头部魔数。
    pub(crate) fn disguised_inner_format(
        path: &Path,
        info: &DisguiseInfo,
    ) -> Result<ArchiveFormat, ArchiveError> {
        let by_name = detect_format(Path::new(&info.original_name));
        if by_name != ArchiveFormat::Unknown {
            return Ok(by_name);
        }
        let mut reader = crate::disguise::open_payload(path)?;
        let mut header = [0u8; 512];
        let read = reader.read(&mut header)?;
        let by_magic = detect_magic(&header[..read]);
        if by_magic == ArchiveFormat::Unknown {
            return Err(ArchiveError::Corrupt(format!(
                "无法识别伪装包内层格式：{}",
                info.original_name
            )));
        }
        Ok(by_magic)
    }

    /// 准备伪装包的读取输入：读取视图或物化副本。
    pub(crate) fn disguised_input(
        path: &Path,
    ) -> Result<(ArchiveFormat, DisguiseInfo, DisguisedInput), ArchiveError> {
        let info = crate::disguise::probe(path)?.ok_or_else(|| {
            ArchiveError::Corrupt(format!("不是 Lumark 伪装文件：{}", path.display()))
        })?;
        if info.payload_kind != DisguisePayloadKind::Archive {
            return Err(ArchiveError::InvalidRequest(
                "该伪装文件保存的是原始文件，请先还原后再打开".into(),
            ));
        }
        let format = Self::disguised_inner_format(path, &info)?;
        if Self::view_capable(format) {
            let reader = crate::disguise::open_payload(path)?;
            Ok((format, info, DisguisedInput::View(reader)))
        } else {
            let materialized = crate::disguise::materialize(path)?;
            Ok((format, info, DisguisedInput::Materialized(materialized)))
        }
    }

    /// 为 LZH 伪装包构造直接读取载荷的传统读取器。
    pub(crate) fn open_legacy_reader_view(
        path: &Path,
    ) -> Result<Box<dyn LegacyReader>, ArchiveError> {
        let view = crate::disguise::open_payload(path)?;
        Ok(Box::new(geezipx_core::archive::lzh::LzhReader::new(view)))
    }

    /// 探测伪装包内层归档的加密状态。
    pub(crate) fn probe_encryption_disguised(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
        let (format, _info, input) = Self::disguised_input(path)?;
        match input {
            DisguisedInput::Materialized(temp) => {
                let mut info = Self::probe_encryption_direct(&temp)?;
                info.format = Some(ArchiveFormat::Disguised);
                Ok(info)
            }
            DisguisedInput::View(reader) => match format {
                ArchiveFormat::Zip => Ok(EncryptionInfo {
                    encrypted: Self::probe_zip(reader)?,
                    header_encrypted: false,
                    format: Some(ArchiveFormat::Disguised),
                }),
                ArchiveFormat::SevenZip => {
                    let (header_encrypted, encrypted) = Self::probe_seven_zip(reader)?;
                    Ok(EncryptionInfo {
                        encrypted,
                        header_encrypted,
                        format: Some(ArchiveFormat::Disguised),
                    })
                }
                _ => Ok(EncryptionInfo {
                    encrypted: false,
                    header_encrypted: false,
                    format: Some(ArchiveFormat::Disguised),
                }),
            },
        }
    }

    /// 列出伪装包内层归档。
    pub(crate) fn list_disguised(
        path: &Path,
        password: Option<&str>,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let (format, info, input) = Self::disguised_input(path)?;
        match input {
            DisguisedInput::Materialized(temp) => Self::list_direct(&temp, password),
            DisguisedInput::View(reader) => {
                let mut archive = match format {
                    ArchiveFormat::Zip => Self::list_zip(reader)?,
                    ArchiveFormat::SevenZip => {
                        let header_encrypted = ArchiveReader::new(
                            crate::disguise::open_payload(path)?,
                            Password::empty(),
                        )
                        .is_err();
                        Self::list_seven_zip(
                            reader,
                            password.unwrap_or_default(),
                            header_encrypted,
                        )?
                    }
                    ArchiveFormat::Tar
                    | ArchiveFormat::TarGz
                    | ArchiveFormat::TarBz2
                    | ArchiveFormat::TarXz
                    | ArchiveFormat::TarZst
                    | ArchiveFormat::TarBr
                    | ArchiveFormat::TarLz4
                    | ArchiveFormat::TarLz
                    | ArchiveFormat::TarLzma => Self::list_tar(Box::new(reader), format)?,
                    ArchiveFormat::Gzip
                    | ArchiveFormat::Bzip2
                    | ArchiveFormat::Xz
                    | ArchiveFormat::Zstd
                    | ArchiveFormat::Brotli
                    | ArchiveFormat::Lz4 => Self::list_single_stream_meta(
                        &disguise_stem(&info.original_name),
                        info.payload_len,
                        format,
                    ),
                    ArchiveFormat::Lzh => {
                        Self::list_legacy_with(Self::open_legacy_reader_view(path)?, format)?
                    }
                    other => {
                        return Err(ArchiveError::UnsupportedFormat(other.label().to_string()));
                    }
                };
                archive.format = Some(ArchiveFormat::Disguised);
                Ok(archive)
            }
        }
    }

    /// 解压伪装包内层归档。
    pub(crate) fn extract_disguised<F>(
        path: &Path,
        options: &ExtractionOptions,
        mut progress: F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let (format, info, input) = Self::disguised_input(path)?;
        match input {
            DisguisedInput::Materialized(temp) => Self::extract_direct(&temp, options, progress),
            DisguisedInput::View(reader) => match format {
                ArchiveFormat::Zip => Self::extract_zip(reader, options, &mut progress),
                ArchiveFormat::SevenZip => Self::extract_seven_zip(reader, options, &mut progress),
                ArchiveFormat::Tar
                | ArchiveFormat::TarGz
                | ArchiveFormat::TarBz2
                | ArchiveFormat::TarXz
                | ArchiveFormat::TarZst
                | ArchiveFormat::TarBr
                | ArchiveFormat::TarLz4
                | ArchiveFormat::TarLz
                | ArchiveFormat::TarLzma => {
                    Self::extract_tar(Box::new(reader), format, options, &mut progress)
                }
                ArchiveFormat::Gzip
                | ArchiveFormat::Bzip2
                | ArchiveFormat::Xz
                | ArchiveFormat::Zstd
                | ArchiveFormat::Brotli
                | ArchiveFormat::Lz4 => Self::extract_single_stream(
                    &disguise_stem(&info.original_name),
                    Box::new(reader),
                    format,
                    options,
                    &mut progress,
                ),
                ArchiveFormat::Lzh => Self::extract_legacy_with(
                    Self::open_legacy_reader_view(path)?,
                    options,
                    &mut progress,
                ),
                other => Err(ArchiveError::UnsupportedFormat(other.label().to_string())),
            },
        }
    }

    /// 测试伪装包内层归档。
    pub(crate) fn test_disguised<F>(
        path: &Path,
        password: Option<&str>,
        mut progress: F,
    ) -> Result<ExtractReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let (format, _info, input) = Self::disguised_input(path)?;
        match input {
            DisguisedInput::Materialized(temp) => Self::test(&temp, password, progress),
            DisguisedInput::View(reader) => match format {
                ArchiveFormat::Zip => Self::test_zip(reader, password, &mut progress),
                ArchiveFormat::SevenZip => Self::test_seven_zip(reader, password, &mut progress),
                ArchiveFormat::Tar
                | ArchiveFormat::TarGz
                | ArchiveFormat::TarBz2
                | ArchiveFormat::TarXz
                | ArchiveFormat::TarZst
                | ArchiveFormat::TarBr
                | ArchiveFormat::TarLz4
                | ArchiveFormat::TarLz
                | ArchiveFormat::TarLzma => Self::test_tar(Box::new(reader), format, &mut progress),
                ArchiveFormat::Gzip
                | ArchiveFormat::Bzip2
                | ArchiveFormat::Xz
                | ArchiveFormat::Zstd
                | ArchiveFormat::Brotli
                | ArchiveFormat::Lz4 => {
                    Self::test_single_stream(Box::new(reader), format, &mut progress)
                }
                ArchiveFormat::Lzh => {
                    Self::test_legacy_with(Self::open_legacy_reader_view(path)?, &mut progress)
                }
                other => Err(ArchiveError::UnsupportedFormat(other.label().to_string())),
            },
        }
    }

    /// 校验伪装包内层归档的密码。
    pub(crate) fn verify_password_disguised(
        path: &Path,
        password: &str,
    ) -> Result<bool, ArchiveError> {
        let (format, _info, input) = Self::disguised_input(path)?;
        match input {
            DisguisedInput::Materialized(temp) => Self::verify_password_direct(&temp, password),
            DisguisedInput::View(reader) => match format {
                ArchiveFormat::Zip => Self::verify_zip(reader, password),
                ArchiveFormat::SevenZip => Self::verify_seven_zip(reader, password),
                _ => Ok(true),
            },
        }
    }

    /// 生成伪装归档：先写内层归档，再流式包装成媒体外观。
    ///
    /// 内层归档写入同目录临时文件，复用现有全部格式的创建与进度逻辑；
    /// 包装阶段按 1 MiB 分块混淆写入，最后原子替换目标路径。失败或取消
    /// 只清理临时文件，旧输出保持不变。
    pub(crate) fn create_disguised<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        disguise: &DisguiseOptions,
        mut progress: F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        if !options.format.is_writable() {
            return Err(ArchiveError::UnsupportedFormat(format!(
                "{} 不支持创建，无法伪装",
                options.format.label()
            )));
        }
        if options.sfx {
            return Err(ArchiveError::InvalidRequest(
                "伪装与自解压不能同时启用".into(),
            ));
        }
        if options.split_size.is_some_and(|size| size >= 64 * 1024) {
            return Err(ArchiveError::InvalidRequest("伪装不支持分卷".into()));
        }
        if options.delete_source {
            ensure_sources_safe_to_delete(sources, output)?;
        }
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        let inner = parent.join(format!(
            ".lumark-disguise-{}-{}.{}",
            std::process::id(),
            stamp,
            options.format.extension()
        ));
        let staged = parent.join(format!(
            ".lumark-disguise-{}-{}.part",
            std::process::id(),
            stamp
        ));
        let mut inner_options = options.clone();
        inner_options.disguise = None;
        inner_options.test_after = false;
        inner_options.delete_source = false;
        inner_options.split_size = None;
        let mut report = match Self::create_direct(&inner, sources, &inner_options, &mut progress) {
            Ok(report) => report,
            Err(error) => {
                let _ = fs::remove_file(&inner);
                return Err(error);
            }
        };
        let entries_total = report.files;
        let source_total = report.bytes;
        let result = (|| -> Result<(), ArchiveError> {
            if options.test_after {
                Self::test(
                    &inner,
                    options.password.as_deref().map(String::as_str),
                    |_| {},
                )?;
            }
            let original_name = disguise.original_name.clone().unwrap_or_else(|| {
                let stem = output
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("archive");
                format!("{stem}.{}", options.format.extension())
            });
            crate::disguise::wrap(
                &inner,
                &staged,
                &disguise.mask,
                &original_name,
                DisguisePayloadKind::Archive,
                options.cancel.as_ref(),
                |copied, total| {
                    progress(CreateProgress {
                        current: "写入伪装容器".to_string(),
                        bytes_done: source_total,
                        bytes_total: source_total,
                        current_bytes_done: copied,
                        current_bytes_total: total,
                        output_bytes: copied,
                        entries_done: entries_total,
                        entries_total,
                    });
                },
            )?;
            crate::disguise::probe(&staged)?.ok_or_else(|| {
                ArchiveError::Corrupt("伪装文件写入后无法解析尾部信息".to_string())
            })?;
            let output_size = fs::metadata(&staged).map(|meta| meta.len()).unwrap_or(0);
            progress(CreateProgress {
                current: "写入伪装容器".to_string(),
                bytes_done: source_total,
                bytes_total: source_total,
                current_bytes_done: source_total,
                current_bytes_total: source_total,
                output_bytes: output_size,
                entries_done: entries_total,
                entries_total,
            });
            atomic_replace_file(&staged, output).map_err(ArchiveError::Io)?;
            Ok(())
        })();
        let _ = fs::remove_file(&inner);
        if let Err(error) = result {
            let _ = fs::remove_file(&staged);
            return Err(error);
        }
        report.output = output.to_path_buf();
        if options.delete_source {
            remove_source_paths(sources, &mut report.cleanup_errors);
        }
        Ok(report)
    }

    /// 探测路径是否为 Lumark 伪装文件；供界面菜单与一键还原使用。
    pub fn probe_disguise(
        path: &Path,
    ) -> Result<Option<crate::disguise::DisguiseInfo>, ArchiveError> {
        crate::disguise::probe(path)
    }

    /// 批量把普通文件伪装成媒体外观。
    ///
    /// 每个源文件生成独立的 `<stem>.<面具扩展名>` 输出；已有格式的归档
    /// 记为可浏览载荷，其余文件记为原始载荷。成功后可以按需删除源文件。
    pub fn disguise_files<F>(
        sources: &[PathBuf],
        options: &DisguiseFileOptions,
        mut progress: F,
    ) -> Result<crate::disguise::DisguiseReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        options.mask.validate()?;
        fs::create_dir_all(&options.output_dir)?;
        let mask_extension = options.mask.extension()?;
        let entries_total = sources.len();
        let overall_total: u64 = sources
            .iter()
            .filter_map(|path| fs::metadata(path).ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum();
        let mut report = crate::disguise::DisguiseReport::default();
        let mut bytes_done = 0u64;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        for (index, source) in sources.iter().enumerate() {
            ensure_not_cancelled(&options.cancel)?;
            let metadata = fs::metadata(source)?;
            if !metadata.is_file() {
                return Err(ArchiveError::InvalidRequest(format!(
                    "只能伪装普通文件：{}",
                    source.display()
                )));
            }
            if crate::disguise::probe(source)?.is_some() {
                return Err(ArchiveError::InvalidRequest(format!(
                    "源文件已经是 Lumark 伪装文件：{}",
                    source.display()
                )));
            }
            let file_name = source
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    ArchiveError::InvalidRequest(format!("无法读取源文件名：{}", source.display()))
                })?
                .to_string();
            let stem = source
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("file");
            let mut target = options.output_dir.join(format!("{stem}.{mask_extension}"));
            if target == *source || target.exists() {
                target = unique_destination(&target);
            }
            let kind = if Self::detect(source) != ArchiveFormat::Unknown {
                DisguisePayloadKind::Archive
            } else {
                DisguisePayloadKind::RawFile
            };
            let staged = options.output_dir.join(format!(
                ".lumark-disguise-{}-{}-{index}.part",
                std::process::id(),
                stamp
            ));
            let current_total = metadata.len();
            let base_done = bytes_done;
            let files_done = report.files;
            let current_name = file_name.clone();
            let wrapped = crate::disguise::wrap(
                source,
                &staged,
                &options.mask,
                &file_name,
                kind,
                options.cancel.as_ref(),
                |copied, file_total| {
                    progress(CreateProgress {
                        current: current_name.clone(),
                        bytes_done: base_done + copied,
                        bytes_total: overall_total.max(base_done + current_total),
                        current_bytes_done: copied,
                        current_bytes_total: file_total.max(current_total),
                        output_bytes: copied,
                        entries_done: files_done,
                        entries_total,
                    });
                },
            );
            if let Err(error) = wrapped {
                let _ = fs::remove_file(&staged);
                return Err(error);
            }
            if let Err(error) = atomic_replace_file(&staged, &target) {
                let _ = fs::remove_file(&staged);
                return Err(ArchiveError::Io(error));
            }
            report.outputs.push(target);
            report.files += 1;
            report.bytes += current_total;
            bytes_done += current_total;
            if options.delete_source
                && let Err(error) = fs::remove_file(source)
            {
                report
                    .cleanup_errors
                    .push(format!("{}：{error}", source.display()));
            }
        }
        Ok(report)
    }

    /// 把伪装文件还原成原归档或原文件。
    pub fn restore_disguise<F>(
        archive: &Path,
        output_dir: &Path,
        cancel: Option<&Arc<AtomicBool>>,
        progress: F,
    ) -> Result<crate::disguise::RestoreReport, ArchiveError>
    where
        F: FnMut(ExtractProgress),
    {
        let mut progress = progress;
        crate::disguise::restore(archive, output_dir, cancel, |done, total| {
            progress(ExtractProgress {
                current: "还原伪装文件".to_string(),
                bytes_done: done,
                bytes_total: total,
                entries_done: 0,
                entries_total: 1,
            });
        })
    }
}
