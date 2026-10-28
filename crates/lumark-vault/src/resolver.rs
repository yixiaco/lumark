//! 归档密码候选排序和匹配。

use std::path::{Path, PathBuf};

use blake3::Hasher;
use globset::Glob;

use crate::model::{PasswordEntry, SecretString, VaultError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// 密码候选的优先级别。
pub enum CandidatePriority {
    /// 完整路径精确匹配。
    ExactPath = 0,
    /// 归档指纹匹配。
    Fingerprint = 1,
    /// 通配规则匹配。
    Pattern = 2,
    /// 最近使用。
    Recent = 3,
    /// 通用候选。
    Generic = 4,
}

#[derive(Debug, Clone)]
/// 带优先级和匹配原因的密码候选。
pub struct PasswordCandidate {
    /// 密码库条目 ID。
    pub entry_id: uuid::Uuid,
    /// 候选展示名称。
    pub label: String,
    /// 候选密码。
    pub password: SecretString,
    /// 候选优先级。
    pub priority: CandidatePriority,
}

/// 按归档路径、指纹和匹配规则排序密码候选。
pub struct PasswordResolver;

impl PasswordResolver {
    /// 生成去重后的密码候选列表。
    pub fn candidates(
        entries: &[PasswordEntry],
        archive_path: &Path,
        fingerprint: Option<&str>,
    ) -> Vec<PasswordCandidate> {
        let path = normalize_path(archive_path);
        let file_name = archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_lowercase();

        let mut ordered: Vec<(CandidatePriority, PasswordCandidate)> = Vec::new();
        for entry in entries {
            let priority = match (&entry.archive_pattern, &entry.archive_fingerprint) {
                (Some(pattern), _) if pattern_matches(pattern, &path, &file_name) => {
                    if normalize_path(Path::new(pattern)) == path {
                        CandidatePriority::ExactPath
                    } else {
                        CandidatePriority::Pattern
                    }
                }
                (_, Some(entry_fingerprint))
                    if fingerprint.is_some_and(|value| value == entry_fingerprint) =>
                {
                    CandidatePriority::Fingerprint
                }
                (None, None) => {
                    if entry.last_used_at.is_some() {
                        CandidatePriority::Recent
                    } else {
                        CandidatePriority::Generic
                    }
                }
                _ => continue,
            };
            ordered.push((
                priority,
                PasswordCandidate {
                    entry_id: entry.id,
                    label: entry.display_name().to_string(),
                    password: entry.password.clone(),
                    priority,
                },
            ));
        }

        ordered.sort_by(|(left_priority, left), (right_priority, right)| {
            left_priority
                .cmp(right_priority)
                .then_with(|| left.label.cmp(&right.label))
        });

        let mut seen = Vec::<String>::new();
        let mut result = Vec::new();
        for (_, candidate) in ordered {
            let value = candidate.password.expose().to_string();
            if seen.iter().any(|existing| existing == &value) {
                continue;
            }
            seen.push(value);
            result.push(candidate);
        }
        result
    }

    /// 计算归档路径和内容相关的指纹。
    pub fn archive_fingerprint(path: &Path) -> Result<String, VaultError> {
        let metadata = std::fs::metadata(path)?;
        let mut hasher = Hasher::new();
        hasher.update(&metadata.len().to_le_bytes());
        let mut file = std::fs::File::open(path)?;
        let mut head = vec![0u8; 64 * 1024];
        let head_len = std::io::Read::read(&mut file, &mut head)?;
        hasher.update(&head[..head_len]);

        let tail_len = metadata.len().min(64 * 1024) as usize;
        if tail_len > 0 {
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::End(-(tail_len as i64)))?;
            let mut tail = vec![0u8; tail_len];
            std::io::Read::read_exact(&mut file, &mut tail)?;
            hasher.update(&tail);
        }
        Ok(hasher.finalize().to_hex().to_string())
    }
}

/// 规范化路径以便跨大小写和分隔符比较。
fn normalize_path(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    while value.ends_with('\\') && value.len() > 1 {
        value.pop();
    }
    value.to_lowercase()
}

/// 判断归档路径是否匹配密码记录规则。
fn pattern_matches(pattern: &str, full_path: &str, file_name: &str) -> bool {
    let normalized = normalize_path(Path::new(pattern));
    if normalized == full_path {
        return true;
    }
    if normalized == file_name {
        return true;
    }
    if let Ok(glob) = Glob::new(&normalized) {
        let matcher = glob.compile_matcher();
        return matcher.is_match(full_path) || matcher.is_match(file_name);
    }
    false
}

/// 去掉分卷后缀后计算逻辑归档指纹。
pub fn split_volume_fingerprint(paths: &[PathBuf]) -> Result<String, VaultError> {
    let mut hasher = Hasher::new();
    for path in paths {
        let metadata = std::fs::metadata(path)?;
        hasher.update(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update(&metadata.len().to_le_bytes());
    }
    if let Some(first) = paths.first() {
        let mut file = std::fs::File::open(first)?;
        let mut head = vec![0u8; 64 * 1024];
        let len = std::io::Read::read(&mut file, &mut head)?;
        hasher.update(&head[..len]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
