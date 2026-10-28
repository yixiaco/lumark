//! 压缩进度换算、取消读取器和复制辅助。

use super::*;

/// 固实块内单个文件在块内字节轴上的位置。
pub(crate) struct ProgressSpan {
    /// 条目名称。
    pub(crate) name: String,
    /// 条目在源数据中的起始偏移。
    pub(crate) start: u64,
    /// 条目在源数据中的结束偏移。
    pub(crate) end: u64,
}

/// 一次编码阶段的真实进度换算状态。
///
/// 多线程 LZMA2 每完成一个块才向输出写一次，`chunks` 记录已写出的块数；
/// `locate` 把“已压缩字节”换算成当前文件和已完成条目，避免直接使用读取
/// 位置导致进度提前冲到 100%。
pub(crate) struct ChunkProgressState {
    /// 是否处于可计数编码块阶段。
    pub(crate) active: bool,
    /// 本文件之前的累计字节。
    pub(crate) base: u64,
    /// 本文件总字节。
    pub(crate) total: u64,
    /// 单个编码块的大致字节数。
    pub(crate) chunk_bytes: u64,
    /// 已完成的编码块数。
    pub(crate) chunks: u64,
    /// 本文件之前的条目数。
    pub(crate) entries_before: usize,
    /// 总条目数。
    pub(crate) entries_total: usize,
    /// 全部源字节数。
    pub(crate) bytes_total: u64,
    /// 已写出的归档字节数。
    pub(crate) output_bytes: u64,
    /// 文件在源数据中的偏移表。
    pub(crate) spans: Vec<ProgressSpan>,
}

impl ChunkProgressState {
    /// 开始一个新的固实块或单文件阶段，并重置块内计数。
    pub(crate) fn begin(
        &mut self,
        base: u64,
        total: u64,
        entries_before: usize,
        spans: Vec<ProgressSpan>,
    ) {
        self.active = true;
        self.base = base;
        self.total = total;
        self.chunks = 0;
        self.entries_before = entries_before;
        self.spans = spans;
    }

    /// 把块内已压缩字节换算成（文件名、文件内已完成、文件总大小、总完成条目数）。
    pub(crate) fn locate(&self, done: u64) -> (String, u64, u64, usize) {
        let done = done.min(self.total);
        let index = self.spans.partition_point(|span| span.end <= done);
        let entries_done = self.entries_before + index;
        if let Some(span) = self.spans.get(index) {
            (
                span.name.clone(),
                done.saturating_sub(span.start),
                span.end.saturating_sub(span.start),
                entries_done,
            )
        } else if let Some(span) = self.spans.last() {
            let size = span.end.saturating_sub(span.start);
            (
                span.name.clone(),
                size,
                size,
                self.entries_before + self.spans.len(),
            )
        } else {
            (String::new(), 0, 0, entries_done)
        }
    }
}

/// 包装 7Z 输出文件：统计写入字节，并在每个编码块完成时上报真实进度。
///
/// 多线程 LZMA2 的块结果按顺序各写一次，因此 `write` 的调用次数就是已完成
/// 块数。只在 `active` 阶段计数，`finish()` 写文件头时不会误报。
pub(crate) struct ChunkTrackingWriter<'a, F>
where
    F: FnMut(CreateProgress),
{
    /// 底层输出文件。
    pub(crate) inner: File,
    /// 共享的编码块进度。
    pub(crate) state: Rc<RefCell<ChunkProgressState>>,
    /// 进度回调。
    pub(crate) progress: Rc<RefCell<&'a mut F>>,
}

impl<F> Write for ChunkTrackingWriter<'_, F>
where
    F: FnMut(CreateProgress),
{
    /// 写入一个编码块并推进压缩进度。
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        // 一次性写完，保证调用方的 write_all 只触发一次，和编码块一一对应。
        self.inner.write_all(buffer)?;
        let mut state = self.state.borrow_mut();
        state.output_bytes = state.output_bytes.saturating_add(buffer.len() as u64);
        // 1 字节的 LZMA2 流结束标记不是编码块，不能计入已完成块数。
        if !state.active || buffer.len() < 3 {
            return Ok(buffer.len());
        }
        state.chunks = state.chunks.saturating_add(1);
        let done = (state.chunks.saturating_mul(state.chunk_bytes)).min(state.total);
        let (current, current_done, current_total, entries_done) = state.locate(done);
        let event = CreateProgress {
            current,
            bytes_done: state.base + done,
            bytes_total: state.bytes_total,
            current_bytes_done: current_done,
            current_bytes_total: current_total,
            output_bytes: state.output_bytes,
            entries_done,
            entries_total: state.entries_total,
        };
        drop(state);
        (self.progress.borrow_mut())(event);
        Ok(buffer.len())
    }

    /// 刷新底层文件。
    /// 刷新底层写入器。
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<F> Seek for ChunkTrackingWriter<'_, F>
where
    F: FnMut(CreateProgress),
{
    /// 定位底层文件；进度状态由写路径维护。
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

/// 只负责取消检查的读取器；进度由输出写入器按完成块上报。
pub(crate) struct CancelReader {
    /// 底层输入文件。
    pub(crate) inner: File,
    /// 取消标志。
    pub(crate) cancel: Option<Arc<AtomicBool>>,
}

impl Read for CancelReader {
    /// 每次读取前检查取消标志。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(&self.cancel) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "compression cancelled",
            ));
        }
        self.inner.read(buffer)
    }
}

