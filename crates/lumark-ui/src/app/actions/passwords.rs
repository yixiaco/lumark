use super::super::*;

impl LumarkApp {
    /// 密码库解锁后初始化可编辑的密码草稿。
    pub(crate) fn ensure_password_draft(&mut self) {
        if self.password_draft.is_none() && self.vault.state() == VaultState::Unlocked {
            self.password_draft = Some(self.vault.entries());
            self.password_draft_dirty = false;
        }
    }

    /// 优先在草稿中查找记录，未找到时回退到持久化密码库。
    pub(crate) fn find_password(&self, id: Uuid) -> Option<PasswordEntry> {
        match &self.password_draft {
            Some(entries) => entries.iter().find(|entry| entry.id == id).cloned(),
            None => self.vault.find_entry(id),
        }
    }

    /// 校验并在内存草稿中创建或更新密码记录。
    pub(crate) fn draft_save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_password_draft();
        let Some(entries) = self.password_draft.as_mut() else {
            self.status = "请先解锁密码库".to_string();
            return;
        };
        let label = input_value(&self.password_label, cx);
        let password = input_value(&self.password_value, cx);
        let confirm = input_value(&self.password_confirm, cx);
        let pattern = input_value(&self.password_pattern, cx);
        let note = input_value(&self.password_note, cx);
        if password.is_empty() {
            self.status = "密码不能为空".to_string();
            return;
        }
        if password != confirm {
            self.status = "两次输入的密码不一致".to_string();
            return;
        }
        let duplicate = entries.iter().any(|entry| {
            Some(entry.id) != self.selected_password && entry.password.expose() == password
        });
        if duplicate {
            self.status = "该密码已存在，请编辑已有条目或使用不同密码".to_string();
            return;
        }
        if let Some(id) = self.selected_password {
            if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
                entry.label = label;
                entry.password = lumark_vault::SecretString::new(password);
                entry.archive_pattern = (!pattern.trim().is_empty()).then_some(pattern);
                entry.note = note;
                entry.updated_at = chrono::Utc::now();
            }
        } else {
            let mut entry = PasswordEntry::new(
                if label.trim().is_empty() {
                    "未命名密码"
                } else {
                    label.trim()
                },
                password,
            );
            entry.archive_pattern = (!pattern.trim().is_empty()).then_some(pattern);
            entry.note = note;
            self.selected_password = Some(entry.id);
            entries.push(entry);
        }
        self.password_draft_dirty = true;
        set_input(&self.password_confirm, "", window, cx);
        self.status = "密码修改已加入草稿，点击“确定”后写入密码库".to_string();
    }

    /// 从草稿中移除所选记录，此时不修改密码库。
    pub(crate) fn draft_delete_password(&mut self) {
        self.ensure_password_draft();
        let Some(entries) = self.password_draft.as_mut() else {
            self.status = "请先解锁密码库".to_string();
            return;
        };
        let Some(id) = self.selected_password else {
            self.status = "请先选择一条密码".to_string();
            return;
        };
        entries.retain(|entry| entry.id != id);
        self.selected_password = None;
        self.password_draft_dirty = true;
        self.status = "密码已从草稿删除，点击“确定”生效".to_string();
    }

    /// 把草稿作为一次密码库事务持久化，并报告校验失败。
    pub(crate) fn commit_password_draft(&mut self) {
        let Some(entries) = self.password_draft.clone() else {
            return;
        };
        match self.vault.replace_entries(entries) {
            Ok(()) => {
                self.password_draft = None;
                self.password_draft_dirty = false;
                self.status = "密码库已保存".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 丢弃全部未提交的密码修改并重新加载持久化记录。
    pub(crate) fn discard_password_draft(&mut self) {
        self.password_draft = None;
        self.password_draft_dirty = false;
        self.selected_password = None;
        self.status = "已取消密码修改".to_string();
    }

    /// 丢弃草稿并恢复此前请求的页面切换。
    pub(crate) fn discard_draft_and_switch(&mut self) {
        self.discard_password_draft();
        self.show_discard_password_draft = false;
        if let Some(screen) = self.pending_screen.take() {
            self.screen = screen;
            if screen == Screen::Passwords {
                self.ensure_password_draft();
            }
        }
    }

    /// 复制所选密码，并在 30 秒后清理剪贴板。
    pub(crate) fn copy_password(&self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_password else {
            return;
        };
        if let Some(entry) = self.find_password(id) {
            let password = entry.password.expose().to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(password.clone()));
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                let _ = this.update(cx, |_, cx| {
                    if cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .is_some_and(|current| current == password)
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                    }
                });
            })
            .detach();
        }
    }

    /// 临时显示一个密码十秒，随后重新掩码。
    pub(crate) fn toggle_password_reveal(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if self.revealed_password == Some(id) {
            self.revealed_password = None;
            return;
        }
        self.revealed_password = Some(id);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(10))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.revealed_password == Some(id) {
                    app.revealed_password = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 使用输入的主密码解锁密码库并加载新草稿。
    pub(crate) fn unlock_vault(&mut self, cx: &mut Context<Self>) {
        let password = input_value(&self.master_current, cx);
        match self.vault.unlock(&password) {
            Ok(()) => {
                self.status = "密码库已解锁".to_string();
                self.password_draft = Some(self.vault.entries());
                self.password_draft_dirty = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 解锁密码库并恢复待处理的归档自动密码尝试。
    pub(crate) fn unlock_for_extract(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = input_value(&self.master_current, cx);
        match self.vault.unlock(&password) {
            Ok(()) => {
                set_input(&self.master_current, "", window, cx);
                self.show_unlock_for_extract = false;
                if let Some(archive) = self.pending_unlock_archive.take() {
                    let action = if self.unlock_for_browse {
                        PendingPasswordAction::OpenArchive {
                            archive: archive.clone(),
                            intent: ArchiveOpenIntent::Browse,
                        }
                    } else {
                        PendingPasswordAction::ExtractDialog {
                            archive: archive.clone(),
                        }
                    };
                    if !self.begin_vault_password_resolution(&archive, action) {
                        self.status = "没有可用的记录密码，请手动输入或新增密码".to_string();
                        if !self.unlock_for_browse {
                            self.prepare_extract_dialog(archive, false, None, window, cx);
                        }
                    }
                }
                self.unlock_for_browse = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 设置或轮换密码库主密码，无需重新加密每条记录。
    pub(crate) fn set_master_password(&mut self, cx: &mut Context<Self>) {
        let current = input_value(&self.master_current, cx);
        let new = input_value(&self.master_new, cx);
        let current_ref = self.vault.has_master_password().then_some(current.as_str());
        match self.vault.set_master_password(current_ref, &new) {
            Ok(()) => self.status = "主密码已设置".to_string(),
            Err(error) => self.status = error.to_string(),
        }
        let _ = cx;
    }

    /// 切换自动密码尝试，并把偏好同步到应用设置。
    pub(crate) fn toggle_auto_extract(&mut self, _cx: &mut Context<Self>) {
        let mut settings = self.vault.settings();
        settings.auto_extract = !settings.auto_extract;
        match self.vault.update_settings(settings) {
            Ok(()) => {
                let enabled = settings.auto_extract;
                let _ = self
                    .settings
                    .update(|stored| stored.auto_extract_enabled = enabled);
                self.status = "自动解压设置已更新".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 切换解压后的密码保存提示。
    pub(crate) fn toggle_prompt_save(&mut self, _cx: &mut Context<Self>) {
        let mut settings = self.vault.settings();
        settings.prompt_save = !settings.prompt_save;
        match self.vault.update_settings(settings) {
            Ok(()) => {
                let enabled = settings.prompt_save;
                let _ = self
                    .settings
                    .update(|stored| stored.prompt_save_password = enabled);
                self.status = "保存提示设置已更新".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 循环切换密码库空闲锁定时间。
    pub(crate) fn toggle_auto_lock(&mut self, _cx: &mut Context<Self>) {
        let mut settings = self.vault.settings();
        settings.auto_lock_minutes = match settings.auto_lock_minutes {
            0 => 5,
            5 => 10,
            10 => 30,
            _ => 0,
        };
        match self.vault.update_settings(settings) {
            Ok(()) => {
                let minutes = settings.auto_lock_minutes;
                let _ = self
                    .settings
                    .update(|stored| stored.auto_lock_minutes = minutes);
                self.status = if settings.auto_lock_minutes == 0 {
                    "密码库不会因空闲自动锁定".to_string()
                } else {
                    format!("密码库将在空闲 {} 分钟后锁定", settings.auto_lock_minutes)
                };
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 使用独立口令导出加密密码库快照。
    pub(crate) fn export_vault(&mut self, cx: &mut Context<Self>) {
        let path = PathBuf::from(input_value(&self.vault_transfer_path, cx));
        let passphrase = input_value(&self.vault_transfer_passphrase, cx);
        if path.as_os_str().is_empty() || passphrase.chars().count() < 8 {
            self.status = "请输入导出路径和至少 8 位独立口令".to_string();
            return;
        }
        match self.vault.export(&path, &passphrase) {
            Ok(()) => self.status = format!("密码库已加密导出到 {}", path.display()),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 按合并或替换语义导入加密密码库快照。
    pub(crate) fn import_vault(&mut self, cx: &mut Context<Self>) {
        let path = PathBuf::from(input_value(&self.vault_transfer_path, cx));
        let passphrase = input_value(&self.vault_transfer_passphrase, cx);
        if path.as_os_str().is_empty() || passphrase.is_empty() {
            self.status = "请输入导入文件和导出口令".to_string();
            return;
        }
        match self
            .vault
            .import(&path, &passphrase, self.vault_transfer_replace)
        {
            Ok(count) => {
                self.status = format!("密码库已导入，共 {count} 条记录");
                self.show_vault_transfer = false;
                self.password_draft = None;
                self.password_draft_dirty = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 验证当前主密码，并让密码库恢复 DPAPI 包裹。
    pub(crate) fn confirm_remove_master(&mut self, cx: &mut Context<Self>) {
        let current = input_value(&self.master_current, cx);
        match self.vault.remove_master_password(&current) {
            Ok(()) => {
                self.show_confirm_remove_master = false;
                self.status = "主密码已删除，密码库改由 Windows DPAPI 保护".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 确认对话框通过后清除密码库全部记录。
    pub(crate) fn clear_vault(&mut self, _cx: &mut Context<Self>) {
        match self.vault.clear_entries() {
            Ok(()) => {
                self.show_confirm_clear_vault = false;
                self.selected_password = None;
                self.password_draft = Some(Vec::new());
                self.password_draft_dirty = false;
                self.status = "密码库记录已清空".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}
