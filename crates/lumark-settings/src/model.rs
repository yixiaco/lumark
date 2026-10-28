//! 用户设置的数据模型和默认值。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
/// Lumark 的用户设置。
pub struct Settings {
    /// 设置文件版本。
    pub version: u32,
    /// 主题模式。
    pub theme: ThemePreference,
    /// 界面强调色的十六进制值，默认蓝色。
    pub accent_color: String,
    /// 界面语言。
    pub language: String,
    /// 默认压缩格式。
    pub default_format: String,
    /// 默认压缩预设。
    pub default_preset: CompressionPreset,
    /// 默认输出目录。
    pub default_output_dir: Option<PathBuf>,
    /// 自定义密码库文件路径；`None` 表示使用默认位置。
    pub vault_path: Option<PathBuf>,
    /// 默认解压覆盖策略。
    pub overwrite_policy: OverwritePolicy,
    /// 解压完成后是否打开输出目录。
    pub open_after_extract: bool,
    /// 删除文件前是否确认。
    pub confirm_delete: bool,
    /// 上次浏览目录。
    pub last_directory: Option<PathBuf>,
    /// 最近打开的路径。
    pub recent_paths: Vec<PathBuf>,
    /// 收藏目录。
    pub favorites: Vec<PathBuf>,
    /// 是否显示侧栏详情。
    pub details_visible: bool,
    /// 文件表列宽预设。
    pub column_layout: u8,
    /// 主窗口宽度。
    pub window_width: f32,
    /// 主窗口高度。
    pub window_height: f32,
    /// 侧栏宽度。
    pub sidebar_width: f32,
    /// 是否自动解压单文件归档。
    pub auto_extract_enabled: bool,
    /// 是否提示保存密码。
    pub prompt_save_password: bool,
    /// 密码库自动锁定分钟数。
    pub auto_lock_minutes: u32,
    /// 归档编辑是否默认保留备份。
    pub archive_edit_backup: bool,
    /// 新建压缩窗口默认是否勾选伪装输出。
    pub disguise_enabled: bool,
    /// 默认面具类型。
    pub disguise_mask: DisguiseMaskPreference,
    /// 自定义面具文件路径；仅 `disguise_mask == Custom` 时使用。
    pub disguise_custom_mask: Option<PathBuf>,
}

impl Default for Settings {
    /// 返回默认设置。
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
            disguise_enabled: false,
            disguise_mask: DisguiseMaskPreference::Video,
            disguise_custom_mask: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
/// 界面主题偏好。
pub enum ThemePreference {
    /// 跟随系统。
    System,
    /// 浅色。
    Light,
    /// 深色。
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
/// 默认压缩预设。
pub enum CompressionPreset {
    /// 优先速度。
    Speed,
    /// 平衡。
    Balanced,
    /// 优先体积。
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
/// 默认解压覆盖策略。
pub enum OverwritePolicy {
    /// 每次询问。
    Ask,
    /// 直接覆盖。
    Overwrite,
    /// 跳过。
    Skip,
    /// 自动改名。
    Rename,
}

/// 伪装面具的默认类型。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisguiseMaskPreference {
    /// 内置 MP4 视频面具。
    #[default]
    Video,
    /// 内置 JPG 图片面具。
    Image,
    /// 用户选择的自定义媒体文件。
    Custom,
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
