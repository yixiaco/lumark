//! 密码请求、记录候选验证和保存流程。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 显示独立的压缩包密码输入弹窗，并暂存密码通过后的目标动作。
    pub(crate) fn request_archive_password(
        &mut self,
        archive: PathBuf,
        action: PendingPasswordAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_archive_password_prompt = true;
        self.archive_password_action = Some(action);
        self.archive_password_visible = false;
        self.archive_password_use_vault =
            self.vault.state() == VaultState::Unlocked && self.vault.settings().auto_extract;
        // 保存引导只在自动尝试失败后出现，普通密码输入保持不写入密码库。
        self.archive_password_after_failure = false;
        self.archive_password_save_to_vault = false;
        set_input(&self.archive_password_input, "", window, cx);
        self.archive_password_input
            .update(cx, |state, cx| state.set_masked(true, window, cx));
        self.dismiss_extract_window(cx);
        self.status = format!("{} 已加密，请输入密码", archive.display());
        cx.notify();
    }

    /// 读取密码弹窗并继续之前的打开、解压或测试动作。
    pub(crate) fn confirm_archive_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.archive_password_action.clone() else {
            self.show_archive_password_prompt = false;
            return;
        };
        let password = input_value(&self.archive_password_input, cx);
        if password.is_empty() {
            if self.archive_password_use_vault
                && self.vault.state() == VaultState::Unlocked
                && self.vault.settings().auto_extract
            {
                let archive = password_action_archive(&action).to_path_buf();
                if self.begin_vault_password_resolution(&archive, action, cx) {
                    self.show_archive_password_prompt = false;
                    self.archive_password_action = None;
                } else {
                    self.status = "密码库中没有匹配密码，请手动输入".to_string();
                }
            } else {
                self.status = "请输入压缩包密码".to_string();
            }
            cx.notify();
            return;
        }

        self.show_archive_password_prompt = false;
        self.archive_password_action = None;
        self.archive_password_after_failure = false;
        // 勾选保存时先在工作线程校验密码：命中后才写入密码库并继续原动作，
        // 避免把输错的密码持久化到密码库。
        let save_to_vault = std::mem::take(&mut self.archive_password_save_to_vault);
        if save_to_vault && self.vault.state() == VaultState::Unlocked {
            let archive = password_action_archive(&action).to_path_buf();
            self.submit_manual_password_verification(
                archive,
                action,
                zeroize::Zeroizing::new(password),
            );
            cx.notify();
            return;
        }
        match action {
            PendingPasswordAction::OpenArchive { archive, intent } => {
                self.begin_archive_load(archive, Some(zeroize::Zeroizing::new(password)), intent);
            }
            PendingPasswordAction::ExtractDialog { archive } => {
                self.prepare_extract_dialog(archive, false, Some(&password), window, cx);
            }
            PendingPasswordAction::OpenEntry {
                archive,
                relative,
                mode,
            } => {
                self.open_archive_entry_with_password(
                    archive,
                    relative,
                    Some(zeroize::Zeroizing::new(password)),
                    mode,
                    window,
                    cx,
                );
            }
            PendingPasswordAction::Test { archive } => {
                self.submit_archive_test(archive, Some(password));
            }
        }
        cx.notify();
    }

    /// 启动基于密码库的密码匹配，或在需要时先请求解锁。
    pub(crate) fn begin_vault_password_resolution(
        &mut self,
        archive: &Path,
        action: PendingPasswordAction,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.vault.state() != VaultState::Unlocked || !self.vault.settings().auto_extract {
            return false;
        }
        let fingerprint = lumark_vault::PasswordResolver::archive_fingerprint(archive).ok();
        let candidates = self.vault.candidates(archive, fingerprint.as_deref());
        let mut candidates = candidates.into_iter();
        let primary: Vec<_> = candidates.by_ref().take(32).collect();
        let extras: Vec<_> = candidates.collect();
        self.submit_password_candidates(archive, action, primary, extras, false, cx)
    }

    /// 在工作线程中校验有序候选密码，不写入输出。
    ///
    /// `save_on_success` 只用于手动输入密码的校验：命中后先把密码写入密码库，
    /// 再继续原动作；自动尝试沿用已有候选记录，不会重复写入。
    pub(crate) fn submit_password_candidates(
        &mut self,
        archive: &Path,
        action: PendingPasswordAction,
        candidates: Vec<lumark_vault::PasswordCandidate>,
        extras: Vec<lumark_vault::PasswordCandidate>,
        save_on_success: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if candidates.is_empty() {
            return false;
        }
        self.vault.acquire_lease();
        let entry_ids = candidates
            .iter()
            .map(|candidate| candidate.entry_id)
            .collect();
        let total = candidates.len();
        let passwords = candidates
            .into_iter()
            .map(|candidate| zeroize::Zeroizing::new(candidate.password.expose().to_string()))
            .collect();
        let load_info = matches!(&action, PendingPasswordAction::OpenArchive { .. });
        let id = self.tasks.submit(TaskRequest::ResolvePassword {
            archive: archive.to_path_buf(),
            candidates: passwords,
            load_info,
        });
        self.pending_password_jobs.insert(
            id,
            PendingPasswordJob {
                action,
                entry_ids,
                extras,
                save_on_success,
            },
        );
        if save_on_success {
            self.status = tr(
                "正在验证密码，成功后会保存到密码库…",
                "Verifying password; it will be saved to the vault on success...",
            )
            .to_string();
            return true;
        }
        self.status = tr("正在验证记录密码…", "Verifying saved passwords...").to_string();
        self.track_password_attempt(cx, id, archive, total);
        true
    }

    /// 校验手动输入的密码，命中后先写入密码库再继续原动作。
    ///
    /// 单独走一次单候选校验，是为了在保存前确认密码正确；否则输错一次就会
    /// 在密码库里留下一条永远解不开压缩包的记录。
    pub(crate) fn submit_manual_password_verification(
        &mut self,
        archive: PathBuf,
        action: PendingPasswordAction,
        password: zeroize::Zeroizing<String>,
    ) {
        self.vault.acquire_lease();
        let id = self.tasks.submit(TaskRequest::ResolvePassword {
            archive,
            candidates: vec![password],
            load_info: false,
        });
        self.pending_password_jobs.insert(
            id,
            PendingPasswordJob {
                action,
                entry_ids: Vec::new(),
                extras: Vec::new(),
                save_on_success: true,
            },
        );
        self.status = tr(
            "正在验证密码，成功后会保存到密码库…",
            "Verifying password; it will be saved to the vault on success...",
        )
        .to_string();
    }

    /// 记录多候选校验的进度，并安排一次延迟重绘。
    ///
    /// 只有多候选校验才值得展示停止入口；延迟重绘让弹窗在验证变慢时出现，
    /// 快速命中时用户看不到任何闪烁。
    pub(crate) fn track_password_attempt(
        &mut self,
        cx: &mut Context<Self>,
        task_id: u64,
        archive: &Path,
        total: usize,
    ) {
        if total < 2 {
            return;
        }
        self.password_attempt = Some(PasswordAttemptProgress {
            task_id,
            archive: archive.to_path_buf(),
            index: 0,
            total,
            started: std::time::Instant::now(),
            hidden: false,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(420))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app
                    .password_attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.task_id == task_id)
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 停止当前记录密码校验，取消事件到达后回退到手动输入。
    pub(crate) fn cancel_password_attempt(&mut self, cx: &mut Context<Self>) {
        let Some(attempt) = self.password_attempt.take() else {
            return;
        };
        match self.tasks.control(attempt.task_id) {
            Some(control) => {
                control.cancel();
                self.status = format!(
                    "正在停止密码验证，随后请手动输入：{}",
                    attempt.archive.display()
                );
            }
            None => self.status = "密码验证已经结束".to_string(),
        }
        cx.notify();
    }

    /// 清除属于指定任务的校验进度。
    ///
    /// 事件可能晚于用户点击停止到达，按任务 ID 比对可以避免清掉新任务的进度。
    pub(crate) fn clear_password_attempt(&mut self, task_id: u64) {
        if self
            .password_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.task_id == task_id)
        {
            self.password_attempt = None;
        }
    }

    /// 应用密码解析结果：保存勾选的密码、记录使用次数并继续原动作。
    pub(crate) fn finish_password_resolution(
        &mut self,
        job: PendingPasswordJob,
        resolved: lumark_tasks::ResolvedPassword,
        archive_info: Option<lumark_archive::ArchiveInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry_id) = job.entry_ids.get(resolved.candidate_index) {
            let _ = self.vault.mark_used(*entry_id);
        }
        let vault_locked = job.save_on_success && self.vault.state() != VaultState::Unlocked;
        let saved = job.save_on_success
            && self.remember_verified_password(
                password_action_archive(&job.action),
                resolved.password.as_str(),
            );
        self.apply_resolved_password(job.action, resolved.password, archive_info, window, cx);
        if saved {
            self.status = format!("{}（密码已保存到密码库）", self.status);
        } else if vault_locked {
            self.status = format!("{}（密码库已锁定，未保存密码）", self.status);
        }
    }

    /// 把刚验证通过的密码写入密码库，重复记录保持原状。
    pub(crate) fn remember_verified_password(&mut self, archive: &Path, password: &str) -> bool {
        if self.vault.state() != VaultState::Unlocked {
            return false;
        }
        if self
            .vault
            .entries()
            .iter()
            .any(|entry| entry.password.expose() == password)
        {
            return false;
        }
        let label = archive
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(tr("压缩包密码", "Archive password"))
            .to_string();
        let pattern = archive.to_string_lossy();
        self.save_password_entry(None, &label, password.to_string(), &pattern, "")
            .is_ok()
    }

    /// 继续尝试超出首次自动尝试上限的候选密码。
    pub(crate) fn try_extra_password_candidates(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(extra) = self.pending_extra_password.take() else {
            self.show_extra_password_prompt = false;
            return;
        };
        self.show_extra_password_prompt = false;
        let archive = match &extra.action {
            PendingPasswordAction::OpenArchive { archive, .. }
            | PendingPasswordAction::ExtractDialog { archive }
            | PendingPasswordAction::OpenEntry { archive, .. }
            | PendingPasswordAction::Test { archive } => archive.clone(),
        };
        if self.vault.state() != VaultState::Unlocked {
            self.apply_password_failure(extra.action, window, cx);
            self.status = "密码库已锁定，请先解锁".to_string();
            return;
        }
        if !self.submit_password_candidates(
            &archive,
            extra.action,
            extra.candidates,
            Vec::new(),
            false,
            cx,
        ) {
            self.status = tr("没有剩余记录密码", "No remaining saved passwords").to_string();
        }
    }

    /// 停止自动尝试并回退到手动密码输入。
    pub(crate) fn decline_extra_password_candidates(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_extra_password_prompt = false;
        if let Some(extra) = self.pending_extra_password.take() {
            self.apply_password_failure(extra.action, window, cx);
            self.status = tr(
                "已停止自动尝试，请手动输入密码",
                "Auto-try stopped; enter the password manually",
            )
            .to_string();
        }
    }

    /// 把成功解析的密码分发给待处理的归档操作。
    pub(crate) fn apply_resolved_password(
        &mut self,
        action: PendingPasswordAction,
        password: zeroize::Zeroizing<String>,
        archive_info: Option<lumark_archive::ArchiveInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingPasswordAction::OpenArchive { archive, intent } => {
                self.open_archive_now(archive, Some(password), intent, archive_info, window, cx);
            }
            PendingPasswordAction::ExtractDialog { archive } => {
                self.prepare_extract_dialog(archive, false, Some(password.as_str()), window, cx);
            }
            PendingPasswordAction::OpenEntry {
                archive,
                relative,
                mode,
            } => {
                self.open_archive_entry_with_password(
                    archive,
                    relative,
                    Some(password),
                    mode,
                    window,
                    cx,
                );
            }
            PendingPasswordAction::Test { archive } => {
                self.submit_archive_test(archive, Some(password.to_string()));
            }
        }
    }

    /// 所有记录密码失败后回退到手动输入。
    pub(crate) fn apply_password_failure(
        &mut self,
        action: PendingPasswordAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingPasswordAction::ExtractDialog { archive } => {
                let prompt_action = PendingPasswordAction::OpenArchive {
                    archive: archive.clone(),
                    intent: ArchiveOpenIntent::Extract,
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = tr(
                    "记录密码均不正确，请手动输入密码",
                    "Saved passwords did not match; enter one manually",
                )
                .to_string();
            }
            PendingPasswordAction::OpenArchive { archive, intent } => {
                let prompt_action = PendingPasswordAction::OpenArchive {
                    archive: archive.clone(),
                    intent,
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = tr(
                    "记录密码均不正确，请手动输入密码",
                    "Saved passwords did not match; enter one manually",
                )
                .to_string();
            }
            PendingPasswordAction::OpenEntry {
                archive,
                relative,
                mode,
            } => {
                let prompt_action = PendingPasswordAction::OpenEntry {
                    archive: archive.clone(),
                    relative,
                    mode,
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = tr(
                    "记录密码均不正确，请手动输入密码",
                    "Saved passwords did not match; enter one manually",
                )
                .to_string();
            }
            PendingPasswordAction::Test { archive } => {
                let prompt_action = PendingPasswordAction::Test {
                    archive: archive.clone(),
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = tr(
                    "记录密码均不正确，请手动输入密码",
                    "Saved passwords did not match; enter one manually",
                )
                .to_string();
            }
        }
        // 自动尝试失败后引导用户把本次密码存进密码库：默认勾选，输入后先
        // 校验再写入，避免保存错误密码。
        self.archive_password_after_failure = true;
        self.archive_password_save_to_vault = self.vault.state() == VaultState::Unlocked;
    }

    /// 把新验证成功的解压密码写入加密密码库。
    ///
    /// 保存提示只负责新增记录；若同一密码已经存在，则把当前压缩包路径
    /// 补充到已有条目，避免密码库出现重复密码。
    pub(crate) fn save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = input_value(&self.password_label, cx);
        let password = input_value(&self.password_value, cx);
        let pattern = input_value(&self.password_pattern, cx);
        let note = input_value(&self.password_note, cx);
        if password.is_empty() {
            self.status = tr("密码不能为空", "Password cannot be empty").to_string();
            return;
        }
        let duplicate = self
            .vault
            .entries()
            .into_iter()
            .find(|entry| entry.password.expose() == password);
        if let Some(mut entry) = duplicate {
            if !pattern.trim().is_empty() {
                entry.archive_pattern = Some(pattern.trim().to_string());
            }
            if !note.trim().is_empty() {
                entry.note = note.trim().to_string();
            }
            match self.vault.update_entry(entry) {
                Ok(()) => {
                    self.status = "密码已存在，已将当前压缩包关联到该条目".to_string();
                    self.show_save_prompt = false;
                }
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        match self.save_password_entry(None, &label, password, &pattern, &note) {
            Ok(_) => {
                self.show_save_prompt = false;
                set_input(&self.password_label, "", window, cx);
                set_input(&self.password_value, "", window, cx);
                set_input(&self.password_pattern, "", window, cx);
                set_input(&self.password_note, "", window, cx);
            }
            Err(error) => self.status = error,
        }
    }
}
