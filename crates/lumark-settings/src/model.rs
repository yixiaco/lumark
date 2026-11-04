//! 用户设置的数据模型和默认值。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 文件表逐列拖动后的宽度；单位为逻辑像素。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ColumnWidths {
    /// 大小列宽。
    pub size: f32,
    /// 修改时间列宽。
    pub modified: f32,
    /// 类型列宽。
    pub kind: f32,
    /// 压缩后大小列宽。
    pub compressed: f32,
    /// 压缩算法列宽。
    pub algorithm: f32,
    /// CRC32 列宽。
    pub crc32: f32,
    /// 归档内路径列宽。
    pub path: f32,
}

impl Default for ColumnWidths {
    /// 返回标准列宽预设。
    fn default() -> Self {
        Self {
            size: 88.0,
            modified: 142.0,
            kind: 72.0,
            compressed: 80.0,
            algorithm: 76.0,
            crc32: 72.0,
            path: 150.0,
        }
    }
}

impl ColumnWidths {
    /// 按三档预设生成初始列宽。
    pub fn from_layout(layout: u8) -> Self {
        let (size, modified, kind) = match layout {
            0 => (72.0, 126.0, 56.0),
            2 => (110.0, 160.0, 92.0),
            _ => (88.0, 142.0, 72.0),
        };
        Self {
            size,
            modified,
            kind,
            ..Self::default()
        }
    }

    /// 把单列宽度限制在可读范围内，并返回实际采用的宽度。
    pub fn clamp_column(&mut self, column: ColumnKind, width: f32) -> f32 {
        let (minimum, maximum) = column.bounds();
        let width = width.clamp(minimum, maximum);
        match column {
            ColumnKind::Size => self.size = width,
            ColumnKind::Modified => self.modified = width,
            ColumnKind::Kind => self.kind = width,
            ColumnKind::Compressed => self.compressed = width,
            ColumnKind::Algorithm => self.algorithm = width,
            ColumnKind::Crc32 => self.crc32 = width,
            ColumnKind::Path => self.path = width,
        }
        width
    }

    /// 返回指定列的当前宽度。
    pub fn width(self, column: ColumnKind) -> f32 {
        match column {
            ColumnKind::Size => self.size,
            ColumnKind::Modified => self.modified,
            ColumnKind::Kind => self.kind,
            ColumnKind::Compressed => self.compressed,
            ColumnKind::Algorithm => self.algorithm,
            ColumnKind::Crc32 => self.crc32,
            ColumnKind::Path => self.path,
        }
    }
}

/// 文件表可拖动的列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    /// 大小列。
    Size,
    /// 修改时间列。
    Modified,
    /// 类型列。
    Kind,
    /// 压缩后大小列。
    Compressed,
    /// 压缩算法列。
    Algorithm,
    /// CRC32 列。
    Crc32,
    /// 归档内路径列。
    Path,
}

impl ColumnKind {
    /// 返回该列允许的最小和最大宽度。
    pub fn bounds(self) -> (f32, f32) {
        match self {
            Self::Size => (56.0, 180.0),
            Self::Modified => (90.0, 260.0),
            Self::Kind => (48.0, 180.0),
            Self::Compressed => (60.0, 160.0),
            Self::Algorithm => (60.0, 160.0),
            Self::Crc32 => (56.0, 140.0),
            Self::Path => (100.0, 320.0),
        }
    }
}

/// 一次邮件发送结果，用于设置页的最近发送记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MailHistoryEntry {
    /// 本地时间，格式为 `MM-DD HH:MM`。
    pub time: String,
    /// 邮件主题。
    pub subject: String,
    /// 是否发送成功。
    pub success: bool,
    /// 失败时的错误文本。
    pub error: Option<String>,
}

/// 一封等待重试的邮件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MailQueueEntry {
    /// 稳定 ID，用于重试完成后移除对应记录。
    pub id: String,
    /// 创建时间，格式为 `MM-DD HH:MM`。
    pub created_at: String,
    /// 邮件主题。
    pub subject: String,
    /// 邮件正文。
    pub body: String,
    /// 已失败的发送次数。
    pub attempts: u32,
    /// 最近一次错误。
    pub last_error: Option<String>,
}

