//! 自解压运行时模块。
//!
//! 生成端把本程序作为前缀、归档数据作为后缀，并在文件末尾写入 v2 页脚
//! （`LMSFX002` 魔数 + 格式 + 标志 + 偏移 + 长度）。运行时同时识别旧的
//! 24 字节 v1 页脚，从自身文件读取数据段，按页脚声明的 ZIP/7Z 格式解压。
//!
//! 解压路径的包含性、符号链接和覆盖行为在这里统一校验；交互式目录和密码
//! 选择由 `main.rs` 的 Win32 对话框完成。
//!
//! 页脚格式由 `lumark-archive` 写入，两边必须保持一致。

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub mod plan;

pub use plan::{SfxPlan, parse_args};

/// v1 页脚魔数，用于兼容旧自解压包。
pub const SFX_MAGIC: [u8; 8] = *b"LMSFX001";

/// v2 页脚魔数。
pub const SFX_MAGIC_V2: [u8; 8] = *b"LMSFX002";

/// v1 页脚长度：魔数 8 + 偏移 8 + 长度 8。
pub const FOOTER_LEN_V1: usize = 24;

/// v2 页脚长度：魔数 8 + 格式 1 + 标志 1 + 保留 6 + 偏移 8 + 长度 8。
pub const FOOTER_LEN: usize = 32;

/// 自解压载荷格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfxFormat {
    /// ZIP 载荷。
    Zip,
    /// 7Z 载荷。
    SevenZip,
}

/// 自解压数据段在文件中的位置和载荷属性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxFooter {
    /// 载荷格式（ZIP/7Z）。
    pub format: SfxFormat,
    /// 载荷是否加密。
    pub encrypted: bool,
    /// 归档头是否加密。
    pub header_encrypted: bool,
    /// 数据段起始偏移，等于运行时模块的字节数。
    pub offset: u64,
    /// 数据段长度。
    pub length: u64,
}

/// 一次解压任务的进度快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfxProgress {
    /// 当前处理的条目名称。
    pub current: String,
    /// 已处理字节数。
    pub bytes_done: u64,
    /// 计划处理的总字节数；无法统计时为 0。
    pub bytes_total: u64,
    /// 当前条目已处理字节数。
    pub current_file_done: u64,
    /// 当前条目总字节数。
    pub current_file_total: u64,
    /// 已处理条目数。
    pub entries_done: usize,
    /// 计划处理的总条目数。
    pub entries_total: usize,
}

/// 一次解压的可选行为：密码、覆盖策略、取消标记与进度回调。
#[derive(Default)]
pub struct SfxExtractOptions<'a> {
    /// 解密密码；未提供时按未加密处理。
    pub password: Option<&'a str>,
    /// 目标目录已有同名文件时是否覆盖。
    pub overwrite: bool,
    /// 取消标记；置位后解压尽快停止并返回 [`SfxError::Cancelled`]。
    pub cancel: Option<&'a AtomicBool>,
    /// 进度回调；解压过程中每次进度变化都会收到快照。
    pub progress: Option<&'a mut dyn FnMut(SfxProgress)>,
}

/// 自解压运行时可区分的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfxError {
    /// 需要密码。
    PasswordRequired,
    /// 密码错误。
    WrongPassword,
    /// 用户在解压过程中请求取消。
    Cancelled,
    /// 其他解压错误。
    Other(String),
}

