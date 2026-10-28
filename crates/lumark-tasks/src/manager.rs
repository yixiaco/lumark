//! 固定工作线程池、任务控制和事件通道。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::{Condvar, Mutex};

use lumark_archive::{
    ArchiveFormat, ArchiveInfo, ArchiveService, CompressionOptions, DisguiseFileOptions,
    EditOptions, ExtractionOptions, SevenZCompression,
};
use zeroize::Zeroizing;

use crate::progress::{TaskEvent, TaskKind, TaskProgress, TaskState};

/// 任务运行状态编码。
const RUNNING: u8 = 0;
/// 任务暂停状态编码。
const PAUSED: u8 = 1;
/// 任务取消状态编码。
const CANCELLED: u8 = 2;

/// 单个任务的暂停、取消和状态控制。
pub struct TaskControl {
    state: Arc<AtomicU8>,
    cancelled: Arc<AtomicBool>,
    pause_lock: Arc<(Mutex<bool>, Condvar)>,
}

impl TaskControl {
    /// 创建运行中的任务控制器。
    fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(RUNNING)),
            cancelled: Arc::new(AtomicBool::new(false)),
            pause_lock: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// 请求暂停任务。
    pub fn pause(&self) {
        if self.state.load(Ordering::Relaxed) == RUNNING {
            self.state.store(PAUSED, Ordering::Relaxed);
            *self.pause_lock.0.lock() = true;
        }
    }

    /// 恢复被暂停的任务。
    pub fn resume(&self) {
        if self.state.load(Ordering::Relaxed) == PAUSED {
            self.state.store(RUNNING, Ordering::Relaxed);
            *self.pause_lock.0.lock() = false;
            self.pause_lock.1.notify_all();
        }
    }

    /// 请求取消任务。
    pub fn cancel(&self) {
        self.state.store(CANCELLED, Ordering::Relaxed);
        self.cancelled.store(true, Ordering::Relaxed);
        *self.pause_lock.0.lock() = false;
        self.pause_lock.1.notify_all();
    }

    /// 返回任务状态编码。
    pub fn state(&self) -> TaskState {
        match self.state.load(Ordering::Relaxed) {
            PAUSED => TaskState::Paused,
            CANCELLED => TaskState::Cancelled,
            _ => TaskState::Running,
        }
    }

    /// 判断任务是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// 返回供归档后端轮询的取消标志。
    pub fn cancelled_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    /// 暂停期间阻塞工作线程，取消时立即返回。
    pub fn wait_if_paused(&self) {
        let (lock, condvar) = &*self.pause_lock;
        let mut paused = lock.lock();
        while *paused && !self.is_cancelled() {
            condvar.wait(&mut paused);
        }
    }
}

/// 后台任务请求。
pub enum TaskRequest {
    /// 压缩请求。
    Compress {
        output: PathBuf,
        sources: Vec<PathBuf>,
        options: CompressionOptions,
        /// 自解压运行时模块路径；`options.sfx` 为真时必填。
        sfx_stub: Option<PathBuf>,
    },
    /// 解压请求。
    Extract {
        archive: PathBuf,
        options: ExtractionOptions,
    },
    /// 测试请求。
    Test {
        archive: PathBuf,
        password: Option<String>,
    },
    /// 密码候选验证请求。
    ResolvePassword {
        archive: PathBuf,
        candidates: Vec<Zeroizing<String>>,
        load_info: bool,
    },
    /// 归档编辑请求。
    Edit {
        archive: PathBuf,
        options: EditOptions,
    },
    /// 后台加载归档请求。
    LoadArchive {
        archive: PathBuf,
        password: Option<Zeroizing<String>>,
    },
    /// 把普通文件批量伪装成媒体外观。
    Disguise {
        sources: Vec<PathBuf>,
        options: DisguiseFileOptions,
    },
    /// 把伪装文件还原成原归档或原文件。
    Restore {
        archive: PathBuf,
        output_dir: PathBuf,
    },
}

/// 密码候选验证结果。
pub struct ResolvedPassword {
    /// 匹配到的候选序号。
    pub candidate_index: usize,
    /// 匹配到的密码。
    pub password: Zeroizing<String>,
}

