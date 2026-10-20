use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VaultState {
    #[default]
    Uninitialized,
    Locked,
    Unlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSettings {
    pub auto_extract: bool,
    pub prompt_save: bool,
    /// 0 表示不因空闲时间自动锁定。
    pub auto_lock_minutes: u32,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            auto_extract: true,
            prompt_save: true,
            auto_lock_minutes: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordSource {
    #[default]
    None,
    Manual,
    Vault(Uuid),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordMatch {
    #[default]
    Any,
    Path,
    Glob,
    Fingerprint,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl Serialize for SecretString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordEntry {
    pub id: Uuid,
    pub label: String,
    pub password: SecretString,
    /// 用于缩小匹配范围的可选精确路径、文件名或通配符。
    pub archive_pattern: Option<String>,
    /// 可选的归档指纹（大小 + 首尾字节哈希）。
    pub archive_fingerprint: Option<String>,
    pub note: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: u32,
}

impl PasswordEntry {
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

    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.archive_pattern = Some(pattern.into());
        self
    }

    pub fn with_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.archive_fingerprint = Some(fingerprint.into());
        self
    }

    pub fn touch_used(&mut self) {
        self.last_used_at = Some(Utc::now());
        self.use_count = self.use_count.saturating_add(1);
        self.updated_at = Utc::now();
    }

    pub fn display_name(&self) -> &str {
        if self.label.trim().is_empty() {
            self.archive_pattern.as_deref().unwrap_or("未命名密码")
        } else {
            &self.label
        }
    }
}

#[derive(Debug, thiserror::Error)]
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