impl fmt::Display for SfxError {
    /// 输出可直接展示给用户的中文错误文本。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PasswordRequired => formatter.write_str("该自解压包需要密码"),
            Self::WrongPassword => formatter.write_str("密码错误"),
            Self::Cancelled => formatter.write_str("已取消解压"),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SfxError {}

/// 解析文件末尾的页脚；优先识别 v2，其次兼容 v1。
pub fn parse_footer(tail: &[u8]) -> Option<SfxFooter> {
    if tail.len() >= FOOTER_LEN {
        let footer = &tail[tail.len() - FOOTER_LEN..];
        if footer[..8] == SFX_MAGIC_V2 {
            let format = match footer[8] {
                1 => SfxFormat::SevenZip,
                _ => SfxFormat::Zip,
            };
            return Some(SfxFooter {
                format,
                encrypted: footer[9] & 1 != 0,
                header_encrypted: footer[9] & 2 != 0,
                offset: u64::from_le_bytes(footer[16..24].try_into().ok()?),
                length: u64::from_le_bytes(footer[24..32].try_into().ok()?),
            });
        }
    }
    if tail.len() >= FOOTER_LEN_V1 {
        let footer = &tail[tail.len() - FOOTER_LEN_V1..];
        if footer[..8] == SFX_MAGIC {
            return Some(SfxFooter {
                format: SfxFormat::Zip,
                encrypted: false,
                header_encrypted: false,
                offset: u64::from_le_bytes(footer[8..16].try_into().ok()?),
                length: u64::from_le_bytes(footer[16..24].try_into().ok()?),
            });
        }
    }
    None
}

/// 读取已打开文件末尾的页脚。
pub fn read_footer(file: &mut File) -> io::Result<Option<SfxFooter>> {
    let total = file.metadata()?.len();
    if total < FOOTER_LEN_V1 as u64 {
        return Ok(None);
    }
    let read_len = if total >= FOOTER_LEN as u64 {
        FOOTER_LEN
    } else {
        FOOTER_LEN_V1
    };
    file.seek(SeekFrom::End(-(read_len as i64)))?;
    let mut tail = vec![0u8; read_len];
    file.read_exact(&mut tail)?;
    Ok(parse_footer(&tail))
}

/// 自解压包的默认解压目录：与可执行文件同名的文件夹。
pub fn default_target(exe: &Path) -> PathBuf {
    let parent = exe.parent().unwrap_or_else(|| Path::new("."));
    let stem = exe
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("lumark-sfx");
    parent.join(stem)
}

/// 把页脚指向的数据解压到目标目录，返回写出的文件数。
///
/// 使用默认选项：无密码、不覆盖、不回调进度、不可取消。
pub fn extract_payload(file: File, footer: SfxFooter, target: &Path) -> Result<usize, SfxError> {
    extract_payload_with_options(file, footer, target, SfxExtractOptions::default())
}

/// 按页脚格式解压，支持密码和覆盖策略。
///
/// 保留为兼容入口，等价于不带进度回调和取消标记的
/// [`extract_payload_with_options`]。
pub fn extract_payload_with_password(
    file: File,
    footer: SfxFooter,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
) -> Result<usize, SfxError> {
    extract_payload_with_options(
        file,
        footer,
        target,
        SfxExtractOptions {
            password,
            overwrite,
            cancel: None,
            progress: None,
        },
    )
}

/// 按页脚格式解压，支持密码、覆盖策略、进度回调和取消。
///
/// 预取消优先于密码校验：用户主动取消时不再要求输入密码。ZIP 和 7Z
/// 的条目路径校验、密码尝试和覆盖策略都保持不变。
pub fn extract_payload_with_options(
    file: File,
    footer: SfxFooter,
    target: &Path,
    mut options: SfxExtractOptions<'_>,
) -> Result<usize, SfxError> {
    if is_cancelled(options.cancel) {
        return Err(SfxError::Cancelled);
    }
    if footer.encrypted && options.password.is_none_or(str::is_empty) {
        return Err(SfxError::PasswordRequired);
    }
    let reader = PayloadReader {
        file,
        start: footer.offset,
        len: footer.length,
        pos: 0,
    };
    match footer.format {
        SfxFormat::Zip => extract_zip(reader, target, &mut options),
        SfxFormat::SevenZip => extract_seven_zip(reader, target, footer, &mut options),
    }
}

/// 解压 ZIP 载荷；加密条目逐个尝试密码。
///
/// 先遍历中央目录统计条目数和总字节数，再逐条目上报进度；复制按块进行，
/// 每块之间检查取消标记。
fn extract_zip(
    reader: PayloadReader,
    target: &Path,
    options: &mut SfxExtractOptions<'_>,
) -> Result<usize, SfxError> {
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|error| SfxError::Other(format!("自解压数据损坏：{error}")))?;
    // 中央目录已包含全部条目元数据，先统计以便进度回调给出总量。
    let mut entries_total = 0usize;
    let mut bytes_total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| SfxError::Other(format!("解压失败：{error}")))?;
        if entry.is_dir() {
            continue;
        }
        entries_total += 1;
        bytes_total = bytes_total.saturating_add(entry.size());
    }
    fs::create_dir_all(target)
        .map_err(|error| SfxError::Other(format!("无法创建解压目录：{error}")))?;
    let password = options.password;
    let overwrite = options.overwrite;
    let mut reporter = ProgressReporter::new(options);
    reporter.entries_total = entries_total;
    reporter.bytes_total = bytes_total;
    let mut files = 0;
    for index in 0..archive.len() {
        if reporter.is_cancelled() {
            return Err(SfxError::Cancelled);
        }
        let mut entry = if archive
            .by_index_raw(index)
            .map(|entry| entry.encrypted())
            .unwrap_or(false)
        {
            let password = password.ok_or(SfxError::PasswordRequired)?;
            match archive.by_index_decrypt(index, password.as_bytes()) {
                Ok(entry) => entry,
                Err(zip::result::ZipError::InvalidPassword) => {
                    return Err(SfxError::WrongPassword);
                }
                Err(error) => return Err(SfxError::Other(format!("解压失败：{error}"))),
            }
        } else {
            archive
                .by_index(index)
                .map_err(|error| SfxError::Other(format!("解压失败：{error}")))?
        };
        let Some(enclosed) = entry.enclosed_name() else {
            let name = entry
                .name()
                .map(|name| name.to_string())
                .unwrap_or_else(|_| "<未知条目>".to_string());
            return Err(SfxError::Other(format!("归档条目路径不安全：{name}")));
        };
        let destination = target.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| SfxError::Other(format!("无法创建目录：{error}")))?;
            continue;
        }
        let size = entry.size();
        if destination.exists() && !overwrite {
            // 跳过的文件同样计入已完成字节和条目数，进度才不会停在半路。
            reporter.skip_entry(size);
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| SfxError::Other(format!("无法创建目录：{error}")))?;
        }
        let name = entry
            .name()
            .map(|name| name.to_string())
            .unwrap_or_else(|_| "<未知条目>".to_string());
        let mut output = File::create(&destination)
            .map_err(|error| SfxError::Other(format!("无法写入文件：{error}")))?;
        reporter.begin_entry(&name, size);
        copy_chunked(&mut entry, &mut output, &mut reporter)?;
        reporter.finish_entry();
        files += 1;
    }
    Ok(files)
}