/// 后台归档加载结果。
pub struct ArchiveLoadResult {
    /// 加载到的归档信息。
    pub info: ArchiveInfo,
    /// 加载使用的密码。
    pub password: Option<Zeroizing<String>>,
}

/// 固定工作线程池和任务事件通道。
pub struct TaskManager {
    sender: Sender<TaskEvent>,
    receiver: Receiver<TaskEvent>,
    controls: Mutex<std::collections::HashMap<u64, Arc<TaskControl>>>,
    password_results: Arc<Mutex<std::collections::HashMap<u64, ResolvedPassword>>>,
    archive_results: Arc<Mutex<std::collections::HashMap<u64, ArchiveLoadResult>>>,
    /// 还原任务生成的文件路径，供界面在完成后定位结果。
    restore_results: Arc<Mutex<std::collections::HashMap<u64, PathBuf>>>,
    next_id: Mutex<u64>,
    /// 工作线程共享的排队队列。
    pool: Arc<PoolState>,
}

/// 排队中的任务；轮到工作线程时才会真正开始。
struct PoolJob {
    id: u64,
    request: TaskRequest,
    control: Arc<TaskControl>,
}

/// 工作线程共享的任务队列和关闭标记。
///
/// 队列是先进先出的；`closed` 由 `TaskManager` 析构时置位，工作线程在
/// 当前任务结束后退出，避免测试里反复创建管理器导致线程泄漏。
struct PoolState {
    queue: Mutex<VecDeque<PoolJob>>,
    wake: Condvar,
    closed: AtomicBool,
}

impl PoolState {
    /// 追加任务并唤醒一个空闲工作线程。
    fn push(&self, job: PoolJob) {
        self.queue.lock().push_back(job);
        self.wake.notify_one();
    }

    /// 取出下一个任务；管理器关闭后返回 `None` 让工作线程退出。
    fn pop(&self) -> Option<PoolJob> {
        let mut queue = self.queue.lock();
        loop {
            if self.closed.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(job) = queue.pop_front() {
                return Some(job);
            }
            self.wake.wait(&mut queue);
        }
    }
}

/// 默认工作线程数。
///
/// 压缩本身已经按线程数并行，再让多个大任务同时跑会互相争抢 CPU 和磁盘；
/// 这里按核心数的一半限制并发任务，最多 4 个。
fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| (count.get() / 2).clamp(1, 4))
        .unwrap_or(2)
}

impl TaskManager {
    /// 创建运行中的任务控制器。
    pub fn new() -> Self {
        Self::with_workers(default_workers())
    }

    /// 创建带指定工作线程数的管理器，供测试验证排队和并发行为。
    pub fn with_workers(workers: usize) -> Self {
        let (sender, receiver) = unbounded();
        let pool = Arc::new(PoolState {
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            closed: AtomicBool::new(false),
        });
        let password_results = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let archive_results = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let restore_results = Arc::new(Mutex::new(std::collections::HashMap::new()));
        for _ in 0..workers.max(1) {
            let pool = pool.clone();
            let sender = sender.clone();
            let password_results = password_results.clone();
            let archive_results = archive_results.clone();
            let restore_results = restore_results.clone();
            thread::spawn(move || {
                worker_loop(
                    pool,
                    sender,
                    password_results,
                    archive_results,
                    restore_results,
                )
            });
        }
        Self {
            sender,
            receiver,
            controls: Mutex::new(Default::default()),
            password_results,
            archive_results,
            restore_results,
            next_id: Mutex::new(1),
            pool,
        }
    }

    /// 非阻塞读取一个任务事件。
    pub fn try_recv(&self) -> Option<TaskEvent> {
        self.receiver.try_recv().ok()
    }

    /// 返回指定任务的控制器。
    pub fn control(&self, id: u64) -> Option<Arc<TaskControl>> {
        self.controls.lock().get(&id).cloned()
    }

    /// 取出密码验证结果。
    pub fn take_password_result(&self, id: u64) -> Option<ResolvedPassword> {
        self.password_results.lock().remove(&id)
    }

    /// 取出归档加载结果。
    pub fn take_archive_load_result(&self, id: u64) -> Option<ArchiveLoadResult> {
        self.archive_results.lock().remove(&id)
    }

    /// 取出还原任务生成的文件路径。
    pub fn take_restore_result(&self, id: u64) -> Option<PathBuf> {
        self.restore_results.lock().remove(&id)
    }

