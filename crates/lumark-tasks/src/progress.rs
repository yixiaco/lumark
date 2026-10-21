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
    /// 当前文件已处理的字节数。
    pub current_file_done: u64,
    /// 当前文件总字节数。
    pub current_file_total: u64,
    /// 已写入压缩包输出文件的字节数。
    pub output_bytes: u64,
    pub entries_done: usize,
    pub entries_total: usize,
    pub speed_bps: f64,
    pub elapsed_seconds: u64,
}

impl TaskProgress {
    /// 返回总进度：优先按已完成文件数统计，缺少条目数时退回字节比例。
    pub fn ratio(&self) -> f32 {
        if self.entries_total > 0 {
            (self.entries_done as f32 / self.entries_total as f32).clamp(0.0, 1.0)
        } else if self.bytes_total == 0 {
            0.0
        } else {
            (self.bytes_done as f32 / self.bytes_total as f32).clamp(0.0, 1.0)
        }
    }

    /// 返回当前处理文件的完成比例。
    pub fn current_file_ratio(&self) -> f32 {
        if self.current_file_total == 0 {
            0.0
        } else {
            (self.current_file_done as f32 / self.current_file_total as f32).clamp(0.0, 1.0)
        }
    }

    /// 返回输出大小与已处理源字节的比值。
    pub fn compression_ratio(&self) -> Option<f32> {
        (self.bytes_done > 0).then(|| self.output_bytes as f32 / self.bytes_done as f32)
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

#[cfg(test)]
mod tests {
    use super::TaskProgress;

    /// 总进度按完成文件数统计，当前文件比例仍按字节统计。
    #[test]
    fn total_ratio_uses_completed_entries() {
        let progress = TaskProgress {
            current: "b.txt".to_string(),
            bytes_done: 10,
            bytes_total: 100,
            current_file_done: 5,
            current_file_total: 10,
            output_bytes: 0,
            entries_done: 1,
            entries_total: 4,
            speed_bps: 0.0,
            elapsed_seconds: 0,
        };
        assert!((progress.ratio() - 0.25).abs() < f32::EPSILON);
        assert!((progress.current_file_ratio() - 0.5).abs() < f32::EPSILON);
    }
}
