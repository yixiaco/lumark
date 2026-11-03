//! 自解压解压线程：持有取消标记，并把进度和结果交给 UI 线程。
//!
//! 解压回调运行在后台线程，只做节流和 channel 发送；GPUI 实体只在
//! UI 线程的轮询循环里更新，避免跨线程持有 `Context`。

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use lumark_sfx::{
    SfxError, SfxExtractOptions, SfxFooter, SfxProgress, extract_payload_with_options,
};
use zeroize::Zeroizing;

/// 解压线程发送给界面的结果。
pub(crate) enum ExtractionEvent {
    /// 最新进度、瞬时速度和已运行秒数。
    Progress {
        /// 解压内核的进度快照。
        progress: SfxProgress,
        /// 平滑后的处理速度，单位字节/秒。
        speed_bps: f64,
        /// 从开始到当前事件已运行的秒数。
        elapsed_seconds: u64,
    },
    /// 解压完成并返回写出的文件数。
    Completed {
        /// 成功写出的文件数。
        files: usize,
    },
    /// 解压失败。
    Failed {
        /// 解压内核返回的错误。
        error: SfxError,
    },
    /// 用户取消了解压。
    Cancelled,
}

/// 一次正在运行的自解压任务。
pub(crate) struct ExtractionTask {
    /// 供界面置位的取消标记。
    pub(crate) cancel: Arc<AtomicBool>,
    /// 后台线程发送的事件。
    pub(crate) events: Receiver<ExtractionEvent>,
}

/// 启动解压线程；线程启动失败时返回可展示的错误文本。
pub(crate) fn spawn(
    exe: PathBuf,
    footer: SfxFooter,
    target: PathBuf,
    password: Option<Zeroizing<String>>,
    overwrite: bool,
) -> Result<ExtractionTask, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (sender, events) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumark-sfx-extract".to_string())
        .spawn(move || {
            run(
                sender,
                exe,
                footer,
                target,
                password,
                overwrite,
                worker_cancel,
            );
        })
        .map_err(|error| format!("无法启动解压线程：{error}"))?;
    Ok(ExtractionTask { cancel, events })
}

/// 在后台线程执行解压，并把进度和最终结果发送回界面。
fn run(
    sender: mpsc::Sender<ExtractionEvent>,
    exe: PathBuf,
    footer: SfxFooter,
    target: PathBuf,
    password: Option<Zeroizing<String>>,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
) {
    let file = match File::open(&exe) {
        Ok(file) => file,
        Err(error) => {
            let _ = sender.send(ExtractionEvent::Failed {
                error: SfxError::Other(format!("无法打开自身文件：{error}")),
            });
            return;
        }
    };
    let started = Instant::now();
    let mut pacer = ProgressPacer::new(started);
    let mut last_sent = started;
    let mut progress = |progress: SfxProgress| {
        let (speed_bps, elapsed_seconds) = pacer.sample(progress.bytes_done);
        let complete = progress.bytes_total > 0 && progress.bytes_done >= progress.bytes_total;
        let now = Instant::now();
        if now.duration_since(last_sent) >= Duration::from_millis(80) || complete {
            last_sent = now;
            let _ = sender.send(ExtractionEvent::Progress {
                progress,
                speed_bps,
                elapsed_seconds,
            });
        }
    };
    let options = SfxExtractOptions {
        password: password.as_ref().map(|value| value.as_str()),
        overwrite,
        cancel: Some(cancel.as_ref()),
        progress: Some(&mut progress),
    };
    match extract_payload_with_options(file, footer, &target, options) {
        Ok(files) => {
            let _ = sender.send(ExtractionEvent::Completed { files });
        }
        Err(SfxError::Cancelled) => {
            let _ = sender.send(ExtractionEvent::Cancelled);
        }
        Err(_) if cancel.load(Ordering::SeqCst) => {
            let _ = sender.send(ExtractionEvent::Cancelled);
        }
        Err(error) => {
            let _ = sender.send(ExtractionEvent::Failed { error });
        }
    }
}

/// 根据相邻两次进度事件估算速度，避免 UI 线程重复做时间采样。
struct ProgressPacer {
    started: Instant,
    last_at: Instant,
    last_bytes: u64,
    speed_bps: f64,
}

impl ProgressPacer {
    /// 从解压开始时间建立采样器。
    fn new(started: Instant) -> Self {
        Self {
            started,
            last_at: started,
            last_bytes: 0,
            speed_bps: 0.0,
        }
    }

    /// 采样一次进度，返回平滑速度和已运行秒数。
    fn sample(&mut self, bytes: u64) -> (f64, u64) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_at).as_secs_f64();
        let delta = bytes.saturating_sub(self.last_bytes);
        if delta > 0 && elapsed > 0.001 {
            let instant = delta as f64 / elapsed;
            self.speed_bps = if self.speed_bps > 0.0 {
                self.speed_bps * 0.5 + instant * 0.5
            } else {
                instant
            };
        }
        self.last_at = now;
        self.last_bytes = bytes;
        (
            self.speed_bps,
            now.saturating_duration_since(self.started).as_secs(),
        )
    }
}