    /// 提交任务并返回任务 ID。
    pub fn submit(&self, request: TaskRequest) -> u64 {
        let id = {
            let mut next = self.next_id.lock();
            let id = *next;
            *next += 1;
            id
        };
        let control = Arc::new(TaskControl::new());
        self.controls.lock().insert(id, control.clone());
        let (kind, label) = match &request {
            TaskRequest::Compress {
                output, options, ..
            } => {
                let action = if options.disguise.is_some() {
                    "伪装压缩"
                } else {
                    "压缩"
                };
                (TaskKind::Compress, format!("{action} {}", output.display()))
            }
            TaskRequest::Extract { archive, .. } => {
                (TaskKind::Extract, format!("解压 {}", archive.display()))
            }
            TaskRequest::Test { archive, .. } => {
                (TaskKind::Test, format!("测试 {}", archive.display()))
            }
            TaskRequest::ResolvePassword {
                archive,
                candidates,
                ..
            } => (
                TaskKind::PasswordResolve,
                format!(
                    "验证 {} 的 {} 条记录密码",
                    archive.display(),
                    candidates.len()
                ),
            ),
            TaskRequest::Edit { archive, .. } => {
                (TaskKind::Edit, format!("更新 {}", archive.display()))
            }
            TaskRequest::LoadArchive { archive, .. } => {
                (TaskKind::LoadArchive, format!("打开 {}", archive.display()))
            }
            TaskRequest::Disguise { sources, .. } => {
                (TaskKind::Disguise, format!("伪装 {} 个文件", sources.len()))
            }
            TaskRequest::Restore { archive, .. } => {
                (TaskKind::Restore, format!("还原 {}", archive.display()))
            }
        };
        let _ = self.sender.send(TaskEvent::Queued { id, kind, label });
        self.pool.push(PoolJob {
            id,
            request,
            control,
        });
        id
    }
}

/// 记录真实进度事件的节奏，供界面在编码块之间做平滑估算。
///
/// 7Z 多线程压缩每 16 MiB 才产生一次真实事件；界面按最近一次瞬时速度
/// 在两次事件之间推进显示值，块完成后再用真实字节校正。
struct ProgressPacing {
    last_at: Instant,
    last_bytes: u64,
    recent_speed_bps: f64,
    chunk_bytes: u64,
}

impl ProgressPacing {
    /// 从任务开始时建立采样器；`initial_chunk_bytes` 是已知的首块大小。
    fn new(initial_chunk_bytes: u64) -> Self {
        Self {
            last_at: Instant::now(),
            last_bytes: 0,
            recent_speed_bps: 0.0,
            chunk_bytes: initial_chunk_bytes,
        }
    }

    /// 采样一次真实进度，返回平滑速度和观察到的最大块增量。
    fn sample(&mut self, bytes_done: u64) -> (f64, u64) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_at).as_secs_f64();
        let delta = bytes_done.saturating_sub(self.last_bytes);
        if delta > 0 && elapsed > 0.001 {
            let instant = delta as f64 / elapsed;
            self.recent_speed_bps = if self.recent_speed_bps > 0.0 {
                self.recent_speed_bps * 0.5 + instant * 0.5
            } else {
                instant
            };
            self.chunk_bytes = self.chunk_bytes.max(delta);
        }
        self.last_at = now;
        self.last_bytes = bytes_done;
        (self.recent_speed_bps, self.chunk_bytes)
    }
}

