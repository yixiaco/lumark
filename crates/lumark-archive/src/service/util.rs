//! 归档时间、格式、源计划和流读取辅助。

use super::*;

/// 把系统时间转换为 ZIP 可写入的本地时间。
pub(crate) fn zip_datetime(time: SystemTime) -> Option<zip::DateTime> {
    let local: DateTime<chrono::Local> = time.into();
    zip::DateTime::try_from(local.naive_local()).ok()
}

/// 把系统时间写入 7Z 条目的修改时间字段。
pub(crate) fn apply_entry_time(entry: &mut sevenz_rust2::ArchiveEntry, time: SystemTime) {
    if let Ok(date) = NtTime::try_from(time) {
        entry.last_modified_date = date;
        entry.has_last_modified_date = true;
    }
}

/// 把界面压缩选项映射为 ZIP 压缩方法。
pub(crate) fn zip_method(options: &CompressionOptions) -> CompressionMethod {
    match options.zip_compression {
        crate::format::ZipCompression::Deflate => CompressionMethod::Deflated,
        crate::format::ZipCompression::Bzip2 => CompressionMethod::Bzip2,
        crate::format::ZipCompression::Xz => CompressionMethod::Xz,
        crate::format::ZipCompression::Zstd => CompressionMethod::Zstd,
        crate::format::ZipCompression::Ppmd => CompressionMethod::Ppmd,
        crate::format::ZipCompression::Lzma => CompressionMethod::Lzma,
    }
}

/// 把 ZIP 本地 DOS 时间转换为 UTC。
pub(crate) fn zip_time_to_chrono(value: Option<zip::DateTime>) -> Option<DateTime<Utc>> {
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

/// 判断条目是否在选择性解压列表中。
pub(crate) fn selected(name: &str, selected_paths: &[String]) -> bool {
    if selected_paths.is_empty() {
        return true;
    }
    let name = name.replace('\\', "/");
    selected_paths.iter().any(|selected| {
        let selected = selected.replace('\\', "/");
        name == selected || name.starts_with(&format!("{selected}/"))
    })
}

/// 展开全部源后的文件列表、总字节和最新修改时间。
pub(crate) struct SourcePlan {
    /// 归档内相对路径、源文件路径和规划时的字节数。
    pub(crate) files: Vec<(String, PathBuf, u64)>,
    /// 全部源文件字节数。
    pub(crate) total_bytes: u64,
    /// 源文件中最新的修改时间。
    pub(crate) latest_modified: Option<SystemTime>,
}

/// 返回伪装包原文件名去掉最后一个扩展名后的 stem。
///
/// 单文件流解压需要根据内层文件名生成输出名；外层 `.mp4` 面具扩展名不能
/// 参与命名，否则会得到 `photos` 之外的错误结果。
pub(crate) fn disguise_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("data")
        .to_string()
}

/// 展开源目录并按路径模式、排除和修改时间生成计划。
pub(crate) fn build_source_plan(
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
            // 符号链接和 Windows 目录联接不作为普通文件加入归档。zpaq 会
            // 直接跳过重解析点，ZIP/TAR 也无法把目录当文件读取；链接目标
            // 如果位于源树内，会按真实路径另行收录。
            if entry.file_type().is_symlink() || entry.path_is_symlink() {
                continue;
            }
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
pub(crate) const LZMA2_CHUNK_BYTES: u64 = 16 * 1024 * 1024;

/// lzma-rust2 预设级别对应的字典大小，用于推算多线程 LZMA2 的实际块大小。
///
/// 固定依赖 sevenz-rust2 0.24.1 / lzma-rust2 0.23.0；升级压缩库时需要同步
/// 检查这张表与 `from_level_mt` 的钳制逻辑。
pub(crate) fn lzma2_preset_dictionary(level: u32) -> u64 {
    /// lzma-rust2 预设级别对应的字典大小。
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
pub(crate) const DEFAULT_SOLID_BLOCK_BYTES: u64 = 2048 * 1024 * 1024;

/// 单个固实块最多包含的文件数，作为超大目录的兜底限制。
///
/// 正常情况下由“固实块大小”决定块边界；这个上限只在文件极多且极小时生效，
/// 避免一个块一次性打开几十万个句柄。
pub(crate) const SOLID_BLOCK_MAX_FILES: usize = 16384;

/// 把待压缩文件切成固实块，同时受字节上限和文件数上限约束。
///
/// 7Z 的固实块需要先打开块内全部源文件再统一压缩，块越大占用越多；单独
/// 一个文件已经超过字节上限时让它独占一块，否则整个块会被它无限拉长。
pub(crate) fn solid_blocks(
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

/// 求多个源路径的共同父目录，用于计算归档内相对路径。
pub(crate) fn common_base(sources: &[PathBuf]) -> PathBuf {
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

/// 用 glob 规则匹配归档内相对路径。
pub(crate) fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.replace('\\', "/").to_lowercase();
    let value = value.replace('\\', "/").to_lowercase();
    if let Ok(glob) = globset::Glob::new(&pattern) {
        glob.compile_matcher().is_match(&value)
    } else {
        false
    }
}

/// 把压缩预设映射为 LZH 写入方法。
///
/// 显式 `level == 0` 表示仅存储；其余按速度/平衡/体积映射到 lh4/lh5/lh7。
pub(crate) fn lzh_method(options: &CompressionOptions) -> oxiarc_lzhuf::LzhMethod {
    if options.level == Some(0) {
        return oxiarc_lzhuf::LzhMethod::Lh0;
    }
    match options.preset {
        crate::format::CompressionPreset::Speed => oxiarc_lzhuf::LzhMethod::Lh4,
        crate::format::CompressionPreset::Balanced => oxiarc_lzhuf::LzhMethod::Lh5,
        crate::format::CompressionPreset::Size => oxiarc_lzhuf::LzhMethod::Lh7,
    }
}
/// 为 TAR 变体套上对应的解压读取器。
pub(crate) fn tar_reader(
    reader: Box<dyn Read>,
    format: ArchiveFormat,
) -> Result<tar::Archive<Box<dyn Read>>, ArchiveError> {
    let reader: Box<dyn Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::GzDecoder::new(reader)),
        ArchiveFormat::TarBz2 => Box::new(bzip2::read::BzDecoder::new(reader)),
        ArchiveFormat::TarXz => Box::new(lzma_rust2::XzReader::new(reader, true)),
        ArchiveFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(reader)?),
        ArchiveFormat::TarBr => Box::new(brotli::Decompressor::new(reader, 64 * 1024)),
        ArchiveFormat::TarLz4 => Box::new(lz4_flex::frame::FrameDecoder::new(reader)),
        ArchiveFormat::TarLz => Box::new(lzma_rust2::LzipReader::new(reader)),
        ArchiveFormat::TarLzma => Box::new(
            lzma_rust2::LzmaReader::new_mem_limit(reader, u32::MAX, None)
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?,
        ),
        _ => reader,
    };
    Ok(tar::Archive::new(reader))
}

