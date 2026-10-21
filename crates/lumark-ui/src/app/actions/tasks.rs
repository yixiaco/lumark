use super::super::*;

impl LumarkApp {
    /// 查找指定任务的历史记录。
    pub(crate) fn task_record(&self, id: u64) -> Option<&TaskRecord> {
        self.task_records.iter().find(|record| record.id == id)
    }

    /// 把新提交的任务加入任务中心列表。
    pub(crate) fn record_task_queued(&mut self, id: u64, kind: TaskKind, label: String) {
        self.task_records.push(TaskRecord {
            id,
            kind,
            label,
            state: TaskState::Queued,
            progress: TaskProgress::default(),
            message: None,
        });
        self.prune_task_records();
    }

    /// 更新任务进度，不覆盖已经结束的终态。
    pub(crate) fn record_task_progress(&mut self, id: u64, progress: TaskProgress) {
        let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) else {
            return;
        };
        record.progress = progress;
        if matches!(record.state, TaskState::Queued | TaskState::Running) {
            record.state = TaskState::Running;
        }
    }

    /// 更新任务状态，终态优先于后到的运行态事件。
    pub(crate) fn record_task_state(&mut self, id: u64, state: TaskState) {
        let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) else {
            return;
        };
        if !is_terminal_task_state(record.state) {
            record.state = state;
        }
    }

    /// 标记任务完成并保存最终提示。
    pub(crate) fn record_task_completed(&mut self, id: u64, message: String) {
        if let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) {
            record.state = TaskState::Completed;
            record.message = Some(message);
        }
    }

    /// 标记任务失败并保存失败原因。
    pub(crate) fn record_task_failed(&mut self, id: u64, message: String) {
        if let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) {
            record.state = TaskState::Failed;
            record.message = Some(message);
        }
    }

    /// 标记任务已取消。
    pub(crate) fn record_task_cancelled(&mut self, id: u64) {
        if let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) {
            record.state = TaskState::Cancelled;
            record.message = Some("任务已取消".to_string());
        }
    }

    /// 限制历史记录数量，优先淘汰最早结束的任务。
    fn prune_task_records(&mut self) {
        while self.task_records.len() > 80 {
            let index = self
                .task_records
                .iter()
                .position(|record| is_terminal_task_state(record.state))
                .unwrap_or(0);
            self.task_records.remove(index);
        }
    }
}

/// 判断任务是否已经进入不可逆的结束状态。
pub(crate) fn is_terminal_task_state(state: TaskState) -> bool {
    matches!(
        state,
        TaskState::Completed | TaskState::Failed | TaskState::Cancelled
    )
}

/// 返回任务状态的中文名称。
pub(crate) fn task_state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Queued => "排队中",
        TaskState::Running => "进行中",
        TaskState::Paused => "已暂停",
        TaskState::Cancelling => "取消中",
        TaskState::Completed => "已完成",
        TaskState::Failed => "失败",
        TaskState::Cancelled => "已取消",
    }
}