/// 在池工作线程里执行一个任务，并发送进度和最终状态事件。
fn run_job(
    id: u64,
    request: TaskRequest,
    control: Arc<TaskControl>,
    sender: &Sender<TaskEvent>,
    password_results: &Mutex<std::collections::HashMap<u64, ResolvedPassword>>,
    archive_results: &Mutex<std::collections::HashMap<u64, ArchiveLoadResult>>,
    restore_results: &Mutex<std::collections::HashMap<u64, PathBuf>>,
) {
    let started = Instant::now();
    let _ = sender.send(TaskEvent::StateChanged {
        id,
        state: TaskState::Running,
    });
    // 任务线程里的 panic 只会终止这条线程，事件通道收不到任何结果，
    // 进度窗口会永远停在最后一帧；这里统一捕获并转成失败事件。
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request {
        TaskRequest::Compress {
            output,
            sources,
            mut options,
            sfx_stub,
        } => {
            options.cancel = Some(control.cancelled_flag());
            // 固实 7Z 多线程压缩在首块完成前没有真实事件；先按 16 MiB
            // 块大小给界面一个可推进的显示上限，首块完成后再用真实增量校正。
            let initial_chunk = if options.format == ArchiveFormat::SevenZip
                && options.solid
                && options.password.is_none()
                && options.threads > 1
                && matches!(options.sevenz_compression, SevenZCompression::Lzma2)
            {
                (16 * 1024 * 1024).max(options.dictionary_size.map(u64::from).unwrap_or(0))
            } else {
                0
            };
            let mut pacing = ProgressPacing::new(initial_chunk);
            let progress = |progress: lumark_archive::CreateProgress| {
                control.wait_if_paused();
                if control.is_cancelled() {
                    return;
                }
                let (recent_speed_bps, chunk_bytes) = pacing.sample(progress.bytes_done);
                let _ = sender.send(TaskEvent::Progress {
                    id,
                    progress: TaskProgress {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: progress.bytes_total,
                        current_file_done: progress.current_bytes_done,
                        current_file_total: progress.current_bytes_total,
                        output_bytes: progress.output_bytes,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps: speed(progress.bytes_done, started.elapsed().as_secs_f64()),
                        elapsed_seconds: started.elapsed().as_secs(),
                        display_bytes: progress.bytes_done,
                        updated_at: Some(Instant::now()),
                        recent_speed_bps,
                        chunk_bytes,
                    },
                });
            };
            // 自解压需要额外的运行时模块；缺失时直接失败，避免生成
            // 只有 ZIP 却没有解压入口的半成品。
            let created = if options.sfx {
                match sfx_stub {
                    Some(stub) => ArchiveService::create_sfx(
                        &output, &sources, &options, &stub, progress,
                    ),
                    None => Err(lumark_archive::ArchiveError::Corrupt(
                        "找不到自解压运行时模块 lumark-sfx.exe，请先构建 lumark-sfx 或重新安装 Lumark"
                            .to_string(),
                    )),
                }
            } else {
                ArchiveService::create(&output, &sources, &options, progress)
            };
            created.map(|report| {
                let mut message = if options.sfx {
                    format!("已创建自解压 {}", report.output.display())
                } else if options.disguise.is_some() {
                    format!("已创建伪装压缩文件 {}", report.output.display())
                } else {
                    format!("已创建 {}", report.output.display())
                };
                if !report.cleanup_errors.is_empty() {
                    message.push_str(&format!(
                        "；{} 个源文件未能删除",
                        report.cleanup_errors.len()
                    ));
                }
                message
            })
        }
        TaskRequest::Extract {
            archive,
            mut options,
        } => {
            options.cancel = Some(control.cancelled_flag());
            ArchiveService::extract(&archive, &options, |progress| {
                control.wait_if_paused();
                if control.is_cancelled() {
                    return;
                }
                let _ = sender.send(TaskEvent::Progress {
                    id,
                    progress: TaskProgress {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: progress.bytes_total,
                        current_file_done: 0,
                        current_file_total: 0,
                        output_bytes: 0,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps: speed(progress.bytes_done, started.elapsed().as_secs_f64()),
                        elapsed_seconds: started.elapsed().as_secs(),
                        display_bytes: progress.bytes_done,
                        updated_at: Some(Instant::now()),
                        recent_speed_bps: 0.0,
                        chunk_bytes: 0,
                    },
                });
            })
            .map(|report| format!("已解压 {} 个文件", report.files))
        }
        TaskRequest::Test { archive, password } => ArchiveService::test_cancellable(
            &archive,
            password.as_deref(),
            Some(control.cancelled_flag()),
            |progress| {
                control.wait_if_paused();
                if control.is_cancelled() {
                    return;
                }
                let _ = sender.send(TaskEvent::Progress {
                    id,
                    progress: TaskProgress {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: progress.bytes_total,
                        current_file_done: 0,
                        current_file_total: 0,
                        output_bytes: 0,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps: speed(progress.bytes_done, started.elapsed().as_secs_f64()),
                        elapsed_seconds: started.elapsed().as_secs(),
                        display_bytes: progress.bytes_done,
                        updated_at: Some(Instant::now()),
                        recent_speed_bps: 0.0,
                        chunk_bytes: 0,
                    },
                });
            },
        )
        .map(|report| format!("测试通过，共 {} 个文件", report.files)),
        TaskRequest::ResolvePassword {
            archive,
            candidates,
            load_info,
        } => {
            let total = candidates.len();
            let mut matched = None;
            for (index, password) in candidates.into_iter().enumerate() {
                control.wait_if_paused();
                if control.is_cancelled() {
                    break;
                }
                let _ = sender.send(TaskEvent::PasswordAttempt {
                    id,
                    index: index + 1,
                    total,
                });
                if ArchiveService::verify_password(&archive, password.as_str()).unwrap_or(false) {
                    matched = Some((index, password));
                    break;
                }
            }
            match matched {
                Some((candidate_index, password)) => {
                    let info_result = if load_info {
                        ArchiveService::list(&archive, Some(password.as_str())).map(Some)
                    } else {
                        Ok(None)
                    };
                    match info_result {
                        Ok(info) => {
                            password_results.lock().insert(
                                id,
                                ResolvedPassword {
                                    candidate_index,
                                    password,
                                },
                            );
                            if let Some(info) = info {
                                archive_results.lock().insert(
                                    id,
                                    ArchiveLoadResult {
                                        info,
                                        password: None,
                                    },
                                );
                            }
                            Ok(format!("已从 {total} 条记录密码中找到可用密码"))
                        }
                        Err(error) => Err(error),
                    }
                }
                None if control.is_cancelled() => Err(lumark_archive::ArchiveError::Cancelled),
                None => Ok("记录密码均不正确".to_string()),
            }
        }
        TaskRequest::Edit {
            archive,
            mut options,
        } => {
            options.cancel = Some(control.cancelled_flag());
            ArchiveService::edit(&archive, &options, |progress| {
                control.wait_if_paused();
                if control.is_cancelled() {
                    return;
                }
                let _ = sender.send(TaskEvent::Progress {
                    id,
                    progress: TaskProgress {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: 0,
                        current_file_done: 0,
                        current_file_total: 0,
                        output_bytes: 0,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps: speed(progress.bytes_done, started.elapsed().as_secs_f64()),
                        elapsed_seconds: started.elapsed().as_secs(),
                        display_bytes: progress.bytes_done,
                        updated_at: Some(Instant::now()),
                        recent_speed_bps: 0.0,
                        chunk_bytes: 0,
                    },
                });
            })
            .map(|report| {
                let mut message = format!("已更新 {}", report.output.display());
                if report.deleted > 0 {
                    message.push_str(&format!("，删除 {} 个条目", report.deleted));
                }
                if report.skipped > 0 {
                    message.push_str(&format!("，跳过 {} 个未变化文件", report.skipped));
                }
                if report.reused_blocks > 0 {
                    message.push_str(&format!("，复用 {} 个原始压缩块", report.reused_blocks));
                }
                message
            })
        }
        TaskRequest::LoadArchive { archive, password } => {
            ArchiveService::list(&archive, password.as_deref().map(String::as_str)).map(|info| {
                archive_results
                    .lock()
                    .insert(id, ArchiveLoadResult { info, password });
                format!("已加载 {}", archive.display())
            })
        }
        TaskRequest::Disguise {
            sources,
            mut options,
        } => {
            options.cancel = Some(control.cancelled_flag());
            ArchiveService::disguise_files(&sources, &options, |progress| {
                control.wait_if_paused();
                if control.is_cancelled() {
                    return;
                }
                let _ = sender.send(TaskEvent::Progress {
                    id,
                    progress: TaskProgress {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: progress.bytes_total,
                        current_file_done: progress.current_bytes_done,
                        current_file_total: progress.current_bytes_total,
                        output_bytes: progress.output_bytes,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps: speed(progress.bytes_done, started.elapsed().as_secs_f64()),
                        elapsed_seconds: started.elapsed().as_secs(),
                        display_bytes: progress.bytes_done,
                        updated_at: Some(Instant::now()),
                        recent_speed_bps: 0.0,
                        chunk_bytes: 0,
                    },
                });
            })
            .map(|report| {
                let mut message = format!("已伪装 {} 个文件", report.files);
                if !report.cleanup_errors.is_empty() {
                    message.push_str(&format!(
                        "；{} 个源文件未能删除",
                        report.cleanup_errors.len()
                    ));
                }
                message
            })
        }
        TaskRequest::Restore {
            archive,
            output_dir,
        } => {
            let cancel = control.cancelled_flag();
            ArchiveService::restore_disguise(&archive, &output_dir, Some(&cancel), |progress| {
                control.wait_if_paused();
                if control.is_cancelled() {
                    return;
                }
                let _ = sender.send(TaskEvent::Progress {
                    id,
                    progress: TaskProgress {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: progress.bytes_total,
                        current_file_done: 0,
                        current_file_total: 0,
                        output_bytes: 0,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps: speed(progress.bytes_done, started.elapsed().as_secs_f64()),
                        elapsed_seconds: started.elapsed().as_secs(),
                        display_bytes: progress.bytes_done,
                        updated_at: Some(Instant::now()),
                        recent_speed_bps: 0.0,
                        chunk_bytes: 0,
                    },
                });
            })
            .map(|report| {
                restore_results.lock().insert(id, report.output.clone());
                format!("已还原为 {}", report.output.display())
            })
        }
    }));
    let result = match result {
        Ok(result) => result,
        Err(payload) => Err(lumark_archive::ArchiveError::Panicked(panic_message(
            payload.as_ref(),
        ))),
    };
    match result {
        Ok(message) => {
            let _ = sender.send(TaskEvent::StateChanged {
                id,
                state: TaskState::Completed,
            });
            let _ = sender.send(TaskEvent::Completed { id, message });
        }
        Err(_error) if control.is_cancelled() => {
            let _ = sender.send(TaskEvent::StateChanged {
                id,
                state: TaskState::Cancelled,
            });
            let _ = sender.send(TaskEvent::Cancelled { id });
        }
        Err(error) => {
            let _ = sender.send(TaskEvent::StateChanged {
                id,
                state: TaskState::Failed,
            });
            let _ = sender.send(TaskEvent::Failed {
                id,
                message: error.to_string(),
            });
        }
    }
}

