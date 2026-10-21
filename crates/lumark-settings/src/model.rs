use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: ThemePreference,
    /// 界面强调色的十六进制值，默认蓝色。
    pub accent_color: String,
    pub language: String,
    pub default_format: String,
    pub default_preset: CompressionPreset,
    pub default_output_dir: Option<PathBuf>,
    /// 自定义密码库文件路径；`None` 表示使用默认位置。
    pub vault_path: Option<PathBuf>,
    pub overwrite_policy: OverwritePolicy,
    pub open_after_extract: bool,
    pub confirm_delete: bool,
    pub last_directory: Option<PathBuf>,
    pub recent_paths: Vec<PathBuf>,
    pub favorites: Vec<PathBuf>,
    pub details_visible: bool,
    pub column_layout: u8,
    pub window_width: f32,
    pub window_height: f32,
    pub sidebar_width: f32,
    pub auto_extract_enabled: bool,
    pub prompt_save_password: bool,
    pub auto_lock_minutes: u32,
    pub archive_edit_backup: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: ThemePreference::System,
            accent_color: "#2563eb".to_string(),
            language: "zh-CN".to_string(),
            default_format: "zip".to_string(),
            default_preset: CompressionPreset::Balanced,
            default_output_dir: None,
            vault_path: None,
            overwrite_policy: OverwritePolicy::Ask,
            open_after_extract: false,
            confirm_delete: true,
            last_directory: None,
            recent_paths: Vec::new(),
            favorites: Vec::new(),
            details_visible: true,
            column_layout: 1,
            window_width: 1280.0,
            window_height: 800.0,
            sidebar_width: 250.0,
            auto_extract_enabled: true,
            prompt_save_password: true,
            auto_lock_minutes: 10,
            archive_edit_backup: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionPreset {
    Speed,
    Balanced,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverwritePolicy {
    Ask,
    Overwrite,
    Skip,
    Rename,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确认新安装默认使用蓝色强调色。
    #[test]
    fn default_accent_color_is_blue() {
        assert_eq!(Settings::default().accent_color, "#2563eb");
    }

    /// 确认缺少新字段的旧设置文件会回落到默认蓝色。
    #[test]
    fn legacy_settings_default_to_blue_accent() {
        let settings: Settings = serde_json::from_str("{}").expect("empty settings");
        assert_eq!(settings.accent_color, "#2563eb");
    }
}