/// AES 密钥来源。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AesKeySource {
    /// 尚未设置。
    #[default]
    None,
    /// 从 Shipping.exe 自动提取。
    Extracted,
    /// 用户手动输入。
    Manual,
}

/// 单个游戏的默认导出配置（E-1）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameExportConfig {
    /// 默认输出目录；`None` 表示 `<游戏目录>/LumarkExports`。
    pub output_dir: Option<PathBuf>,
    /// 目标格式：`glb`/`uemodel`/`usda`/`psk`。
    pub formats: Vec<String>,
    /// 资产路径子串过滤。
    pub asset_filter: Option<String>,
    /// 最多导出资产数。
    pub limit: Option<usize>,
    /// 是否覆盖已有文件。
    pub overwrite: bool,
    /// 是否解析材质。
    pub include_materials: bool,
    /// 贴图最大边长。
    pub max_texture_size: u32,
    /// 位置缩放。
    pub scale: f32,
}

impl Default for GameExportConfig {
    fn default() -> Self {
        Self {
            output_dir: None,
            formats: vec!["glb".to_string(), "uemodel".to_string()],
            asset_filter: None,
            limit: None,
            overwrite: false,
            include_materials: true,
            max_texture_size: 512,
            scale: 1.0,
        }
    }
}

impl GameExportConfig {
    /// 规范化：格式去重/排序、只保留支持格式，数值限幅。
    pub fn normalized(mut self) -> Self {
        self.formats = self
            .formats
            .into_iter()
            .map(|format| format.to_ascii_lowercase())
            .filter(|format| matches!(format.as_str(), "glb" | "uemodel" | "usda" | "psk"))
            .collect();
        self.formats.sort();
        self.formats.dedup();
        if self.formats.is_empty() {
            self.formats = vec!["glb".to_string()];
        }
        self.max_texture_size = self.max_texture_size.clamp(16, 8192);
        self.scale = self.scale.clamp(0.0001, 100.0);
        if self.asset_filter.as_deref().is_some_and(str::is_empty) {
            self.asset_filter = None;
        }
        self
    }
}

/// 游戏资产库中的一条游戏记录。
///
/// `aes_key` 只保存 Windows DPAPI 加密后的 blob，不保存明文密钥；
/// UI 在本次会话内解密后仅保留在内存中。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameLibraryEntry {
    /// 稳定 ID（UUID）。
    pub id: String,
    /// 界面展示名称。
    pub name: String,
    /// 用户选择的游戏安装根目录。
    pub root: PathBuf,
    /// 引擎版本文本，例如 `5.6`。
    pub engine: String,
    /// 引擎版本是否由用户手动指定。
    pub engine_manual: bool,
    /// DPAPI 保护的 AES 密钥 blob。
    pub aes_key: Option<String>,
    /// 密钥来源。
    pub key_source: AesKeySource,
    /// 用户选择的 Oodle 动态库路径。
    pub oodle_dll: Option<PathBuf>,
    /// 用户选择的 `.usmap` 映射文件，用于无版本属性。
    pub mappings: Option<PathBuf>,
    /// 每游戏默认导出配置。
    pub export: GameExportConfig,
    /// 最近打开时间的 RFC3339 文本。
    pub last_opened: Option<String>,
}

