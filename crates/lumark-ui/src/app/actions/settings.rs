//! 设置动作入口。

use super::super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 在当前中英文之间切换界面语言。
    pub(crate) fn toggle_language(&mut self) {
        let next = if self.settings.get().language.starts_with("en") {
            "zh-CN"
        } else {
            "en-US"
        };
        match self
            .settings
            .update(|settings| settings.language = next.to_string())
        {
            Ok(()) => {
                crate::i18n::set_language(next);
                self.status = if next == "en-US" {
                    "Interface language: English".to_string()
                } else {
                    "界面语言：简体中文".to_string()
                };
            }
            Err(error) => self.status = error.to_string(),
        }
    }

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
        self.status = tr("已恢复默认设置", "Defaults restored").to_string();
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
            ThemePreference::System => tr("主题：跟随系统", "Theme: system"),
            ThemePreference::Light => tr("主题：浅色", "Theme: light"),
            ThemePreference::Dark => tr("主题：深色", "Theme: dark"),
        }
        .to_string();
    }

    /// 设置新压缩对话框使用的默认归档格式。
    pub(crate) fn set_default_format(&mut self, format: &str) {
        match self.settings.update(|settings| {
            settings.default_format = format.to_string();
        }) {
            Ok(()) => {
                self.status = format!(
                    "{}{}",
                    tr("默认格式：", "Default format: "),
                    format.to_ascii_uppercase()
                )
            }
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
                    "{}{}",
                    tr("压缩预设：", "Compression preset: "),
                    match preset {
                        SettingsPreset::Speed => tr("速度优先", "Speed first"),
                        SettingsPreset::Balanced => tr("平衡", "Balanced"),
                        SettingsPreset::Size => tr("体积优先", "Smaller first"),
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
                    tr(
                        "解压完成后将打开目标目录",
                        "Open the output folder after extraction",
                    )
                    .to_string()
                } else {
                    tr(
                        "解压完成后不自动打开目录",
                        "Do not open the output folder after extraction",
                    )
                    .to_string()
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
                    tr(
                        "归档编辑前将保留 .lumark.bak",
                        "Keep .lumark.bak before archive edits",
                    )
                    .to_string()
                } else {
                    tr(
                        "归档编辑不保留额外备份",
                        "Do not keep extra backups for archive edits",
                    )
                    .to_string()
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
                    "{}{}",
                    tr("解压覆盖策略：", "Overwrite policy: "),
                    match policy {
                        lumark_settings::OverwritePolicy::Ask =>
                            tr("询问/默认覆盖", "Ask / default overwrite"),
                        lumark_settings::OverwritePolicy::Overwrite => tr("覆盖", "Overwrite"),
                        lumark_settings::OverwritePolicy::Skip => tr("跳过", "Skip"),
                        lumark_settings::OverwritePolicy::Rename => tr("重命名", "Rename"),
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
                    tr(
                        "新压缩默认启用伪装",
                        "Disguise enabled by default for new archives",
                    )
                    .to_string()
                } else {
                    tr(
                        "新压缩默认不启用伪装",
                        "Disguise disabled by default for new archives",
                    )
                    .to_string()
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
                self.status = format!(
                    "{}{}",
                    tr("默认伪装面具：", "Default disguise mask: "),
                    disguise_mask_label(preference)
                );
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
                self.status = format!(
                    "{}{}",
                    tr("自定义伪装面具：", "Custom disguise mask: "),
                    path.display()
                );
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
            return Err(tr(
                "请等待当前任务完成后再更改密码库位置",
                "Wait for current tasks to finish before moving the vault",
            )
            .to_string());
        }
        let current = self.vault.path().to_path_buf();
        let target = directory.join("vault.dat");
        if target == current {
            return Err(tr(
                "密码库已经保存在该位置",
                "The vault is already stored there",
            )
            .to_string());
        }
        if target.exists() || target.with_extension("bak").exists() {
            return Err(tr(
                "目标文件夹中已存在密码库文件，请选择其他文件夹",
                "A vault already exists in the target folder; choose another",
            )
            .to_string());
        }
        copy_vault_files(&current, &target)?;
        let new_vault = match VaultStore::open(&target) {
            Ok(vault) => vault,
            Err(error) => {
                remove_vault_files(&target);
                return Err(format!(
                    "{}{error}",
                    tr(
                        "无法打开新位置的密码库：",
                        "Cannot open the vault at the new location: "
                    )
                ));
            }
        };
        if let Err(error) = self
            .settings
            .update(|settings| settings.vault_path = Some(target.clone()))
        {
            remove_vault_files(&target);
            return Err(format!(
                "{}{error}",
                tr("无法保存密码库位置：", "Cannot save vault location: ")
            ));
        }
        self.vault = Arc::new(new_vault);
        self.vault_error = None;
        self.selected_password = None;
        self.revealed_password = None;
        self.dismiss_password_windows(cx);
        remove_vault_files(&current);
        self.status = if self.vault.state() == VaultState::Locked {
            format!(
                "{}{}{}",
                tr("密码库已移动到 ", "Vault moved to "),
                self.vault.path().display(),
                tr("，请重新解锁", "; unlock it again")
            )
        } else {
            format!(
                "{}{}",
                tr("密码库已保存到 ", "Vault saved to "),
                self.vault.path().display()
            )
        };
        Ok(())
    }

    /// 为当前用户注册 Lumark 资源管理器右键菜单命令。
    pub(crate) fn register_shell_menu(&mut self) {
        match platform::shell::register_shell_menu() {
            Ok(()) => {
                self.status = tr(
                    "已注册当前用户的资源管理器右键菜单",
                    "Registered Explorer context menu for the current user",
                )
                .to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 移除当前用户的资源管理器右键菜单命令。
    pub(crate) fn unregister_shell_menu(&mut self) {
        match platform::shell::unregister_shell_menu() {
            Ok(()) => {
                self.status = tr(
                    "已移除当前用户的资源管理器右键菜单",
                    "Removed Explorer context menu for the current user",
                )
                .to_string();
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
                    self.status = tr(
                        "文件关联写入完成，但未检测到完整的 Lumark 注册项",
                        "Associations written, but no complete Lumark registration was detected",
                    )
                    .to_string();
                    return;
                }
                self.status = match platform::open_default_apps_for_lumark() {
                    Ok(()) => format!(
                        "{}{count}{}",
                        tr("已注册 ", "Registered "),
                        tr(
                            " 种压缩格式，请在 Windows 设置中为每种格式选择 Lumark",
                            " archive formats; choose Lumark for each in Windows Settings"
                        )
                    ),
                    Err(error) => {
                        format!(
                            "{}{count}{}{error}",
                            tr("已注册 ", "Registered "),
                            tr(
                                " 种压缩格式，但无法打开 Windows 设置：",
                                " archive formats, but Windows Settings could not open: "
                            )
                        )
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
                    tr("已移除当前用户的文件关联；如仍由 Lumark 打开，请在默认应用中改选", "Removed file associations for the current user; change the default app if Lumark still opens them").to_string();
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
            Ok(()) => {
                self.status = tr(
                    "已打开 Windows 默认应用设置",
                    "Opened Windows default apps settings",
                )
                .to_string()
            }
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
