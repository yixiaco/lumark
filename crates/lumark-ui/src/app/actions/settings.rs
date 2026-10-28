//! 设置动作入口。

use super::super::*;

impl LumarkApp {
    /// 把主窗口带到前台，供设置窗口跳转到密码库等主界面操作。
    pub(crate) fn activate_main_window(&self, cx: &mut gpui_kit::App) {
        let _ = self
            .main_window
            .update(cx, |_, window, _| window.activate_window());
    }

    /// 恢复界面与压缩偏好的出厂值，同时保留路径、窗口尺寸和用户数据。
    pub(crate) fn reset_preferences(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let defaults = Settings::default();
        let vault_path = self.settings.get().vault_path.clone();
        self.settings
            .update(|settings| {
                settings.theme = defaults.theme;
                settings.accent_color = defaults.accent_color.clone();
                settings.language = defaults.language.clone();
                settings.default_format = defaults.default_format.clone();
                settings.default_preset = defaults.default_preset;
                settings.default_output_dir = None;
                settings.overwrite_policy = defaults.overwrite_policy;
                settings.open_after_extract = defaults.open_after_extract;
                settings.confirm_delete = defaults.confirm_delete;
                settings.details_visible = defaults.details_visible;
                settings.column_layout = defaults.column_layout;
                settings.auto_extract_enabled = defaults.auto_extract_enabled;
                settings.prompt_save_password = defaults.prompt_save_password;
                settings.auto_lock_minutes = defaults.auto_lock_minutes;
                settings.archive_edit_backup = defaults.archive_edit_backup;
                settings.disguise_enabled = defaults.disguise_enabled;
                settings.disguise_mask = defaults.disguise_mask;
                settings.disguise_custom_mask = defaults.disguise_custom_mask.clone();
                settings.vault_path = vault_path;
            })
            .map_err(|error| error.to_string())?;
        self.details_visible = defaults.details_visible;
        self.column_layout = defaults.column_layout.min(2);
        self.compress_disguise = defaults.disguise_enabled;
        self.compress_disguise_mask = defaults.disguise_mask;
        self.compress_disguise_custom = defaults.disguise_custom_mask.clone();
        apply_theme(defaults.theme, &defaults.accent_color, window, cx);
        self.status = "已恢复默认设置".to_string();
        Ok(())
    }

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

    /// 设置新压缩对话框使用的默认归档格式。
    pub(crate) fn set_default_format(&mut self, format: &str) {
        match self.settings.update(|settings| {
            settings.default_format = format.to_string();
        }) {
            Ok(()) => self.status = format!("默认格式：{}", format.to_ascii_uppercase()),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 设置新任务使用的默认压缩预设。
    pub(crate) fn set_default_preset(&mut self, preset: SettingsPreset) {
        match self
            .settings
            .update(|settings| settings.default_preset = preset)
        {
            Ok(()) => {
                self.status = format!(
                    "压缩预设：{}",
                    match preset {
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

    /// 设置解压时的默认冲突策略。
    pub(crate) fn set_overwrite_policy(&mut self, policy: lumark_settings::OverwritePolicy) {
        match self
            .settings
            .update(|settings| settings.overwrite_policy = policy)
        {
            Ok(()) => {
                self.status = format!(
                    "解压覆盖策略：{}",
                    match policy {
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

    /// 切换新压缩窗口默认是否启用伪装输出。
    pub(crate) fn set_disguise_enabled(&mut self, enabled: bool) {
        match self
            .settings
            .update(|settings| settings.disguise_enabled = enabled)
        {
            Ok(()) => {
                self.compress_disguise = enabled;
                self.status = if enabled {
                    "新压缩默认启用伪装".to_string()
                } else {
                    "新压缩默认不启用伪装".to_string()
                };
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 设置新压缩窗口默认使用的面具类型。
    pub(crate) fn set_disguise_mask(&mut self, preference: DisguiseMaskPreference) {
        match self
            .settings
            .update(|settings| settings.disguise_mask = preference)
        {
            Ok(()) => {
                self.compress_disguise_mask = preference;
                self.status = format!("默认伪装面具：{}", disguise_mask_label(preference));
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 设置自定义面具文件，并自动把默认面具类型切到“自定义文件”。
    pub(crate) fn set_disguise_custom_mask(&mut self, path: PathBuf) {
        match self.settings.update(|settings| {
            settings.disguise_custom_mask = Some(path.clone());
            settings.disguise_mask = DisguiseMaskPreference::Custom;
        }) {
            Ok(()) => {
                self.compress_disguise_custom = Some(path.clone());
                self.compress_disguise_mask = DisguiseMaskPreference::Custom;
                self.status = format!("自定义伪装面具：{}", path.display());
            }
            Err(error) => self.status = error.to_string(),
        }
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
    pub(crate) fn register_shell_menu(&mut self) {
        match platform::shell::register_shell_menu() {
            Ok(()) => {
                self.status = "已注册当前用户的资源管理器右键菜单".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 移除当前用户的资源管理器右键菜单命令。
    pub(crate) fn unregister_shell_menu(&mut self) {
        match platform::shell::unregister_shell_menu() {
            Ok(()) => {
                self.status = "已移除当前用户的资源管理器右键菜单".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 按用户勾选的格式注册文件关联，并打开 Windows 的 Lumark 默认应用页面。
    pub(crate) fn apply_file_associations(&mut self, extensions: Vec<String>) {
        if extensions.is_empty() {
            self.unregister_file_associations();
            return;
        }
        match platform::shell::register_file_associations(&extensions) {
            Ok(()) => {
                self.file_association_count = platform::shell::registered_extension_count();
                let count = self.file_association_count;
                if count == 0 {
                    self.status = "文件关联写入完成，但未检测到完整的 Lumark 注册项".to_string();
                    return;
                }
                self.status = match platform::open_default_apps_for_lumark() {
                    Ok(()) => format!(
                        "已注册 {count} 种压缩格式，请在 Windows 设置中为每种格式选择 Lumark"
                    ),
                    Err(error) => {
                        format!("已注册 {count} 种压缩格式，但无法打开 Windows 设置：{error}")
                    }
                };
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 移除压缩格式的文件关联注册项。
    pub(crate) fn unregister_file_associations(&mut self) {
        match platform::shell::unregister_file_associations() {
            Ok(()) => {
                self.file_association_count = 0;
                self.status =
                    "已移除当前用户的文件关联；如仍由 Lumark 打开，请在默认应用中改选".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 打开 Windows 默认应用页面，不静默修改默认打开方式。
    pub(crate) fn open_default_apps_page(&mut self) {
        let result = if self.file_association_count > 0 {
            platform::open_default_apps_for_lumark().or_else(|_| platform::open_default_apps())
        } else {
            platform::open_default_apps()
        };
        match result {
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
