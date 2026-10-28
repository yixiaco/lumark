//! 各归档格式的创建流程。

use super::*;

impl ArchiveService {
    /// 创建 ZIP 归档，按分块响应取消并上报进度。
    pub(crate) fn create_zip<F>(
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
        Self::create_zip_to(File::create(output)?, output, sources, options, progress)
    }

    /// 把 ZIP 写入任意写入器，供伪装单遍写入复用。
    pub(crate) fn create_zip_to<W, F>(
        destination: W,
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        W: Write,
        F: FnMut(CreateProgress),
    {
        let mut writer = ZipWriter::new_stream(destination);
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

    /// 创建 7Z 归档，支持固实块、密码和线程参数。
    pub(crate) fn create_seven_zip<F>(
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
        let plan = build_source_plan(sources, options)?;
        let bytes_total = plan.total_bytes;
        let entries_total = plan.files.len();
        // 总量不足一个编码块时多线程既没有中间进度事件，又多一层线程开销；
        // 改用单线程，读取与编码同步，进度可以每 100 毫秒连续推进。
        let effective_threads = if bytes_total <= LZMA2_CHUNK_BYTES {
            1
        } else {
            options.threads
        };
        // 多线程 LZMA2 会提前读走一整段在途缓冲，按读取字节上报会在几秒内
        // 冲到 100% 然后长时间不动。未加密的 LZMA2 多线程流每完成一个编码
        // 块才向输出写一次，可以据此换算真实进度；加密后输出按 AES 分块写出，
        // 无法区分编码块边界，仍退回读取进度。
        let track_chunks = matches!(
            options.sevenz_compression,
            crate::format::SevenZCompression::Lzma2
        ) && effective_threads > 1
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
            output_bytes: fs::metadata(output).map(|meta| meta.len()).unwrap_or(0),
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
                let mut lzma2 = if effective_threads > 1 {
                    Lzma2Options::from_level_mt(mt_level, effective_threads, LZMA2_CHUNK_BYTES)
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
                let mut lzma = SevenZLzmaOptions::from_level(level);
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

    /// 创建 TAR 及压缩变体归档。
    pub(crate) fn create_tar<F>(
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
        Self::create_tar_to(File::create(output)?, output, sources, options, progress)
    }

    /// 把 TAR 及压缩变体写入任意写入器，供伪装单遍写入复用。
    pub(crate) fn create_tar_to<W, F>(
        destination: W,
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        W: Write,
        F: FnMut(CreateProgress),
    {
        let level = options.level.unwrap_or(6).clamp(1, 9) as u32;
        let mut report = CreateReport {
            output: output.to_path_buf(),
            ..Default::default()
        };
        match options.format {
            ArchiveFormat::Tar => {
                let mut builder = tar::Builder::new(destination);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.finish()?;
            }
            ArchiveFormat::TarGz => {
                let encoder =
                    flate2::write::GzEncoder::new(destination, flate2::Compression::new(level));
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarBz2 => {
                let encoder =
                    bzip2::write::BzEncoder::new(destination, bzip2::Compression::new(level));
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarXz => {
                let encoder = lzma_rust2::XzWriter::new(
                    destination,
                    lzma_rust2::XzOptions::with_preset(level),
                )?;
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarZst => {
                let encoder = zstd::stream::write::Encoder::new(destination, level as i32)?;
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.finish()?;
            }
            ArchiveFormat::TarBr => {
                // Brotli 质量范围是 0–11，与 TAR 其他变体的 1–9 不同。
                let quality = options.level.unwrap_or(6).clamp(0, 11) as u32;
                let encoder = brotli::CompressorWriter::new(destination, 64 * 1024, quality, 22);
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder.into_inner()?.into_inner();
            }
            ArchiveFormat::TarLz4 => {
                let encoder = lz4_flex::frame::FrameEncoder::new(destination);
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder
                    .into_inner()?
                    .finish()
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            }
            ArchiveFormat::TarLz => {
                let preset = options.level.unwrap_or(6).clamp(0, 9) as u32;
                let encoder = LzipWriter::new(destination, LzipOptions::with_preset(preset));
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder
                    .into_inner()?
                    .finish()
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            }
            ArchiveFormat::TarLzma => {
                let preset = options.level.unwrap_or(6).clamp(0, 9) as u32;
                let encoder = LzmaWriter::new_use_header(
                    destination,
                    &LzmaOptions::with_preset(preset),
                    None,
                )
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
                let mut builder = tar::Builder::new(encoder);
                append_tar_sources(&mut builder, sources, options, progress, &mut report)?;
                builder
                    .into_inner()?
                    .finish()
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            }
            _ => {
                return Err(ArchiveError::UnsupportedFormat(
                    options.format.label().to_string(),
                ));
            }
        }
        Ok(report)
    }

    /// 创建未压缩 WIM 归档。
    ///
    /// 上游写入器只支持 Store，文件内容会整体读入内存；因此这里按文件
    /// 读取、写入并上报进度，不支持密码和分卷。
    pub(crate) fn create_wim<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        use geezipx_core::archive::ArchiveWriter as LegacyArchiveWriter;
        use geezipx_core::archive::wim::WimWriter;

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let plan = build_source_plan(sources, options)?;
        let entries_total = plan.files.len();
        let bytes_total = plan.total_bytes;
        let mut report = CreateReport {
            output: output.to_path_buf(),
            ..Default::default()
        };
        let mut writer = WimWriter::new(File::create(output)?);
        for (relative, source, file_bytes) in plan.files {
            ensure_not_cancelled(&options.cancel)?;
            let mut input = File::open(&source)?;
            writer
                .add_entry_from_reader(Path::new(&relative), &mut input)
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            report.files += 1;
            report.bytes = report.bytes.saturating_add(file_bytes);
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
        let _written = Box::new(writer)
            .finish()
            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
        Ok(report)
    }

    /// 创建 LZH/LHA 归档。
    ///
    /// 当前后端按条目整块读入内存，因此对单条目设置 512 MiB 上限；压缩方法
    /// 由预设映射到 lh4/lh5/lh7，`level == 0` 时使用 store。后端没有逐条目
    /// 进度和取消钩子，进度按文件上报。
    pub(crate) fn create_lzh<F>(
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        F: FnMut(CreateProgress),
    {
        let plan = build_source_plan(sources, options)?;
        crate::lzh::create(
            output,
            &plan,
            lzh_method(options),
            &options.cancel,
            progress,
        )
    }

    /// 创建 GZ/BZ2/XZ/ZST/Brotli/LZ4 单文件流。
    pub(crate) fn create_single_stream<F>(
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
        Self::create_single_stream_to(File::create(output)?, output, sources, options, progress)
    }

    /// 把单文件流写入任意写入器，供伪装单遍写入复用。
    pub(crate) fn create_single_stream_to<W, F>(
        destination: W,
        output: &Path,
        sources: &[PathBuf],
        options: &CompressionOptions,
        progress: &mut F,
    ) -> Result<CreateReport, ArchiveError>
    where
        W: Write,
        F: FnMut(CreateProgress),
    {
        let suggestion = options
            .format
            .single_stream_tar_variant()
            .map(|format| format.label())
            .unwrap_or("TAR 归档");
        let source = sources.first().ok_or_else(|| {
            ArchiveError::InvalidRequest(format!(
                "{} 需要一个源文件；压缩多个文件请改用 {suggestion}。",
                options.format.label()
            ))
        })?;
        if sources.len() != 1 {
            return Err(ArchiveError::InvalidRequest(format!(
                "{} 是单文件流格式，只能压缩一个源文件；压缩多个文件请改用 {suggestion}。",
                options.format.label()
            )));
        }
        if !source.is_file() {
            return Err(ArchiveError::InvalidRequest(format!(
                "{} 只能压缩单个文件，不能压缩文件夹；请改用 {suggestion}。",
                options.format.label()
            )));
        }
        let total = fs::metadata(source)?.len();
        let mut input = File::open(source)?;
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
                let mut encoder = lzma_rust2::XzWriter::new(
                    destination,
                    lzma_rust2::XzOptions::with_preset(level),
                )?;
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