/// 解压 7Z 载荷；路径安全由 `extract_seven_zip_entry` 统一检查。
///
/// 先用归档元数据统计非目录条目和总字节数，再在 `for_each_entries` 回调里
/// 逐条目上报进度并检查取消；取消统一映射为 [`SfxError::Cancelled`]，
/// 不会误报成密码错误。加密归档头解析失败仍按 [`SfxError::WrongPassword`] 处理。
fn extract_seven_zip(
    reader: PayloadReader,
    target: &Path,
    footer: SfxFooter,
    options: &mut SfxExtractOptions<'_>,
) -> Result<usize, SfxError> {
    let password = options.password.unwrap_or_default();
    let mut archive =
        sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::from(password)).map_err(
            |error| {
                if footer.header_encrypted || footer.encrypted {
                    SfxError::WrongPassword
                } else {
                    SfxError::Other(format!("自解压数据损坏：{error}"))
                }
            },
        )?;
    fs::create_dir_all(target)
        .map_err(|error| SfxError::Other(format!("无法创建解压目录：{error}")))?;
    let mut entries_total = 0usize;
    let mut bytes_total = 0u64;
    for entry in &archive.archive().files {
        if entry.is_directory() {
            continue;
        }
        entries_total += 1;
        bytes_total = bytes_total.saturating_add(entry.size);
    }
    let overwrite = options.overwrite;
    let mut reporter = ProgressReporter::new(options);
    reporter.entries_total = entries_total;
    reporter.bytes_total = bytes_total;
    let mut files = 0usize;
    let mut cancelled = false;
    let result = archive.for_each_entries(|entry, input| {
        match extract_seven_zip_entry(entry, input, target, overwrite, &mut reporter) {
            Ok(true) => {
                files += 1;
                Ok(true)
            }
            Ok(false) => Ok(true),
            Err(SfxError::Cancelled) => {
                // 用内部 IO 错误让解码器立即停止，再由外层统一转换成 Cancelled。
                cancelled = true;
                Err(cancelled_io_error().into())
            }
            Err(error) => Err(io::Error::other(error.to_string()).into()),
        }
    });
    match result {
        Ok(()) => Ok(files),
        Err(_) if cancelled => Err(SfxError::Cancelled),
        Err(error) => Err(SfxError::Other(format!("解压失败：{error}"))),
    }
}