/// 把源计划中的文件追加到 TAR 构建器。
pub(crate) fn append_tar_sources<F>(
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

/// 分块复制单文件并按节流上报进度。
pub(crate) fn copy_with_progress<F, W: Write>(
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

/// 为单文件流创建解压读取器。
pub(crate) fn decompress_reader(
    reader: Box<dyn Read>,
    format: ArchiveFormat,
) -> Result<Box<dyn Read>, ArchiveError> {
    Ok(match format {
        ArchiveFormat::Gzip => Box::new(flate2::read::GzDecoder::new(reader)),
        ArchiveFormat::Bzip2 => Box::new(bzip2::read::BzDecoder::new(reader)),
        ArchiveFormat::Xz => Box::new(lzma_rust2::XzReader::new(reader, true)),
        ArchiveFormat::Zstd => Box::new(zstd::stream::read::Decoder::new(reader)?),
        ArchiveFormat::Brotli => Box::new(brotli::Decompressor::new(reader, 64 * 1024)),
        ArchiveFormat::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(reader)),
        ArchiveFormat::Snappy => Box::new(snappy_frame_reader(reader)?),
        ArchiveFormat::Bz3 => Box::new(
            bzip3_rs::read::Decoder::new(reader)
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?,
        ),
        other => return Err(ArchiveError::UnsupportedFormat(other.label().to_string())),
    })
}

/// 只接受标准 Snappy Framed 流；Raw/Hadoop 流给出可操作的错误。
fn snappy_frame_reader(mut reader: Box<dyn Read>) -> Result<Box<dyn Read>, ArchiveError> {
    const MAGIC: &[u8; 10] = b"\xff\x06\x00\x00sNaPpY";
    let mut header = [0u8; 10];
    reader.read_exact(&mut header).map_err(|_| {
        ArchiveError::UnsupportedFormat(
            "Snappy Raw/Hadoop 流不受支持；请使用 Framed（.sz/.snappy）".to_string(),
        )
    })?;
    if &header != MAGIC {
        return Err(ArchiveError::UnsupportedFormat(
            "Snappy Raw/Hadoop 流不受支持；请使用 Framed（.sz/.snappy）".to_string(),
        ));
    }
    let chained = std::io::Cursor::new(header.to_vec()).chain(reader);
    Ok(Box::new(snap::read::FrameDecoder::new(chained)))
}

/// 打开 Unix .Z 并返回解压读取器。
pub(crate) fn open_unix_compress(
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

/// 解密 AES 容器数据流。
pub(crate) fn decrypt_encrypted_container(
    path: &Path,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, ArchiveError> {
    let mut input = File::open(path)?;
    let mut data = Zeroizing::new(Vec::new());
    input.read_to_end(&mut data)?;
    let decrypted = enc_file::decrypt_bytes(
        &data,
        secrecy::SecretString::new(password.to_string().into()),
    )
    .map_err(|error| {
        ArchiveError::Corrupt(format!("AES 容器解密失败（密码错误或数据损坏）：{error}"))
    })?;
    Ok(Zeroizing::new(decrypted))
}

/// 从加密容器路径推导输出文件名。
pub(crate) fn encrypted_container_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("output")
        .to_string()
}

/// 从 .Z 头或路径推导输出文件名。
pub(crate) fn unix_compress_name(path: &Path, embedded: &str) -> String {
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

/// 判断取消标志是否已置位。
pub(crate) fn is_cancelled(flag: &Option<Arc<AtomicBool>>) -> bool {
    flag.as_ref()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// 取消已请求时返回 Cancelled。
pub(crate) fn ensure_not_cancelled(flag: &Option<Arc<AtomicBool>>) -> Result<(), ArchiveError> {
    if is_cancelled(flag) {
        Err(ArchiveError::Cancelled)
    } else {
        Ok(())
    }
}

/// 判断格式是否由传统只读读取器处理。
pub(crate) fn is_legacy_archive(format: ArchiveFormat) -> bool {
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
            | ArchiveFormat::Lzh
            | ArchiveFormat::Deb
    )
}
