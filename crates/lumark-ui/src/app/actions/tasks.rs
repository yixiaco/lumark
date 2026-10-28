//! 后台任务记录与事件桥。
//!
//! 任务事件在后台线程产生，这里按批次合并进度并驱动界面刷新；Shell 多选
//! 压缩先短暂收集路径，再统一打开一个轻量压缩窗口。末尾的托盘与隐藏宿主
//! 逻辑只服务于没有可见窗口的后台压缩。

use super::super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 查找指定任务的历史记录。
    pub(crate) fn task_record(&self, id: u64) -> Option<&TaskRecord> {
        self.task_records.iter().find(|record| record.id == id)
    }

    /// 把新提交的任务加入任务中心列表；已存在时保持原记录，避免重复。
    pub(crate) fn record_task_queued(&mut self, id: u64, kind: TaskKind, label: String) {
        if self.task_records.iter().any(|record| record.id == id) {
            return;
        }
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
    ///
    /// 新的真实进度可能低于上一帧的平滑估算值；保留较高的显示值，避免
    /// 进度条在块完成时倒退。
    pub(crate) fn record_task_progress(&mut self, id: u64, progress: TaskProgress) {
        let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) else {
            return;
        };
        let previous_display = record.progress.display_bytes;
        record.progress = progress;
        record.progress.display_bytes = record
            .progress
            .display_bytes
            .max(previous_display.min(record.progress.bytes_total));
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

    /// 标记任务完成，并把平滑显示值归到总量。
    pub(crate) fn record_task_completed(&mut self, id: u64, message: String) {
        if let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) {
            record.state = TaskState::Completed;
            record.message = Some(message);
            if record.progress.bytes_total > 0 {
                record.progress.bytes_done =
                    record.progress.bytes_done.max(record.progress.bytes_total);
                record.progress.display_bytes = record.progress.bytes_total;
            }
        }
    }

    /// 标记任务失败并保存失败原因。
    pub(crate) fn record_task_failed(&mut self, id: u64, message: String) {
        if let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) {
            record.state = TaskState::Failed;
            record.message = Some(message);
            record.progress.display_bytes = record.progress.bytes_done;
        }
    }

    /// 标记任务已取消。
    pub(crate) fn record_task_cancelled(&mut self, id: u64) {
        if let Some(record) = self.task_records.iter_mut().find(|record| record.id == id) {
            record.state = TaskState::Cancelled;
            record.message = Some(tr("任务已取消", "Task cancelled").to_string());
            record.progress.display_bytes = record.progress.bytes_done;
        }
    }

    /// 当前任务结束后，把焦点切到最近一个仍在运行的任务。
    pub(crate) fn focus_next_running_task(&mut self, finished_id: u64) {
        self.active_task = None;
        self.task_progress = None;
        self.active_task_kind = None;
        self.active_task_label = None;
        if let Some(next) = self
            .task_records
            .iter()
            .rev()
            .find(|record| record.id != finished_id && !is_terminal_task_state(record.state))
            .cloned()
        {
            self.active_task = Some(next.id);
            self.task_progress = Some(next.progress);
            self.active_task_kind = Some(next.kind);
            self.active_task_label = Some(next.label);
        }
    }

    /// 每个轮询周期推进运行中任务的平滑显示值。
    ///
    /// 真实完成字节仍来自块完成事件；这里只让界面在两次事件之间按最近
    /// 速度前进，最多领先一个编码块，完成前不会显示 100%。
    /// 会话锁定时自动锁定需要主密码的密码库。
    ///
    /// 每两秒查询一次，避免在 120 ms 的任务轮询里频繁调用 WTS API；DPAPI
    /// 密码库无法被主密码重新解锁，因此不在这里处理。
    pub(crate) fn check_session_lock(&mut self) {
        if self.last_session_check.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        self.last_session_check = std::time::Instant::now();
        if platform::session::session_locked()
            && self.vault.is_unlocked()
            && self.vault.has_master_password()
        {
            self.vault.lock();
            self.selected_password = None;
            self.status = tr(
                "Windows 会话已锁定，密码库已自动锁定",
                "Windows session locked; vault locked automatically",
            )
            .to_string();
        }
    }

    pub(crate) fn advance_running_task_progress(&mut self) {
        let now = std::time::Instant::now();
        for index in 0..self.task_records.len() {
            let id = self.task_records[index].id;
            if is_terminal_task_state(self.task_records[index].state) {
                continue;
            }
            // 暂停或正在取消时编码线程已经停下，继续推进估算会让进度虚跑。
            let running = self
                .tasks
                .control(id)
                .is_none_or(|control| control.state() == TaskState::Running);
            if running {
                self.task_records[index].progress.advance_display(now);
            } else {
                // 暂停/取消期间不推进；把事件时间刷新到当前，恢复后不会
                // 因为累积了整段暂停时间而一次跳很远。
                self.task_records[index].progress.updated_at = Some(now);
            }
        }
        if let Some(id) = self.active_task {
            self.task_progress = self.task_record(id).map(|record| record.progress.clone());
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

    /// 把 Shell 传来的压缩路径加入短暂批次。
    ///
    /// 资源管理器多选时会为每个文件启动一次进程，这里用一个很短的收集
    /// 窗口把这些请求合并成同一个压缩任务。
    pub(crate) fn queue_shell_compress(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            if !self.pending_shell_compress.contains(&path) {
                self.pending_shell_compress.push(path);
            }
        }
        if !self.pending_shell_compress.is_empty() {
            self.shell_compress_deadline =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(300));
        }
    }

    /// 收集窗口结束后，用全部选中路径打开一个轻巧压缩窗口。
    pub(crate) fn flush_shell_compress(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(deadline) = self.shell_compress_deadline else {
            return;
        };
        if std::time::Instant::now() < deadline {
            return;
        }
        self.shell_compress_deadline = None;
        let mut paths = std::mem::take(&mut self.pending_shell_compress);
        let mut seen = HashSet::new();
        paths.retain(|path| path.exists() && seen.insert(path.clone()));
        if !paths.is_empty() {
            self.open_compress_dialog_with_sources(paths, window, cx);
        }
    }

    /// 关闭仅供 Shell 压缩使用的隐藏主窗口；所有压缩相关窗口都结束后才执行。
    fn maybe_close_headless_host(&mut self, cx: &mut Context<Self>) {
        if !self.headless_shell_host || !self.pending_shell_compress.is_empty() {
            return;
        }
        if self.compress_window.is_some_and(|handle| {
            handle
                .update(cx, |_, window, _| window.is_visible())
                .unwrap_or(false)
        }) {
            return;
        }
        if self.active_task.is_some() {
            return;
        }
        if self
            .task_records
            .iter()
            .any(|record| !is_terminal_task_state(record.state))
        {
            return;
        }
        if self.progress_windows.values().any(|entry| {
            entry
                .handle
                .update(cx, |_, window, _| window.is_visible())
                .unwrap_or(false)
        }) {
            return;
        }
        self.headless_shell_host = false;
        let _ = self
            .main_window
            .update(cx, |_, window, _| window.remove_window());
        // 隐藏宿主已经没有任何可见页面，显式结束进程，避免任务完成后残留后台实例。
        cx.quit();
    }

    /// 后台任务没有可见窗口时显示托盘图标，并把点击转成进度窗口或主窗口。
    fn sync_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.headless_shell_host {
            self.tray_icon = None;
            let _ = platform::tray::take_activation_request();
            return;
        }
        if self.tray_icon.is_some() && platform::tray::take_activation_request() {
            self.tray_icon = None;
            let task_id = self.active_task.or_else(|| {
                self.task_records
                    .iter()
                    .find(|record| !is_terminal_task_state(record.state))
                    .map(|record| record.id)
            });
            if let Some(task_id) = task_id {
                self.open_progress_window_for(task_id, window, cx);
            } else {
                self.spawn_window(None, None, cx);
                self.headless_shell_host = false;
                let _ = self
                    .main_window
                    .update(cx, |_, window, _| window.remove_window());
            }
            return;
        }
        let waiting =
            self.shell_compress_deadline.is_some() || !self.pending_shell_compress.is_empty();
        let compress_visible = self.compress_window.is_some_and(|handle| {
            handle
                .update(cx, |_, window, _| window.is_visible())
                .unwrap_or(false)
        });
        let progress_visible = self.progress_windows.values().any(|entry| {
            entry
                .handle
                .update(cx, |_, window, _| window.is_visible())
                .unwrap_or(false)
        });
        let has_running_tasks = self
            .task_records
            .iter()
            .any(|record| !is_terminal_task_state(record.state));
        if waiting
            || compress_visible
            || progress_visible
            || (self.active_task.is_none() && !has_running_tasks)
        {
            self.tray_icon = None;
            let _ = platform::tray::take_activation_request();
        } else if self.tray_icon.is_none() {
            self.tray_icon = platform::tray::TrayIcon::show("Lumark - 后台任务").ok();
        }
    }

    /// 启动后台任务事件桥；它先合并进度，再通知界面。
    pub(crate) fn start_task_polling(&self, cx: &mut Context<Self>) {
        let tasks = self.tasks.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
                let mut events = Vec::new();
                while let Some(event) = tasks.try_recv() {
                    events.push(event);
                }
                let _ = this.update_in(cx, |app, window, cx| {
                    app.check_session_lock();
                    if !app.ipc_poll_owner && !IPC_POLL_OWNER.swap(true, Ordering::AcqRel) {
                        app.ipc_poll_owner = true;
                    }
                    let mut requests = if app.ipc_poll_owner {
                        lumark_platform_windows::ipc::take_requests().unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    if app.ipc_poll_owner {
                        requests.extend(lumark_platform_windows::pipe::take_requests());
                    }
                    let had_requests = !requests.is_empty();
                    let had_events = !events.is_empty() || had_requests;
                    let auto_locked = app.vault.maybe_auto_lock();
                    if auto_locked {
                        app.dismiss_password_windows(cx);
                        app.selected_password = None;
                        app.pending_extra_password = None;
                        app.show_extra_password_prompt = false;
                        app.status = "密码库已自动锁定".to_string();
                    }
                    for event in events {
                        match event {
                            TaskEvent::Queued { id, kind, label } => {
                                app.record_task_queued(id, kind, label.clone());
                                if kind != TaskKind::PasswordResolve {
                                    app.active_task = Some(id);
                                    app.task_progress = None;
                                    if matches!(
                                        kind,
                                        TaskKind::Compress
                                            | TaskKind::Extract
                                            | TaskKind::Test
                                            | TaskKind::Disguise
                                            | TaskKind::Restore
                                    ) {
                                        app.active_task_kind = Some(kind);
                                        app.active_task_label = Some(label.clone());
                                        cx.defer_in(window, move |app, window, cx| {
                                            app.open_auto_progress_window_for(id, window, cx);
                                        });
                                    }
                                }
                                app.status = label;
                            }
                            TaskEvent::Progress { id, progress } => {
                                app.record_task_progress(id, progress.clone());
                                // 用记录里合并后的显示值，避免块事件把上一帧的
                                // 平滑值短暂压回去。
                                let merged = app
                                    .task_record(id)
                                    .map(|record| record.progress.clone())
                                    .unwrap_or(progress);
                                app.update_progress_window_for(id, merged.clone(), cx);
                                if app.active_task == Some(id) {
                                    app.task_progress = Some(merged);
                                }
                            }
                            TaskEvent::Completed { id, message } => {
                                let compress_label = app
                                    .task_record(id)
                                    .filter(|record| record.kind == TaskKind::Compress)
                                    .map(|record| record.label.clone());
                                app.record_task_completed(id, message.clone());
                                if let Some(label) = compress_label {
                                    app.notify_compression_completed(&label, &message, cx);
                                }
                                // 任务已经结束，先收回自动打开的进度窗口；后续分支
                                // 可能继续打开查看器或结果窗口，但都不应留下一个停在
                                // 100% 的进度窗口。
                                app.dismiss_progress_window_for(id, cx);
                                if let Some(job) = app.pending_password_jobs.remove(&id) {
                                    app.vault.release_lease();
                                    app.clear_password_attempt(id);
                                    if let Some(resolved) = tasks.take_password_result(id) {
                                        let archive_info = tasks
                                            .take_archive_load_result(id)
                                            .map(|result| result.info);
                                        app.finish_password_resolution(
                                            job,
                                            resolved,
                                            archive_info,
                                            window,
                                            cx,
                                        );
                                    } else if job.save_on_success {
                                        // 手动输入的密码未通过校验：重新弹出密码框，
                                        // 保留“保存到密码库”的勾选状态。
                                        app.apply_password_failure(job.action, window, cx);
                                        app.status = tr("密码不正确，请重新输入", "Incorrect password; try again").to_string();
                                    } else if job.extras.is_empty() {
                                        app.status = tr("记录密码均不正确，请手动输入密码", "Saved passwords did not match; enter one manually").to_string();
                                        app.apply_password_failure(job.action, window, cx);
                                    } else {
                                        let count = job.extras.len();
                                        app.pending_extra_password =
                                            Some(PendingExtraPasswordJob {
                                                action: job.action,
                                                candidates: job.extras,
                                            });
                                        app.show_extra_password_prompt = true;
                                        app.status = format!(
                                            "前 32 条记录密码不正确，还有 {count} 条未尝试"
                                        );
                                    }
                                    continue;
                                }
                                if app
                                    .pending_open_entry
                                    .as_ref()
                                    .is_some_and(|pending| pending.task_id == id)
                                {
                                    if let Some(pending) = app.pending_open_entry.take() {
                                        if app.active_task == Some(id) {
                                            app.active_task = None;
                                            app.task_progress = None;
                                        }
                                        let target = lumark_archive::safe_join(
                                            &pending.temporary,
                                            &pending.relative,
                                        )
                                        .unwrap_or_else(|_| pending.temporary.clone());
                                        match pending.mode {
                                            PendingOpenMode::External => {
                                                app.open_extracted_target(
                                                    target,
                                                    &pending.temporary,
                                                );
                                            }
                                            PendingOpenMode::Viewer => {
                                                app.open_text_viewer(
                                                    target,
                                                    pending.relative.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }
                                    }
                                    continue;
                                }
                                if let Some(action) = app.pending_archive_loads.remove(&id) {
                                    if app.active_task == Some(id) {
                                        app.active_task = None;
                                        app.task_progress = None;
                                    }
                                    if let Some(result) = tasks.take_archive_load_result(id) {
                                        app.apply_loaded_archive(action, result, window, cx);
                                    } else {
                                        app.status = message;
                                    }
                                    continue;
                                }
                                if let Some(edit) = app.pending_edit_jobs.remove(&id) {
                                    if app.active_task == Some(id) {
                                        app.active_task = None;
                                        app.task_progress = None;
                                    }
                                    let current_archive = match &app.browser.location {
                                        BrowserLocation::Archive { archive, .. } => {
                                            Some(archive.clone())
                                        }
                                        BrowserLocation::FileSystem(_) => None,
                                    };
                                    if current_archive.as_ref() == Some(&edit.archive) {
                                        let password =
                                            app.archive_password.as_deref().map(String::as_str);
                                        if let Err(error) =
                                            app.browser.open_archive(&edit.archive, password)
                                        {
                                            app.status = error.to_string();
                                        } else {
                                            set_input(
                                                &app.location_input,
                                                app.browser.current_display().as_str(),
                                                window,
                                                cx,
                                            );
                                        }
                                    }
                                    app.finish_edit_action(edit.action);
                                    if current_archive.as_ref() == Some(&edit.archive) {
                                        app.status = match edit.action {
                                            PendingEditAction::Add => {
                                                tr("已将文件添加到归档", "Files added to archive").to_string()
                                            }
                                            PendingEditAction::Delete => {
                                                tr("已从归档删除项目", "Items deleted from archive").to_string()
                                            }
                                            PendingEditAction::Rename => {
                                                tr("已重命名归档条目", "Archive entry renamed").to_string()
                                            }
                                        };
                                    } else {
                                        app.status = message;
                                    }
                                    continue;
                                }
                                if app.active_task == Some(id) {
                                    app.focus_next_running_task(id);
                                }
                                app.status = message;
                                // 还原任务完成后定位结果文件；伪装任务完成后
                                // 刷新当前目录，让新生成的媒体文件立即可见。
                                if let Some(restored) = tasks.take_restore_result(id) {
                                    if app.current_directory().as_deref()
                                        == restored.parent()
                                    {
                                        let _ = app.browser.refresh();
                                    }
                                    let _ = platform::reveal(&restored);
                                } else if app
                                    .task_record(id)
                                    .is_some_and(|record| record.kind == TaskKind::Disguise)
                                    && matches!(app.browser.location, BrowserLocation::FileSystem(_))
                                {
                                    let _ = app.browser.refresh();
                                }
                                if let Some(directory) = app.pending_extract_open_dirs.remove(&id) {
                                    let _ = std::process::Command::new("explorer.exe")
                                        .arg(directory)
                                        .spawn();
                                }
                                if let Some((archive, password)) =
                                    app.pending_extract_saves.remove(&id)
                                    && app.vault.settings().prompt_save
                                {
                                    let label = archive
                                        .file_name()
                                        .and_then(|value| value.to_str())
                                        .unwrap_or(tr("压缩包密码", "Archive password"))
                                        .to_string();
                                    set_input(&app.password_label, &label, window, cx);
                                    set_input(&app.password_value, password.as_str(), window, cx);
                                    set_input(
                                        &app.password_pattern,
                                        archive.to_string_lossy().as_ref(),
                                        window,
                                        cx,
                                    );
                                    app.selected_password = None;
                                    app.show_save_prompt = true;
                                }
                                // 关机计划只在这个压缩任务真正成功后执行；失败
                                // 或取消在各自分支里清除，不会关机。
                                if app.pending_shutdown_task == Some(id) {
                                    app.pending_shutdown_task = None;
                                    let delay = platform::power::SHUTDOWN_DELAY_SECONDS;
                                    let done = app.status.clone();
                                    match platform::power::schedule_shutdown(delay) {
                                        Ok(()) => {
                                            app.status = format!(
                                                "{done}；系统将在 {delay} 秒后关机（shutdown /a 可取消）"
                                            );
                                        }
                                        Err(error) => {
                                            app.status = format!("{done}{}{error}", tr("；安排关机失败：", "; failed to schedule shutdown: "));
                                        }
                                    }
                                }
                                app.finish_low_priority(id);
                            }
                            TaskEvent::Failed { id, message } => {
                                app.record_task_failed(id, message.clone());
                                // Shell 压缩宿主没有可见主窗口；失败时保留
                                // 结果窗口，避免进度窗口关闭后宿主立即退出，
                                // 让用户看不到具体失败原因。
                                let show_failure_window = app.headless_shell_host
                                    && app.task_record(id).is_some_and(|record| {
                                        matches!(
                                            record.kind,
                                            TaskKind::Compress
                                                | TaskKind::Extract
                                                | TaskKind::Test
                                                | TaskKind::Edit
                                                | TaskKind::Disguise
                                                | TaskKind::Restore
                                        )
                                    });
                                app.dismiss_progress_window_for(id, cx);
                                if show_failure_window {
                                    app.open_progress_window_for(id, window, cx);
                                }
                                app.pending_extract_saves.remove(&id);
                                app.pending_extract_open_dirs.remove(&id);
                                if app.pending_shutdown_task == Some(id) {
                                    app.pending_shutdown_task = None;
                                }
                                if let Some(job) = app.pending_password_jobs.remove(&id) {
                                    app.vault.release_lease();
                                    app.clear_password_attempt(id);
                                    app.apply_password_failure(job.action, window, cx);
                                    app.status = format!("{}{message}", tr("验证密码失败，请手动输入：", "Password verification failed; enter it manually: "));
                                }
                                app.pending_edit_jobs.remove(&id);
                                app.pending_archive_loads.remove(&id);
                                if app
                                    .pending_open_entry
                                    .as_ref()
                                    .is_some_and(|pending| pending.task_id == id)
                                {
                                    app.pending_open_entry = None;
                                }
                                if app.active_task == Some(id) {
                                    app.focus_next_running_task(id);
                                }
                                app.status = format!("{}{message}", tr("失败：", "Failed: "));
                                app.finish_low_priority(id);
                            }
                            TaskEvent::Cancelled { id } => {
                                app.record_task_cancelled(id);
                                // 取消是终态，自动打开的进度窗口随之收回。
                                app.dismiss_progress_window_for(id, cx);
                                app.pending_extract_saves.remove(&id);
                                app.pending_extract_open_dirs.remove(&id);
                                if app.pending_shutdown_task == Some(id) {
                                    app.pending_shutdown_task = None;
                                }
                                if let Some(job) = app.pending_password_jobs.remove(&id) {
                                    app.vault.release_lease();
                                    app.clear_password_attempt(id);
                                    if app.active_task == Some(id) {
                                        app.focus_next_running_task(id);
                                    }
                                    // 用户主动停止自动尝试：直接回退到手动输入，
                                    // 而不是留下一条“任务已取消”就结束。
                                    app.apply_password_failure(job.action, window, cx);
                                    app.status = tr("已停止自动尝试，请手动输入密码", "Auto-try stopped; enter the password manually").to_string();
                                    continue;
                                }
                                app.pending_edit_jobs.remove(&id);
                                app.pending_archive_loads.remove(&id);
                                if app
                                    .pending_open_entry
                                    .as_ref()
                                    .is_some_and(|pending| pending.task_id == id)
                                {
                                    app.pending_open_entry = None;
                                }
                                if app.active_task == Some(id) {
                                    app.focus_next_running_task(id);
                                }
                                app.status = tr("任务已取消", "Task cancelled").to_string();
                                app.finish_low_priority(id);
                            }
                            TaskEvent::PasswordAttempt { id, index, total } => {
                                let Some(job) = app.pending_password_jobs.get(&id) else {
                                    continue;
                                };
                                if job.save_on_success {
                                    app.status = tr("正在验证密码，成功后会保存到密码库…", "Verifying password; it will be saved to the vault on success...").to_string();
                                } else {
                                    app.status = format!("{}{index}/{total}", tr("正在尝试记录密码 ", "Trying saved password "));
                                }
                                if let Some(attempt) = app.password_attempt.as_mut()
                                    && attempt.task_id == id
                                {
                                    attempt.index = index;
                                    attempt.total = total;
                                }
                            }
                            TaskEvent::StateChanged { id, state } => {
                                app.record_task_state(id, state);
                            }
                        }
                    }
                    // 压缩请求按批次合并；打开/解压仍由独立窗口处理，避免覆盖
                    // 当前窗口正在查看的压缩包。
                    for arguments in requests {
                        if let Some(request) = StartupRequest::parse(arguments) {
                            match request.mode {
                                StartupMode::Compress => app.queue_shell_compress(request.paths),
                                StartupMode::Disguise => {
                                    app.compress_disguise = true;
                                    app.queue_shell_compress(request.paths);
                                }
                                StartupMode::MountIso | StartupMode::UnmountIso => {
                                    if let Some(path) =
                                        request.target().map(Path::to_path_buf)
                                    {
                                        if request.mode == StartupMode::MountIso {
                                            app.mount_iso_path(path, window, cx);
                                        } else {
                                            app.unmount_iso_path(path, window, cx);
                                        }
                                    }
                                }
                                StartupMode::Restore => {
                                    if let Some(path) = request.paths.first().cloned() {
                                        app.start_restore_disguise(path, window, cx);
                                    }
                                }
                                _ => app.open_request_window(request, cx),
                            }
                        }
                    }
                    app.flush_shell_compress(window, cx);
                    app.sync_tray(window, cx);
                    app.maybe_close_headless_host(cx);
                    // 两次真实事件之间按最近速度推进显示值，再补一次定时
                    // 重绘，避免进度条在编码块之间长时间静止。
                    app.advance_running_task_progress();
                    app.refresh_running_progress_windows(cx);
                    // 平滑进度每个轮询周期都在变，主窗口也要跟着重绘。
                    let has_running = app
                        .task_records
                        .iter()
                        .any(|record| !is_terminal_task_state(record.state));
                    if had_events || auto_locked || has_running {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
}

/// 判断任务是否已经进入不可逆的结束状态。
pub(crate) fn is_terminal_task_state(state: TaskState) -> bool {
    matches!(
        state,
        TaskState::Completed | TaskState::Failed | TaskState::Cancelled
    )
}

/// 多个任务合并后的进度统计。
///
/// 任务中心用它展示“跨任务总进度”：排队、运行和结束的任务分别计数，
/// 运行中（含暂停）的任务按字节累加，避免只看单个任务时忽略其它任务。
pub(crate) struct TaskAggregate {
    /// 运行中任务数。
    pub running: usize,
    /// 排队任务数。
    pub queued: usize,
    /// 已完成任务数。
    pub finished: usize,
    /// 合并已完成字节。
    pub bytes_done: u64,
    /// 合并总字节。
    pub bytes_total: u64,
}

impl TaskAggregate {
    /// 合并进度：按运行任务的字节总量计算，没有总量时为 0。
    pub fn ratio(&self) -> f32 {
        if self.bytes_total == 0 {
            0.0
        } else {
            (self.bytes_done as f32 / self.bytes_total as f32).clamp(0.0, 1.0)
        }
    }
}

/// 汇总任务记录，得到排队/运行/结束数量和运行任务的字节进度。
pub(crate) fn aggregate_task_progress(records: &[TaskRecord]) -> TaskAggregate {
    let mut aggregate = TaskAggregate {
        running: 0,
        queued: 0,
        finished: 0,
        bytes_done: 0,
        bytes_total: 0,
    };
    for record in records {
        match record.state {
            TaskState::Queued => aggregate.queued += 1,
            state if is_terminal_task_state(state) => aggregate.finished += 1,
            _ => {
                aggregate.running += 1;
                aggregate.bytes_done = aggregate.bytes_done.saturating_add(
                    record
                        .progress
                        .display_bytes
                        .max(record.progress.bytes_done),
                );
                aggregate.bytes_total = aggregate
                    .bytes_total
                    .saturating_add(record.progress.bytes_total);
            }
        }
    }
    aggregate
}

/// 返回任务状态名称。
pub(crate) fn task_state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Queued => crate::i18n::tr("排队中", "Queued"),
        TaskState::Running => crate::i18n::tr("进行中", "Running"),
        TaskState::Paused => crate::i18n::tr("已暂停", "Paused"),
        TaskState::Cancelling => crate::i18n::tr("取消中", "Cancelling"),
        TaskState::Completed => crate::i18n::tr("已完成", "Completed"),
        TaskState::Failed => crate::i18n::tr("失败", "Failed"),
        TaskState::Cancelled => crate::i18n::tr("已取消", "Cancelled"),
    }
}