/// 解压 7Z 的单个条目，返回是否实际写出文件。
///
/// 目录只创建目录；目标已存在且不覆盖时仍然读完条目数据，保证 solid 归档
/// 的后续条目可以正常解码，同时把跳过的字节计入进度。
fn extract_seven_zip_entry(
    entry: &sevenz_rust2::ArchiveEntry,
    input: &mut dyn Read,
    target: &Path,
    overwrite: bool,
    reporter: &mut ProgressReporter<'_>,
) -> Result<bool, SfxError> {
    if reporter.is_cancelled() {
        return Err(SfxError::Cancelled);
    }
    if entry.is_directory() {
        let destination = safe_destination(target, &entry.name, true)
            .map_err(|error| SfxError::Other(error.to_string()))?;
        fs::create_dir_all(destination).map_err(|error| SfxError::Other(error.to_string()))?;
        return Ok(false);
    }
    let destination = safe_destination(target, &entry.name, false)
        .map_err(|error| SfxError::Other(error.to_string()))?;
    reporter.begin_entry(&entry.name, entry.size);
    if destination.exists() && !overwrite {
        let mut sink = io::sink();
        copy_chunked(input, &mut sink, reporter)?;
        reporter.finish_entry();
        return Ok(false);
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| SfxError::Other(error.to_string()))?;
    }
    let mut output =
        File::create(&destination).map_err(|error| SfxError::Other(error.to_string()))?;
    copy_chunked(input, &mut output, reporter)?;
    reporter.finish_entry();
    Ok(true)
}

/// 把归档内路径拼到目标目录，并拒绝绝对路径和 `..`。
fn safe_destination(target: &Path, name: &str, is_dir: bool) -> io::Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    let relative = Path::new(&normalized);
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("归档条目路径不安全：{name}"),
        ));
    }
    let mut destination = target.join(relative);
    if is_dir && !destination.as_os_str().is_empty() {
        destination.push("");
    }
    Ok(destination)
}

/// 单次复制块大小：256 KiB。
///
/// 分块复制让取消请求能在一两秒内生效，同时避免过于频繁的进度回调。
const COPY_CHUNK: usize = 256 * 1024;

/// 判断取消标记是否已置位。
fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// 构造表示“已取消解压”的内部 IO 错误，供 7Z 回调中断解码器。
fn cancelled_io_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "已取消解压")
}

/// 解压过程中的进度汇总器。
///
/// 持有进度回调和取消标记，累计字节与条目计数，并在每个变化点生成一次
/// [`SfxProgress`]；未提供回调时只更新内部计数。
struct ProgressReporter<'a> {
    /// 取消标记；置位后读写循环尽快停止。
    cancel: Option<&'a AtomicBool>,
    /// 进度回调；仅在解压线程内独占调用。
    progress: Option<&'a mut dyn FnMut(SfxProgress)>,
    /// 已处理字节数。
    bytes_done: u64,
    /// 计划处理的总字节数。
    bytes_total: u64,
    /// 已处理条目数。
    entries_done: usize,
    /// 计划处理的总条目数。
    entries_total: usize,
    /// 当前条目名称。
    current: String,
    /// 当前条目已处理字节数。
    current_file_done: u64,
    /// 当前条目总字节数。
    current_file_total: u64,
}

impl<'a> ProgressReporter<'a> {
    /// 从解压选项中取出进度回调并初始化计数器。
    fn new(options: &mut SfxExtractOptions<'a>) -> Self {
        Self {
            cancel: options.cancel,
            progress: options.progress.take(),
            bytes_done: 0,
            bytes_total: 0,
            entries_done: 0,
            entries_total: 0,
            current: String::new(),
            current_file_done: 0,
            current_file_total: 0,
        }
    }