/// 工作线程：从队列取任务执行，直到管理器关闭。
fn worker_loop(
    pool: Arc<PoolState>,
    sender: Sender<TaskEvent>,
    password_results: Arc<Mutex<std::collections::HashMap<u64, ResolvedPassword>>>,
    archive_results: Arc<Mutex<std::collections::HashMap<u64, ArchiveLoadResult>>>,
    restore_results: Arc<Mutex<std::collections::HashMap<u64, PathBuf>>>,
) {
    while let Some(job) = pool.pop() {
        // 排队期间被取消的任务不启动，直接回传取消事件。
        if job.control.is_cancelled() {
            send_cancelled(&sender, job.id);
            continue;
        }
        // 这里不再等待暂停：给排队任务点暂停时，工作线程如果阻塞在队列里，
        // 后续任务会被一起卡住；任务开始后的进度回调会在下一次检查时停住。
        run_job(
            job.id,
            job.request,
            job.control,
            &sender,
            &password_results,
            &archive_results,
            &restore_results,
        );
    }
}

/// 回传排队任务的取消事件，避免界面一直停在“排队中”。
fn send_cancelled(sender: &Sender<TaskEvent>, id: u64) {
    let _ = sender.send(TaskEvent::StateChanged {
        id,
        state: TaskState::Cancelled,
    });
    let _ = sender.send(TaskEvent::Cancelled { id });
}

impl Drop for TaskManager {
    /// 通知工作线程退出；正在执行的任务会先跑完，避免留下半成品文件。
    fn drop(&mut self) {
        self.pool.closed.store(true, Ordering::Relaxed);
        self.pool.wake.notify_all();
    }
}

impl Default for TaskManager {
    /// 创建默认任务管理器。
    fn default() -> Self {
        Self::new()
    }
}

/// 按字节数和耗时计算平均速度。
fn speed(bytes: u64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / seconds
    }
}

/// 把 panic 载荷还原成可读文本，供失败事件显示。
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "任务线程异常退出".to_string()
}
