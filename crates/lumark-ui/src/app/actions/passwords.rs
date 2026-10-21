use super::super::*;

impl LumarkApp {
    /// 从已解锁的密码库读取指定记录。
    pub(crate) fn find_password(&self, id: Uuid) -> Option<PasswordEntry> {
        self.vault.find_entry(id)
    }

    /// 直接创建或更新密码记录，并立即写入加密密码库。
    ///
    /// 密码库页面和独立编辑窗口共用这条路径，因此保存成功后记录已经持久化，
    /// 不需要额外的确认步骤。
    pub(crate) fn save_password_entry(
        &mut self,
        editing: Option<Uuid>,
        label: &str,
        password: String,
        pattern: &str,
        note: &str,
    ) -> Result<Uuid, String> {
        if self.vault.state() != VaultState::Unlocked {
            return Err("请先解锁密码库".to_string());
        }
        if password.is_empty() {
            return Err("密码不能为空".to_string());
        }
        let label = if label.trim().is_empty() {
            "未命名密码".to_string()
        } else {
            label.trim().to_string()
        };
        let pattern = pattern.trim();
        let note = note.trim().to_string();
        let result = if let Some(id) = editing {
            let Some(mut entry) = self.vault.find_entry(id) else {
                return Err("密码条目不存在".to_string());
            };
            entry.label = label;
            entry.password = lumark_vault::SecretString::new(password);
            entry.archive_pattern = (!pattern.is_empty()).then(|| pattern.to_string());
            entry.note = note;
            self.vault.update_entry(entry).map(|_| id)
        } else {
            let mut entry = PasswordEntry::new(label, password);
            entry.archive_pattern = (!pattern.is_empty()).then(|| pattern.to_string());
            entry.note = note;
            self.vault.add_entry(entry)
        };
        match result {
            Ok(id) => {
                self.selected_password = Some(id);
                self.revealed_password = None;
                self.status = "密码已保存到加密密码库".to_string();
                Ok(id)
            }
            Err(lumark_vault::VaultError::DuplicatePassword) => {
                Err("该密码已存在，请编辑已有条目或使用不同密码".to_string())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    /// 从密码库直接删除一条记录。
    pub(crate) fn delete_password(&mut self, id: Uuid) -> Result<(), String> {
        if self.vault.state() != VaultState::Unlocked {
            return Err("请先解锁密码库".to_string());
        }
        self.vault
            .remove_entry(id)
            .map_err(|error| error.to_string())?;
        if self.selected_password == Some(id) {
            self.selected_password = None;
        }
        if self.revealed_password == Some(id) {
            self.revealed_password = None;
        }
        Ok(())
    }

    /// 确认删除当前选中的密码记录。
    pub(crate) fn confirm_delete_password(&mut self) {
        self.show_confirm_delete_password = false;
        let Some(id) = self.selected_password else {
            return;
        };
        match self.delete_password(id) {
            Ok(()) => self.status = "密码已从密码库删除".to_string(),
            Err(error) => self.status = error,
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

    /// 使用密码库页面输入的主密码解锁密码库。
    pub(crate) fn unlock_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = input_value(&self.master_unlock, cx);
        match self.vault.unlock(&password) {
            Ok(()) => {
                set_input(&self.master_unlock, "", window, cx);
                self.status = "密码库已解锁".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 解锁密码库并恢复待处理的归档自动密码尝试。
    pub(crate) fn unlock_for_extract(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = input_value(&self.master_extract_unlock, cx);
        match self.vault.unlock(&password) {
            Ok(()) => {
                set_input(&self.master_extract_unlock, "", window, cx);
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

    /// 设置或轮换密码库主密码，并返回可展示的校验错误。
    pub(crate) fn set_master_password(&mut self, current: &str, new: &str) -> Result<(), String> {
        let current = self.vault.has_master_password().then_some(current);
        match self.vault.set_master_password(current, new) {
            Ok(()) => {
                self.status = if current.is_some() {
                    "主密码已修改".to_string()
                } else {
                    "主密码已设置".to_string()
                };
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
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
                self.selected_password = None;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 验证当前主密码，并让密码库恢复 DPAPI 包裹。
    pub(crate) fn confirm_remove_master(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = input_value(&self.master_remove, cx);
        match self.vault.remove_master_password(&current) {
            Ok(()) => {
                set_input(&self.master_remove, "", window, cx);
                self.show_confirm_remove_master = false;
                self.status =
                    "主密码已删除，密码库改由与当前计算机绑定的 Windows DPAPI 保护".to_string();
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
                self.status = "密码库记录已清空".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}