/// 在 7Z 编码器读取源文件时按块上报当前文件进度并响应取消。
pub(crate) struct ProgressReader<'a, F>
where
    F: FnMut(CreateProgress),
{
    /// 底层输入文件。
    pub(crate) inner: File,
    /// 当前文件名。
    pub(crate) current: String,
    /// 当前文件大小。
    pub(crate) total: u64,
    /// 当前文件已读字节。
    pub(crate) done: u64,
    /// 当前文件之前的累计字节。
    pub(crate) overall_base: u64,
    /// 全部源字节数。
    pub(crate) bytes_total: u64,
    /// 已完成条目数。
    pub(crate) entries_done: usize,
    /// 总条目数。
    pub(crate) entries_total: usize,
    /// 已写出归档字节数。
    pub(crate) output: PathBuf,
    /// 取消标志。
    pub(crate) cancel: Option<Arc<AtomicBool>>,
    /// 进度回调。
    pub(crate) progress: Rc<RefCell<&'a mut F>>,
    /// 上次上报时间，用于节流。
    pub(crate) last_report: Instant,
    /// 上一次 read 返回的数据还没上报；下次 read 时它已经被写入编码器。
    pub(crate) pending: bool,
}

impl<F> Read for ProgressReader<'_, F>
where
    F: FnMut(CreateProgress),
{
    /// 读取源数据并延迟上报已完成字节。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(&self.cancel) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "compression cancelled",
            ));
        }
        // 调用方在上一次 read 之后已经把这些字节写进编码器，这里才把
        // 它们算作已完成字节，避免进度按读取位置提前。
        if self.pending
            && self.done < self.total
            && self.last_report.elapsed() >= Duration::from_millis(100)
        {
            self.pending = false;
            self.last_report = Instant::now();
            (self.progress.borrow_mut())(CreateProgress {
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
        let read = self.inner.read(buffer)?;
        if read > 0 {
            self.done += read as u64;
            self.pending = true;
        }
        Ok(read)
    }
}

/// 固实压缩共用的进度读取器。
///
/// 固实块会先一次性提交多个文件，再统一压缩；每个读取器共享同一个进度
/// 回调和已完成条目计数，因此压缩过程中仍能按实际读取字节更新总进度。
pub(crate) struct SharedProgressReader<'a, F>
where
    F: FnMut(CreateProgress),
{
    /// 底层输入文件。
    pub(crate) inner: File,
    /// 当前文件名。
    pub(crate) current: String,
    /// 当前文件大小。
    pub(crate) total: u64,
    /// 当前文件已读字节。
    pub(crate) done: u64,
    /// 当前文件之前的累计字节。
    pub(crate) overall_base: u64,
    /// 全部源字节数。
    pub(crate) bytes_total: u64,
    /// 已完成条目数。
    pub(crate) entries_done: Rc<Cell<usize>>,
    /// 总条目数。
    pub(crate) entries_total: usize,
    /// 已写出归档字节数。
    pub(crate) output: PathBuf,
    /// 取消标志。
    pub(crate) cancel: Option<Arc<AtomicBool>>,
    /// 进度回调。
    pub(crate) progress: Rc<RefCell<&'a mut F>>,
    /// 上次上报时间，用于节流。
    pub(crate) last_report: Instant,
    /// 是否已经上报最终进度。
    pub(crate) finished: bool,
    /// 上一次 read 返回的数据还没上报；下次 read 时它已经被写入编码器。
    pub(crate) pending: bool,
}

impl<F> Read for SharedProgressReader<'_, F>
where
    F: FnMut(CreateProgress),
{
    /// 读取源数据并更新共享进度。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(&self.cancel) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "compression cancelled",
            ));
        }
        // 只有等调用方把上一批数据写进编码器后，才把它们计入已完成字节；
        // 文件末尾由 finished_now 事件一次性补齐。
        if self.pending
            && self.done < self.total
            && self.last_report.elapsed() >= Duration::from_millis(100)
        {
            self.pending = false;
            self.last_report = Instant::now();
            (self.progress.borrow_mut())(CreateProgress {
                current: self.current.clone(),
                bytes_done: self.overall_base + self.done,
                bytes_total: self.bytes_total,
                current_bytes_done: self.done,
                current_bytes_total: self.total,
                output_bytes: fs::metadata(&self.output)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                entries_done: self.entries_done.get(),
                entries_total: self.entries_total,
            });
        }
        let read = self.inner.read(buffer)?;
        if read > 0 {
            self.done += read as u64;
            self.pending = true;
        }
        let finished_now = read == 0 && !self.finished;
        if finished_now {
            self.finished = true;
            self.entries_done
                .set(self.entries_done.get().saturating_add(1));
            self.last_report = Instant::now();
            (self.progress.borrow_mut())(CreateProgress {
                current: self.current.clone(),
                bytes_done: self.overall_base + self.done,
                bytes_total: self.bytes_total,
                current_bytes_done: self.done,
                current_bytes_total: self.total,
                output_bytes: fs::metadata(&self.output)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                entries_done: self.entries_done.get(),
                entries_total: self.entries_total,
            });
        }
        Ok(read)
    }
}
/// 分块复制并响应取消。
pub(crate) fn copy_cancellable<R: Read + ?Sized, W: Write>(
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

/// 边复制边按 100 毫秒节流回调已复制字节。
///
/// `for_each_entries` 只在条目结束时给一次回调，单个大文件解压时进度条会
/// 长时间停在起点；这里按块上报，让解压进度和压缩进度一样可见。`remaining`
/// 是本次条目还能写出的字节数，超出时返回资源预算错误。
pub(crate) fn copy_entry_with_progress<R: Read + ?Sized, W: Write>(
    input: &mut R,
    output: &mut W,
    cancel: &Option<Arc<AtomicBool>>,
    remaining: u64,
    mut progress: impl FnMut(u64),
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    let mut last_report = Instant::now() - Duration::from_secs(1);
    loop {
        ensure_not_cancelled(cancel)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if total.saturating_add(read as u64) > remaining {
            return Err(ArchiveError::BudgetExceeded(format!(
                "解压数据超过剩余预算 {remaining} 字节"
            )));
        }
        output.write_all(&buffer[..read])?;
        total += read as u64;
        if last_report.elapsed() >= Duration::from_millis(100) {
            last_report = Instant::now();
            progress(total);
        }
    }
    Ok(total)
}

/// 解压预算检查：条目数和累计字节。
///
/// 条目大小来自归档元数据，可能小于实际解压量；真正的写入上限由
/// `copy_cancellable_limited` 在复制时兜底。
pub(crate) fn ensure_budget(
    options: &ExtractionOptions,
    report: &ExtractReport,
    next_bytes: u64,
) -> Result<(), ArchiveError> {
    let budget = &options.budget;
    if report.files >= budget.max_entries {
        return Err(ArchiveError::BudgetExceeded(format!(
            "条目数超过上限 {}",
            budget.max_entries
        )));
    }
    if report.bytes.saturating_add(next_bytes) > budget.max_bytes {
        return Err(ArchiveError::BudgetExceeded(format!(
            "解压总量将超过 {} 字节上限",
            budget.max_bytes
        )));
    }
    Ok(())
}

/// 在剩余预算内复制数据；超出时立即失败，避免先写满磁盘再报错。
pub(crate) fn copy_cancellable_limited<R: Read + ?Sized, W: Write>(
    input: &mut R,
    output: &mut W,
    cancel: &Option<Arc<AtomicBool>>,
    remaining: u64,
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        ensure_not_cancelled(cancel)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if total.saturating_add(read as u64) > remaining {
            return Err(ArchiveError::BudgetExceeded(format!(
                "解压数据超过剩余预算 {remaining} 字节"
            )));
        }
        output.write_all(&buffer[..read])?;
        total += read as u64;
    }
    Ok(total)
}

/// 读取并丢弃 7Z 条目数据，保证流对齐。
pub(crate) fn drain_seven_zip_entry(
    input: &mut dyn Read,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<u64, ArchiveError> {
    let mut sink = io::sink();
    copy_cancellable(input, &mut sink, cancel)
}

/// 写入前检查取消标志的包装写入器。
pub(crate) struct CancelWriter<'a> {
    /// 底层输出文件。
    pub(crate) inner: File,
    /// 取消标志。
    pub(crate) cancel: &'a Option<Arc<AtomicBool>>,
}

impl Write for CancelWriter<'_> {
    /// 写入前检查取消标志。
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if is_cancelled(self.cancel) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "task cancelled"));
        }
        self.inner.write(buffer)
    }

    /// 刷新底层写入器。
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
