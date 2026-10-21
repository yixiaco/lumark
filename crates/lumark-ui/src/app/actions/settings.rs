use super::super::*;

impl LumarkApp {
    /// 循环切换持久化主题偏好并立即应用。
    pub(crate) fn cycle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.settings.get().theme {
            ThemePreference::System => ThemePreference::Light,
            ThemePreference::Light => ThemePreference::Dark,
            ThemePreference::Dark => ThemePreference::System,
        };
        if let Err(error) = self.settings.update(|settings| settings.theme = next) {
            self.status = error.to_string();
            return;
        }
        let accent_color = self.settings.get().accent_color;
        apply_theme(next, &accent_color, window, cx);
        self.status = match next {
            ThemePreference::System => "主题：跟随系统",
            ThemePreference::Light => "主题：浅色",
            ThemePreference::Dark => "主题：深色",
        }
        .to_string();
    }

    /// 循环切换新压缩对话框使用的默认归档格式。
    pub(crate) fn cycle_default_format(&mut self) {
        const FORMATS: [&str; 6] = ["zip", "7z", "tar", "tar.gz", "tar.zst", "gz"];
        let current = self.settings.get().default_format;
        let index = FORMATS
            .iter()
            .position(|format| *format == current)
            .unwrap_or(0);
        let next = FORMATS[(index + 1) % FORMATS.len()];
        match self.settings.update(|settings| {
            settings.default_format = next.to_string();
        }) {
            Ok(()) => self.status = format!("默认格式：{}", next.to_ascii_uppercase()),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 循环切换新任务使用的默认压缩预设。
    pub(crate) fn cycle_default_preset(&mut self) {
        let next = match self.settings.get().default_preset {
            SettingsPreset::Balanced => SettingsPreset::Size,
            SettingsPreset::Size => SettingsPreset::Speed,
            SettingsPreset::Speed => SettingsPreset::Balanced,
        };
        match self
            .settings
            .update(|settings| settings.default_preset = next)
        {
            Ok(()) => {
                self.status = format!(
                    "压缩预设：{}",
                    match next {
                        SettingsPreset::Speed => "速度优先",
                        SettingsPreset::Balanced => "平衡",
                        SettingsPreset::Size => "体积优先",
                    }
                );
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 切换成功解压后是否打开目标文件夹。
    pub(crate) fn toggle_open_after_extract(&mut self) {
        let next = !self.settings.get().open_after_extract;
        match self
            .settings
            .update(|settings| settings.open_after_extract = next)
        {
            Ok(()) => {
                self.status = if next {
                    "解压完成后将打开目标目录".to_string()
                } else {
                    "解压完成后不自动打开目录".to_string()
                };
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 切换归档原位置编辑前的备份。
    pub(crate) fn toggle_archive_backup(&mut self) {
        let next = !self.settings.get().archive_edit_backup;
        match self
            .settings
            .update(|settings| settings.archive_edit_backup = next)
        {
            Ok(()) => {
                self.status = if next {
                    "归档编辑前将保留 .lumark.bak".to_string()
                } else {
                    "归档编辑不保留额外备份".to_string()
                };
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 循环切换解压默认冲突策略。
    pub(crate) fn cycle_overwrite_policy(&mut self) {
        let next = match self.settings.get().overwrite_policy {
            lumark_settings::OverwritePolicy::Ask => lumark_settings::OverwritePolicy::Overwrite,
            lumark_settings::OverwritePolicy::Overwrite => lumark_settings::OverwritePolicy::Skip,
            lumark_settings::OverwritePolicy::Skip => lumark_settings::OverwritePolicy::Rename,
            lumark_settings::OverwritePolicy::Rename => lumark_settings::OverwritePolicy::Ask,
        };
        match self
            .settings
            .update(|settings| settings.overwrite_policy = next)
        {
            Ok(()) => {
                self.status = format!(
                    "解压覆盖策略：{}",
                    match next {
                        lumark_settings::OverwritePolicy::Ask => "询问/默认覆盖",
                        lumark_settings::OverwritePolicy::Overwrite => "覆盖",
                        lumark_settings::OverwritePolicy::Skip => "跳过",
                        lumark_settings::OverwritePolicy::Rename => "重命名",
                    }
                )
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 打开密码库位置编辑对话框，并预填当前文件夹。
    pub(crate) fn open_vault_location_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = self
            .vault
            .path()
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        set_input(&self.vault_location_input, &folder, window, cx);
        self.vault_location_error = None;
        self.show_vault_location = true;
        cx.notify();
    }

    /// 打开系统文件夹选择器，并把结果填入位置编辑框。
    pub(crate) fn browse_vault_location(&mut self, cx: &mut Context<Self>) {
        self.vault_location_error = None;
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择密码库保存文件夹".into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(directory) = paths.into_iter().next() {
                    let _ = this.update_in(cx, |app, window, cx| {
                        set_input(
                            &app.vault_location_input,
                            directory.to_string_lossy().as_ref(),
                            window,
                            cx,
                        );
                        app.vault_location_error = None;
                        cx.notify();
                    });
                }
            }
            Ok(Ok(None)) => {
                let _ = this.update(cx, |app, cx| {
                    app.vault_location_error = None;
                    cx.notify();
                });
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |app, cx| {
                    app.vault_location_error = Some(format!("无法打开文件夹选择器：{error}"));
                    cx.notify();
                });
            }
            Err(_) => {
                let _ = this.update(cx, |app, cx| {
                    app.vault_location_error = Some("文件夹选择器已关闭".to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 把密码库加密文件迁移到指定目录，并持久化新位置。
    pub(crate) fn move_vault_to(
        &mut self,
        directory: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self
            .task_records
            .iter()
            .any(|record| !is_terminal_task_state(record.state))
        {
            return Err("请等待当前任务完成后再更改密码库位置".to_string());
        }
        let current = self.vault.path().to_path_buf();
        let target = directory.join("vault.dat");
        if target == current {
            return Err("密码库已经保存在该位置".to_string());
        }
        if target.exists() || target.with_extension("bak").exists() {
            return Err("目标文件夹中已存在密码库文件，请选择其他文件夹".to_string());
        }
        copy_vault_files(&current, &target)?;
        let new_vault = match VaultStore::open(&target) {
            Ok(vault) => vault,
            Err(error) => {
                remove_vault_files(&target);
                return Err(format!("无法打开新位置的密码库：{error}"));
            }
        };
        if let Err(error) = self
            .settings
            .update(|settings| settings.vault_path = Some(target.clone()))
        {
            remove_vault_files(&target);
            return Err(format!("无法保存密码库位置：{error}"));
        }
        self.vault = Arc::new(new_vault);
        self.vault_error = None;
        self.selected_password = None;
        self.revealed_password = None;
        self.dismiss_password_windows(cx);
        remove_vault_files(&current);
        self.status = if self.vault.state() == VaultState::Locked {
            format!("密码库已移动到 {}，请重新解锁", self.vault.path().display())
        } else {
            format!("密码库已保存到 {}", self.vault.path().display())
        };
        Ok(())
    }

    /// 为当前用户注册 Lumark 资源管理器右键菜单命令。
    pub(crate) fn register_shell_integration(&mut self) {
        match platform::shell::register_current_user() {
            Ok(()) => {
                self.status = "已注册当前用户的文件关联和经典右键菜单".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 移除当前用户的资源管理器右键菜单命令。
    pub(crate) fn unregister_shell_integration(&mut self) {
        match platform::shell::unregister_current_user() {
            Ok(()) => self.status = "已移除当前用户的 Lumark Shell 注册项".to_string(),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 打开 Windows 默认应用页面，不静默修改文件关联。
    pub(crate) fn open_default_apps_page(&mut self) {
        match platform::open_default_apps() {
            Ok(()) => self.status = "已打开 Windows 默认应用设置".to_string(),
            Err(error) => self.status = error.to_string(),
        }
    }
}

/// 复制密码库主文件和可选的损坏恢复备份，先写临时文件再原子替换。
fn copy_vault_files(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if source.exists() {
        let bytes = std::fs::read(source).map_err(|error| error.to_string())?;
        let temp = target.with_extension("move-tmp");
        let _ = std::fs::remove_file(&temp);
        std::fs::write(&temp, bytes).map_err(|error| error.to_string())?;
        if let Err(error) = std::fs::rename(&temp, target) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.to_string());
        }
        let backup = source.with_extension("bak");
        if backup.exists() {
            let _ = std::fs::copy(backup, target.with_extension("bak"));
        }
    }
    Ok(())
}

/// 删除密码库主文件和备份；旧文件删除失败不影响已经迁移的新密码库。
fn remove_vault_files(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("bak"));
}
