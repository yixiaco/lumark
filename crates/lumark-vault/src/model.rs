//! 密码库条目、设置、状态和错误模型。

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
/// 密码库生命周期状态。
pub enum VaultState {
    #[default]
    Uninitialized,
    /// 已锁定。
    Locked,
    /// 已解锁。
    Unlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
/// 密码库的匹配规则和自动锁定设置。
pub struct VaultSettings {
    /// 是否自动用匹配密码解压。
    pub auto_extract: bool,
    /// 是否提示保存密码。
    pub prompt_save: bool,
    /// 0 表示不因空闲时间自动锁定。
    pub auto_lock_minutes: u32,
}

impl Default for VaultSettings {
    /// 返回默认密码库设置。
    fn default() -> Self {
        Self {
            auto_extract: true,
            prompt_save: true,
            auto_lock_minutes: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
/// 密码来源，用于界面筛选。
pub enum PasswordSource {
    #[default]
    None,
    /// 手动输入。
    Manual,
    /// 来自密码库。
    Vault(Uuid),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
/// 密码与归档的匹配方式。
pub enum PasswordMatch {
    #[default]
    Any,
    /// 按完整路径匹配。
    Path,
    /// 按通配符匹配。
    Glob,
    /// 按归档指纹匹配。
    Fingerprint,
}

#[derive(Clone, Default, PartialEq, Eq)]
/// 不会被 Debug 输出的敏感字符串。
pub struct SecretString(String);

impl SecretString {
    /// 包装一个敏感字符串。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 临时借用明文。
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// 判断敏感字符串是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 取出明文，调用方负责后续清零。
    pub fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl Drop for SecretString {
    /// 析构时清零敏感明文。
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretString {
    /// Debug 输出固定为脱敏占位。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl Serialize for SecretString {
    /// 把敏感字符串序列化为明文 JSON 字段。
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretString {
    /// 从 JSON 字段读取敏感明文。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 密码库中的一条密码记录。
pub struct PasswordEntry {
    /// 条目唯一 ID。
    pub id: Uuid,
    /// 条目名称。
    pub label: String,
    /// 密码明文。
    pub password: SecretString,
    /// 用于缩小匹配范围的可选精确路径、文件名或通配符。
    pub archive_pattern: Option<String>,
    /// 可选的归档指纹（大小 + 首尾字节哈希）。
    pub archive_fingerprint: Option<String>,
    /// 备注。
    pub note: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
    /// 最近使用时间。
    pub last_used_at: Option<DateTime<Utc>>,
    /// 使用次数。
    pub use_count: u32,
}

impl PasswordEntry {
    /// 包装一个敏感字符串。
    pub fn new(label: impl Into<String>, password: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            password: SecretString::new(password),
            archive_pattern: None,
            archive_fingerprint: None,
            note: String::new(),
            created_at: now,
            updated_at: now,
            last_used_at: None,
            use_count: 0,
        }
    }

    /// 设置归档匹配规则并返回自身。
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.archive_pattern = Some(pattern.into());
        self
    }

    /// 设置归档指纹并返回自身。
    pub fn with_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.archive_fingerprint = Some(fingerprint.into());
        self
    }

    /// 更新最近使用时间。
    pub fn touch_used(&mut self) {
        self.last_used_at = Some(Utc::now());
        self.use_count = self.use_count.saturating_add(1);
        self.updated_at = Utc::now();
    }

    /// 返回用于界面展示的名称。
    pub fn display_name(&self) -> &str {
        if self.label.trim().is_empty() {
            self.archive_pattern.as_deref().unwrap_or("未命名密码")
        } else {
            &self.label
        }
    }
}

#[derive(Debug, thiserror::Error)]
/// 密码库操作的统一错误。
pub enum VaultError {
    #[error("密码库尚未初始化")]
    Uninitialized,
    #[error("密码库已锁定")]
    Locked,
    #[error("密码库文件损坏：{0}")]
    Corrupt(String),
    #[error("主密码错误")]
    InvalidMasterPassword,
    #[error("主密码至少需要 8 个字符")]
    WeakMasterPassword,
    #[error("密码库格式不受支持")]
    UnsupportedFormat,
    #[error("密码条目不存在")]
    EntryNotFound,
    #[error("密码已存在")]
    DuplicatePassword,
    #[error("归档匹配规则无效：{0}")]
    InvalidPattern(String),
    #[error("Windows DPAPI 不可用或调用失败：{0}")]
    Dpapi(String),
    #[error("导入口令错误")]
    InvalidExportPassword,
    #[error("文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("序列化失败：{0}")]
    Serde(#[from] serde_json::Error),
    #[error("加密操作失败")]
    Crypto,
}