    /// 返回是否已经收到取消请求。
    fn is_cancelled(&self) -> bool {
        is_cancelled(self.cancel)
    }

    /// 上报一次当前进度快照；没有回调时不做任何事。
    fn emit(&mut self) {
        let snapshot = SfxProgress {
            current: self.current.clone(),
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
            current_file_done: self.current_file_done,
            current_file_total: self.current_file_total,
            entries_done: self.entries_done,
            entries_total: self.entries_total,
        };
        if let Some(callback) = self.progress.as_mut() {
            callback(snapshot);
        }
    }

    /// 进入新条目并上报起始进度。
    fn begin_entry(&mut self, name: &str, size: u64) {
        self.current.clear();
        self.current.push_str(name);
        self.current_file_done = 0;
        self.current_file_total = size;
        self.emit();
    }

    /// 累计已处理字节并上报。
    fn advance(&mut self, delta: u64) {
        self.bytes_done = self.bytes_done.saturating_add(delta);
        self.current_file_done = self.current_file_done.saturating_add(delta);
        self.emit();
    }

    /// 跳过已有文件时把计划字节和条目计入已完成进度。
    fn skip_entry(&mut self, size: u64) {
        self.bytes_done = self.bytes_done.saturating_add(size);
        self.entries_done += 1;
        self.emit();
    }

    /// 条目处理完成后更新条目计数并上报。
    fn finish_entry(&mut self) {
        self.entries_done += 1;
        self.emit();
    }
}

/// 分块复制条目数据，逐块累计进度并在块之间检查取消。
fn copy_chunked(
    input: &mut dyn Read,
    output: &mut dyn Write,
    reporter: &mut ProgressReporter<'_>,
) -> Result<(), SfxError> {
    let mut buffer = vec![0u8; COPY_CHUNK];
    loop {
        if reporter.is_cancelled() {
            return Err(SfxError::Cancelled);
        }
        let read = input
            .read(&mut buffer)
            .map_err(|error| SfxError::Other(format!("解压失败：{error}")))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| SfxError::Other(format!("无法写入文件：{error}")))?;
        reporter.advance(read as u64);
    }
    Ok(())
}

/// 只暴露文件中数据段的读写视图，供归档解码器顺序读取。
struct PayloadReader {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
}

impl Read for PayloadReader {
    /// 从载荷视图读取并受长度边界限制。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let remaining = (self.len - self.pos) as usize;
        let limit = buffer.len().min(remaining);
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let read = self.file.read(&mut buffer[..limit])?;
        self.pos += read as u64;
        Ok(read)
    }
}

