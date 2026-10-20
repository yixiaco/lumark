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
