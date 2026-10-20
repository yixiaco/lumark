use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    Compress,
    Extract,
    Test,
    Edit,
    LoadArchive,
    PasswordResolve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Queued,
    Running,
    Paused,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Default)]
pub struct TaskProgress {
    pub current: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub entries_done: usize,
    pub entries_total: usize,
    pub speed_bps: f64,
    pub elapsed_seconds: u64,
}

impl TaskProgress {
    pub fn ratio(&self) -> f32 {
        if self.bytes_total == 0 {
            0.0
        } else {
            (self.bytes_done as f32 / self.bytes_total as f32).clamp(0.0, 1.0)
        }
    }

    pub fn eta_seconds(&self) -> Option<u64> {
        if self.speed_bps <= 0.0 || self.bytes_total <= self.bytes_done {
            return None;
        }
        Some(((self.bytes_total - self.bytes_done) as f64 / self.speed_bps) as u64)
    }
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    Queued {
        id: u64,
        kind: TaskKind,
        label: String,
    },
    Progress {
        id: u64,
        progress: TaskProgress,
    },
    PasswordAttempt {
        id: u64,
        index: usize,
        total: usize,
    },
    Completed {
        id: u64,
        message: String,
    },
    Failed {
        id: u64,
        message: String,
    },
    Cancelled {
        id: u64,
    },
    StateChanged {
        id: u64,
        state: TaskState,
    },
}