impl Seek for PayloadReader {
    /// 在载荷范围内定位。
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(value) => value as i64,
            SeekFrom::Current(value) => self.pos as i64 + value,
            SeekFrom::End(value) => self.len as i64 + value,
        };
        if target < 0 || target as u64 > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "自解压数据段越界",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 组装 v2 测试页脚。
    fn footer_bytes(offset: u64, length: u64, format: SfxFormat, encrypted: bool) -> Vec<u8> {
        let mut footer = Vec::new();
        footer.extend_from_slice(&SFX_MAGIC_V2);
        footer.push(match format {
            SfxFormat::Zip => 0,
            SfxFormat::SevenZip => 1,
        });
        footer.push(u8::from(encrypted));
        footer.extend_from_slice(&[0u8; 6]);
        footer.extend_from_slice(&offset.to_le_bytes());
        footer.extend_from_slice(&length.to_le_bytes());
        footer
    }

    /// 把运行时模块、归档数据和页脚写成一个自解压文件。
    fn write_sfx(
        directory: &Path,
        stub: &[u8],
        payload: &[u8],
        format: SfxFormat,
        encrypted: bool,
    ) -> PathBuf {
        let path = directory.join("sample.exe");
        let mut file = File::create(&path).unwrap();
        file.write_all(stub).unwrap();
        file.write_all(payload).unwrap();
        file.write_all(&footer_bytes(
            stub.len() as u64,
            payload.len() as u64,
            format,
            encrypted,
        ))
        .unwrap();
        path
    }

    /// 生成只包含一个文本条目的测试 ZIP。
    fn payload_with(directory: &Path, name: &str, contents: &[u8]) -> Vec<u8> {
        let path = directory.join("payload.zip");
        {
            let file = File::create(&path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
            writer.finish().unwrap();
        }
        fs::read(&path).unwrap()
    }

    /// 页脚解析应读回 v2 格式和加密标志，并兼容 v1。
    #[test]
    fn footer_round_trips() {
        let bytes = footer_bytes(4096, 128, SfxFormat::SevenZip, true);
        let footer = parse_footer(&bytes).expect("v2 页脚");
        assert_eq!(footer.format, SfxFormat::SevenZip);
        assert!(footer.encrypted);
        assert_eq!((footer.offset, footer.length), (4096, 128));

        let mut v1 = Vec::new();
        v1.extend_from_slice(&SFX_MAGIC);
        v1.extend_from_slice(&64u64.to_le_bytes());
        v1.extend_from_slice(&32u64.to_le_bytes());
        let footer = parse_footer(&v1).expect("v1 页脚");
        assert_eq!(footer.format, SfxFormat::Zip);
        assert!(!footer.encrypted);
        assert_eq!((footer.offset, footer.length), (64, 32));

        assert_eq!(parse_footer(&bytes[..10]), None);
    }

    /// 默认解压目录取可执行文件同目录的同名文件夹。
    #[test]
    fn default_target_uses_exe_stem() {
        let target = default_target(Path::new(r"C:\downloads\photos.exe"));
        assert_eq!(target, PathBuf::from(r"C:\downloads\photos"));
    }

    /// 验证 ZIP 载荷跳过运行时模块并写出文件。
    #[test]
    fn extraction_reads_zip_payload_after_stub() {
        let directory = tempfile::tempdir().unwrap();
        let payload = payload_with(directory.path(), "hello.txt", b"hello sfx");
        let stub = b"MZfake-stub-bytes";
        let sfx_path = write_sfx(directory.path(), stub, &payload, SfxFormat::Zip, false);
        let mut file = File::open(&sfx_path).unwrap();
        let footer = read_footer(&mut file).unwrap().expect("页脚");
        assert_eq!(footer.offset, stub.len() as u64);
        let target = directory.path().join("out");
        let files = extract_payload(file, footer, &target).unwrap();
        assert_eq!(files, 1);
        assert_eq!(
            fs::read_to_string(target.join("hello.txt")).unwrap(),
            "hello sfx"
        );
    }

    /// 带 `..` 的条目必须被拒绝，不能写出目标目录。
    #[test]
    fn traversal_entries_do_not_escape_target() {
        let directory = tempfile::tempdir().unwrap();
        let payload = payload_with(directory.path(), "../escape.txt", b"escape");
        let sfx_path = write_sfx(directory.path(), b"MZstub", &payload, SfxFormat::Zip, false);
        let mut file = File::open(&sfx_path).unwrap();
        let footer = read_footer(&mut file).unwrap().expect("页脚");
        let target = directory.path().join("out");
        let result = extract_payload(file, footer, &target);
        assert!(result.is_err(), "路径穿越条目应被拒绝");
        assert!(
            !directory.path().join("escape.txt").exists(),
            "不能写出目标目录"
        );
    }

    /// 加密 ZIP 必须要求密码，错误密码返回可重试错误。
    #[test]
    fn encrypted_zip_requires_and_validates_password() {
        let directory = tempfile::tempdir().unwrap();
        let payload = payload_with(directory.path(), "hello.txt", b"secret");
        let sfx_path = write_sfx(directory.path(), b"MZstub", &payload, SfxFormat::Zip, true);
        let mut file = File::open(&sfx_path).unwrap();
        let footer = read_footer(&mut file).unwrap().expect("页脚");
        let target = directory.path().join("out");
        assert!(matches!(
            extract_payload_with_password(
                File::open(&sfx_path).unwrap(),
                footer,
                &target,
                None,
                false
            ),
            Err(SfxError::PasswordRequired)
        ));
    }
}
