use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: ThemePreference,
    pub language: String,
    pub default_format: String,
    pub default_preset: CompressionPreset,
    pub default_output_dir: Option<PathBuf>,
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
            language: "zh-CN".to_string(),
            default_format: "zip".to_string(),
            default_preset: CompressionPreset::Balanced,
            default_output_dir: None,
            overwrite_policy: OverwritePolicy::Ask,
            open_after_extract: false,
            confirm_delete: true,
            last_directory: None,
            recent_paths: Vec::new(),
            favorites: Vec::new(),
            details_visible: false,
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
