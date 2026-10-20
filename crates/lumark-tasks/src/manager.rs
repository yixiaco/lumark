use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::{Condvar, Mutex};

use lumark_archive::{
    ArchiveInfo, ArchiveService, CompressionOptions, EditOptions, ExtractionOptions,
};
use zeroize::Zeroizing;

use crate::progress::{TaskEvent, TaskKind, TaskProgress, TaskState};

const RUNNING: u8 = 0;
const PAUSED: u8 = 1;
const CANCELLED: u8 = 2;

pub struct TaskControl {
    state: Arc<AtomicU8>,
    cancelled: Arc<AtomicBool>,
    pause_lock: Arc<(Mutex<bool>, Condvar)>,
}

impl TaskControl {
    fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(RUNNING)),
            cancelled: Arc::new(AtomicBool::new(false)),
            pause_lock: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub fn pause(&self) {
        if self.state.load(Ordering::Relaxed) == RUNNING {
            self.state.store(PAUSED, Ordering::Relaxed);
            *self.pause_lock.0.lock() = true;
        }
    }

    pub fn resume(&self) {
        if self.state.load(Ordering::Relaxed) == PAUSED {
            self.state.store(RUNNING, Ordering::Relaxed);
            *self.pause_lock.0.lock() = false;
            self.pause_lock.1.notify_all();
        }
    }

    pub fn cancel(&self) {
        self.state.store(CANCELLED, Ordering::Relaxed);
        self.cancelled.store(true, Ordering::Relaxed);
        *self.pause_lock.0.lock() = false;
        self.pause_lock.1.notify_all();
    }

    pub fn state(&self) -> TaskState {
        match self.state.load(Ordering::Relaxed) {
            PAUSED => TaskState::Paused,
            CANCELLED => TaskState::Cancelled,
            _ => TaskState::Running,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancelled_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    pub fn wait_if_paused(&self) {
        let (lock, condvar) = &*self.pause_lock;
        let mut paused = lock.lock();
        while *paused && !self.is_cancelled() {
            condvar.wait(&mut paused);
        }
    }
}

pub enum TaskRequest {
    Compress {
        output: PathBuf,
        sources: Vec<PathBuf>,
        options: CompressionOptions,
    },
    Extract {
        archive: PathBuf,
        options: ExtractionOptions,
    },
    Test {
        archive: PathBuf,
        password: Option<String>,
    },
    ResolvePassword {
        archive: PathBuf,
        candidates: Vec<Zeroizing<String>>,
        load_info: bool,
    },
    Edit {
        archive: PathBuf,
        options: EditOptions,
    },
    LoadArchive {
        archive: PathBuf,
        password: Option<Zeroizing<String>>,
    },
}

pub struct ResolvedPassword {
    pub candidate_index: usize,
    pub password: Zeroizing<String>,
}

pub struct ArchiveLoadResult {
    pub info: ArchiveInfo,
    pub password: Option<Zeroizing<String>>,
}

pub struct TaskManager {
    sender: Sender<TaskEvent>,
    receiver: Receiver<TaskEvent>,
    controls: Mutex<std::collections::HashMap<u64, Arc<TaskControl>>>,
    password_results: Arc<Mutex<std::collections::HashMap<u64, ResolvedPassword>>>,
    archive_results: Arc<Mutex<std::collections::HashMap<u64, ArchiveLoadResult>>>,
    next_id: Mutex<u64>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self {
            sender,
            receiver,
            controls: Mutex::new(Default::default()),
            password_results: Arc::new(Mutex::new(Default::default())),
            archive_results: Arc::new(Mutex::new(Default::default())),
            next_id: Mutex::new(1),
        }
    }

    pub fn try_recv(&self) -> Option<TaskEvent> {
        self.receiver.try_recv().ok()
    }

    pub fn control(&self, id: u64) -> Option<Arc<TaskControl>> {
        self.controls.lock().get(&id).cloned()
    }

    pub fn take_password_result(&self, id: u64) -> Option<ResolvedPassword> {
        self.password_results.lock().remove(&id)
    }

    pub fn take_archive_load_result(&self, id: u64) -> Option<ArchiveLoadResult> {
        self.archive_results.lock().remove(&id)
    }

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
            TaskRequest::Compress { output, .. } => {
                (TaskKind::Compress, format!("压缩 {}", output.display()))
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
        };
        let _ = self.sender.send(TaskEvent::Queued { id, kind, label });
        let sender = self.sender.clone();
        let password_results = self.password_results.clone();
        let archive_results = self.archive_results.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let _ = sender.send(TaskEvent::StateChanged {
                id,
                state: TaskState::Running,
            });
            let result = match request {
                TaskRequest::Compress {
                    output,
                    sources,
                    mut options,
                } => {
                    options.cancel = Some(control.cancelled_flag());
                    ArchiveService::create(&output, &sources, &options, |progress| {
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
                                speed_bps: speed(
                                    progress.bytes_done,
                                    started.elapsed().as_secs_f64(),
                                ),
                                elapsed_seconds: started.elapsed().as_secs(),
                            },
                        });
                    })
                    .map(|report| {
                        let mut message = format!("已创建 {}", report.output.display());
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
                                speed_bps: speed(
                                    progress.bytes_done,
                                    started.elapsed().as_secs_f64(),
                                ),
                                elapsed_seconds: started.elapsed().as_secs(),
                            },
                        });
                    })
                    .map(|report| format!("已解压 {} 个文件", report.files))
                }
                TaskRequest::Test { archive, password } => {
                    ArchiveService::test(&archive, password.as_deref(), |progress| {
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
                                speed_bps: speed(
                                    progress.bytes_done,
                                    started.elapsed().as_secs_f64(),
                                ),
                                elapsed_seconds: started.elapsed().as_secs(),
                            },
                        });
                    })
                    .map(|report| format!("测试通过，共 {} 个文件", report.files))
                }
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
                        if ArchiveService::verify_password(&archive, password.as_str())
                            .unwrap_or(false)
                        {
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
                        None if control.is_cancelled() => {
                            Err(lumark_archive::ArchiveError::Cancelled)
                        }
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
                                speed_bps: speed(
                                    progress.bytes_done,
                                    started.elapsed().as_secs_f64(),
                                ),
                                elapsed_seconds: started.elapsed().as_secs(),
                            },
                        });
                    })
                    .map(|report| format!("已更新 {}", report.output.display()))
                }
                TaskRequest::LoadArchive { archive, password } => {
                    ArchiveService::list(&archive, password.as_deref().map(String::as_str)).map(
                        |info| {
                            archive_results
                                .lock()
                                .insert(id, ArchiveLoadResult { info, password });
                            format!("已加载 {}", archive.display())
                        },
                    )
                }
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
        });
        id
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

fn speed(bytes: u64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / seconds
    }
}
