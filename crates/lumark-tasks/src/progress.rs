use std::time::Instant;

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
    /// 界面平滑显示用的估算字节数；真实完成字节仍是 `bytes_done`。
    pub display_bytes: u64,
    /// 最近一次真实进度事件的时间，用于块内平滑。
    pub updated_at: Option<Instant>,
    /// 最近一次真实事件的瞬时速度。
    pub recent_speed_bps: f64,
    /// 观察到的编码块字节数；0 表示不做块内平滑。
    pub chunk_bytes: u64,
}

impl TaskProgress {
    /// 在两次真实进度事件之间按最近速度推进显示值。
    ///
    /// 只领先 `chunk_bytes` 一个块，且完成前最多到 `bytes_total - 1`，
    /// 避免估算把进度提前报满；真实 `bytes_done` 不受影响。
    pub fn advance_display(&mut self, now: Instant) {
        if self.bytes_total == 0 || self.chunk_bytes == 0 || self.bytes_total <= self.chunk_bytes {
            self.display_bytes = self.bytes_done;
            return;
        }
        let elapsed = self
            .updated_at
            .map(|updated| now.saturating_duration_since(updated).as_secs_f64())
            .unwrap_or(0.0);
        let speed = if self.recent_speed_bps > 0.0 {
            self.recent_speed_bps
        } else {
            // 首块完成前没有实测速度，用 1 MiB/s 的保守值先让界面动起来；
            // 首块完成后立即换成真实速度。
            1024.0 * 1024.0
        };
        let estimate = self.bytes_done as f64 + speed * elapsed * 0.8;
        let cap = self.bytes_done.saturating_add(self.chunk_bytes);
        let limit = self.bytes_total.saturating_sub(1);
        let target = (estimate as u64).min(cap).min(limit);
        self.display_bytes = self.display_bytes.max(self.bytes_done).max(target);
    }
}

impl TaskProgress {
    /// 返回总进度：优先按字节统计，缺少字节总量时退回已完成文件数。
    ///
    /// 有块内平滑值时使用 `display_bytes`，让进度条在两次编码块事件之间
    /// 也能连续推进；真实完成字节仍以 `bytes_done` 为准。
    pub fn ratio(&self) -> f32 {
        if self.bytes_total > 0 {
            (self.display_bytes.max(self.bytes_done) as f32 / self.bytes_total as f32)
                .clamp(0.0, 1.0)
        } else if self.entries_total > 0 {
            (self.entries_done as f32 / self.entries_total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// 返回当前处理文件的完成比例。
    ///
    /// 把总进度的平滑增量借给当前文件，避免当前文件条在块内长时间不动；
    /// 增量最多补到文件末尾。
    pub fn current_file_ratio(&self) -> f32 {
        if self.current_file_total == 0 {
            return 0.0;
        }
        // 只有当前文件本身跨过至少一个编码块时才借用总进度的平滑增量；
        // 小文件直接显示真实比例，避免被估算值瞬间补到 100%。
        let extra = if self.chunk_bytes > 0 && self.current_file_total >= self.chunk_bytes {
            self.display_bytes.saturating_sub(self.bytes_done)
        } else {
            0
        };
        let done = self
            .current_file_done
            .saturating_add(extra)
            .min(self.current_file_total);
        (done as f32 / self.current_file_total as f32).clamp(0.0, 1.0)
    }

    /// 返回输出大小与已处理源字节的比值。
    pub fn compression_ratio(&self) -> Option<f32> {
        (self.bytes_done > 0).then(|| self.output_bytes as f32 / self.bytes_done as f32)
    }

    pub fn eta_seconds(&self) -> Option<u64> {
        let done = self.display_bytes.max(self.bytes_done);
        if self.speed_bps <= 0.0 || self.bytes_total <= done {
            return None;
        }
        Some(((self.bytes_total - done) as f64 / self.speed_bps) as u64)
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
    use std::time::{Duration, Instant};

    use super::TaskProgress;

    /// 总进度按字节统计，当前文件比例仍按字节统计。
    #[test]
    fn total_ratio_uses_processed_bytes() {
        let progress = TaskProgress {
            current: "b.txt".to_string(),
            bytes_done: 10,
            bytes_total: 100,
            current_file_done: 5,
            current_file_total: 10,
            ..Default::default()
        };
        assert!((progress.ratio() - 0.1).abs() < f32::EPSILON);
        assert!((progress.current_file_ratio() - 0.5).abs() < f32::EPSILON);
    }

    /// 两次真实事件之间按速度推进，但只领先一个编码块。
    #[test]
    fn display_progress_advances_within_one_chunk() {
        let now = Instant::now();
        let chunk = 16 * 1024 * 1024;
        let mut progress = TaskProgress {
            bytes_done: chunk,
            bytes_total: 4 * chunk,
            chunk_bytes: chunk,
            recent_speed_bps: 4.0 * 1024.0 * 1024.0,
            updated_at: Some(now - Duration::from_secs(1)),
            display_bytes: chunk,
            ..Default::default()
        };
        progress.advance_display(now);
        assert!(progress.display_bytes > progress.bytes_done);
        assert!(progress.display_bytes <= progress.bytes_done + chunk);
        assert!(progress.ratio() < 1.0);
    }

    /// 完成前估算值最多到总量减一，不能提前报满。
    #[test]
    fn display_progress_stops_before_total() {
        let now = Instant::now();
        let mut progress = TaskProgress {
            bytes_done: 95,
            bytes_total: 100,
            chunk_bytes: 50,
            recent_speed_bps: 1000.0,
            updated_at: Some(now - Duration::from_secs(10)),
            display_bytes: 95,
            ..Default::default()
        };
        progress.advance_display(now);
        assert_eq!(progress.display_bytes, 99);
    }

    /// 没有字节总量时退回已完成文件数，编辑等任务仍能显示进度。
    #[test]
    fn total_ratio_falls_back_to_entries() {
        let progress = TaskProgress {
            bytes_done: 0,
            bytes_total: 0,
            entries_done: 3,
            entries_total: 4,
            ..Default::default()
        };
        assert!((progress.ratio() - 0.75).abs() < f32::EPSILON);
    }
}