/// Lumark 的用户设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// 用户逐列拖动后的列宽；`None` 表示按 `column_layout` 预设显示。
    pub column_widths: Option<ColumnWidths>,
    /// 主窗口宽度。
    pub window_width: f32,
    /// 主窗口高度。
    pub window_height: f32,
    /// 侧栏宽度。
    pub sidebar_width: f32,
    /// 侧栏详细信息面板高度。
    pub details_height: f32,
    /// 是否强制使用内置纯 Rust Oodle 解码器（移除 DLL 后使用）。
    pub oodle_prefer_rust: bool,
    /// 是否自动解压单文件归档。
    pub auto_extract_enabled: bool,
    /// 是否提示保存密码。
    pub prompt_save_password: bool,
    /// 密码库自动锁定分钟数。
    pub auto_lock_minutes: u32,
    /// 归档编辑是否默认保留备份。
    pub archive_edit_backup: bool,
    /// 压缩成功后是否发送邮件通知。
    pub email_notify: bool,
    /// SMTP 主机。
    pub smtp_host: String,
    /// SMTP 端口。
    pub smtp_port: u16,
    /// SMTP 连接和命令超时秒数。
    pub smtp_timeout_secs: u64,
    /// SMTP 登录用户名。
    pub smtp_username: String,
    /// DPAPI 保护的 SMTP 密码；`None` 表示未设置。
    pub smtp_password: Option<String>,
    /// 发件人地址。
    pub smtp_from: String,
    /// 收件人地址，多个地址用逗号或分号分隔。
    pub smtp_to: String,
    /// 使用隐式 TLS（常见端口 465）。
    pub smtp_implicit_tls: bool,
    /// 使用 STARTTLS（常见端口 587）。
    pub smtp_starttls: bool,
    /// 使用 XOAUTH2；没有刷新令牌时 SMTP 密码字段保存访问令牌。
    pub smtp_xoauth2: bool,
    /// OAuth2 令牌端点。
    pub smtp_oauth2_token_url: String,
    /// OAuth2 客户端 ID。
    pub smtp_oauth2_client_id: String,
    /// DPAPI 保护的 OAuth2 客户端密钥。
    pub smtp_oauth2_client_secret: Option<String>,
    /// DPAPI 保护的 OAuth2 刷新令牌。
    pub smtp_oauth2_refresh_token: Option<String>,
    /// 最近发送记录，最新在前。
    pub mail_history: Vec<MailHistoryEntry>,
    /// 等待重试的压缩完成通知。
    pub mail_outbox: Vec<MailQueueEntry>,
    /// 新建压缩窗口默认是否勾选伪装输出。
    pub disguise_enabled: bool,
    /// 默认面具类型。
    pub disguise_mask: DisguiseMaskPreference,
    /// 自定义面具文件路径；仅 `disguise_mask == Custom` 时使用。
    pub disguise_custom_mask: Option<PathBuf>,
    /// 游戏资产库。
    pub games: Vec<GameLibraryEntry>,
    /// 当前游戏资产库选中的游戏 ID。
    pub active_game: Option<String>,
    /// 上次打开的游戏 ID 列表；下次进入游戏资产时尝试恢复。
    pub open_games: Vec<String>,
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
            column_widths: None,
            window_width: 1280.0,
            window_height: 800.0,
            sidebar_width: 250.0,
            details_height: 190.0,
            oodle_prefer_rust: false,
            auto_extract_enabled: true,
            prompt_save_password: true,
            auto_lock_minutes: 10,
            archive_edit_backup: false,
            email_notify: false,
            smtp_host: String::new(),
            smtp_port: 465,
            smtp_timeout_secs: 30,
            smtp_username: String::new(),
            smtp_password: None,
            smtp_from: String::new(),
            smtp_to: String::new(),
            smtp_implicit_tls: true,
            smtp_starttls: false,
            smtp_xoauth2: false,
            smtp_oauth2_token_url: String::new(),
            smtp_oauth2_client_id: String::new(),
            smtp_oauth2_client_secret: None,
            smtp_oauth2_refresh_token: None,
            mail_history: Vec::new(),
            mail_outbox: Vec::new(),
            disguise_enabled: false,
            disguise_mask: DisguiseMaskPreference::Video,
            disguise_custom_mask: None,
            games: Vec::new(),
            active_game: None,
            open_games: Vec::new(),
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

    /// 验证邮件通知默认关闭、默认 465 隐式 TLS，旧设置文件可回退。
    #[test]
    fn mail_notification_defaults_are_safe() {
        let settings: Settings = serde_json::from_str("{}").expect("empty settings");
        assert!(!settings.email_notify);
        assert_eq!(settings.smtp_port, 465);
        assert_eq!(settings.smtp_timeout_secs, 30);
        assert!(settings.smtp_implicit_tls);
        assert!(!settings.smtp_starttls);
        assert!(!settings.smtp_xoauth2);
        assert!(settings.smtp_oauth2_client_secret.is_none());
        assert!(settings.smtp_oauth2_refresh_token.is_none());
        assert!(settings.smtp_password.is_none());
        assert!(settings.mail_history.is_empty());
        assert!(settings.mail_outbox.is_empty());
    }

    /// 验证逐列宽度按预设初始化，并被限制在可读范围内。
    #[test]
    fn column_widths_follow_layout_and_clamp() {
        let mut widths = ColumnWidths::from_layout(0);
        assert_eq!(widths.size, 72.0);
        assert_eq!(widths.modified, 126.0);
        let clamped = widths.clamp_column(ColumnKind::Size, 10.0);
        assert_eq!(clamped, 56.0);
        let clamped = widths.clamp_column(ColumnKind::Size, 999.0);
        assert_eq!(clamped, 180.0);
        assert_eq!(widths.width(ColumnKind::Size), 180.0);
    }

    /// 旧设置文件没有游戏库字段时回落到空列表，且游戏记录可往返序列化。
    #[test]
    fn game_library_defaults_and_roundtrip() {
        let settings: Settings = serde_json::from_str("{}").expect("empty settings");
        assert!(settings.games.is_empty());
        assert!(settings.active_game.is_none());
        assert!(settings.open_games.is_empty());
        let entry = GameLibraryEntry {
            id: "game-1".to_string(),
            name: "BNB".to_string(),
            root: PathBuf::from(r"D:\Games\BNB"),
            engine: "5.6".to_string(),
            engine_manual: false,
            aes_key: Some("dpapi-blob".to_string()),
            key_source: AesKeySource::Extracted,
            oodle_dll: Some(PathBuf::from(
                r"D:\Games\FModel\Output\.data\oodle-data-shared.dll",
            )),
            mappings: Some(PathBuf::from(r"D:\Games\FModel\BNB.usmap")),
            export: GameExportConfig::default(),
            last_opened: None,
        };
        let encoded = serde_json::to_string(&entry).unwrap();
        assert!(!encoded.contains("59321AC6"), "设置中不得出现明文密钥");
        let decoded: GameLibraryEntry = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.id, "game-1");
        assert_eq!(decoded.root, PathBuf::from(r"D:\Games\BNB"));
        assert_eq!(decoded.key_source, AesKeySource::Extracted);
        assert_eq!(
            decoded.mappings,
            Some(PathBuf::from(r"D:\Games\FModel\BNB.usmap"))
        );
        assert_eq!(decoded.export, GameExportConfig::default());
    }

    /// 每游戏导出配置默认值、旧 JSON 回退与规范化。
    #[test]
    fn game_export_config_defaults_and_normalizes() {
        let legacy: GameLibraryEntry =
            serde_json::from_str(r#"{"id":"g","name":"G","root":"D:/G"}"#).expect("旧记录");
        assert_eq!(legacy.export.formats, vec!["glb", "uemodel"]);
        assert!(legacy.export.include_materials);
        let config = GameExportConfig {
            formats: vec![
                "PSK".to_string(),
                "glb".to_string(),
                "unknown".to_string(),
                "glb".to_string(),
            ],
            max_texture_size: 1,
            scale: 0.0,
            asset_filter: Some(String::new()),
            ..GameExportConfig::default()
        }
        .normalized();
        assert_eq!(config.formats, vec!["glb", "psk"]);
        assert_eq!(config.max_texture_size, 16);
        assert!(config.scale > 0.0);
        assert!(config.asset_filter.is_none());
    }
}
