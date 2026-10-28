//! 密码库解锁、持久化、自动锁定和 CRUD。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::RwLock;
use uuid::Uuid;

use crate::crypto::{DecryptedVault, VaultCrypto, VaultFile};
use crate::model::{PasswordEntry, VaultError, VaultSettings, VaultState};
use crate::resolver::{PasswordCandidate, PasswordResolver};

/// 密码库的内存状态：解锁密钥、条目、设置、活动时间和文件戳。
struct VaultInner {
    state: VaultState,
    file: Option<VaultFile>,
    entries: Vec<PasswordEntry>,
    settings: VaultSettings,
    key: Option<[u8; 32]>,
    last_activity: Instant,
    leases: u32,
    file_stamp: Option<(u64, u64)>,
}

/// 判断密码库是否需要主密码才能重新解锁。
fn requires_master_password(inner: &VaultInner) -> bool {
    inner
        .file
        .as_ref()
        .is_some_and(|file| matches!(file.key_wrap, crate::crypto::KeyWrap::MasterPassword { .. }))
}

/// 线程安全的密码库存储，负责解锁、持久化和自动锁定。
pub struct VaultStore {
    path: PathBuf,
    inner: RwLock<VaultInner>,
}

impl VaultStore {
    /// 返回当前用户的默认密码库路径。
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Lumark")
            .join("vault")
            .join("vault.dat")
    }

    /// 打开或创建密码库，并自动处理损坏恢复和 DPAPI 迁移。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, VaultError> {
        let path = path.into();
        let mut inner = VaultInner {
            state: VaultState::Uninitialized,
            file: None,
            entries: Vec::new(),
            settings: VaultSettings::default(),
            key: None,
            last_activity: Instant::now(),
            leases: 0,
            file_stamp: file_stamp(&path),
        };

        if path.exists() {
            let mut file = match VaultCrypto::read_file(&path) {
                Ok(file) => file,
                Err(_) => recover_corrupt_vault(&path)?,
            };
            if matches!(file.key_wrap, crate::crypto::KeyWrap::Dpapi { .. }) {
                let decrypted = VaultCrypto::unlock(&file, None)?;
                let (entries, settings, key) = decrypted.into_parts();
                inner.state = VaultState::Unlocked;
                inner.settings = settings;
                inner.entries = entries;
                inner.key = Some(key);
                if matches!(
                    file.key_wrap,
                    crate::crypto::KeyWrap::Dpapi {
                        machine_bound: false,
                        ..
                    }
                ) {
                    // 旧版 DPAPI 仅绑定当前用户；升级后改为计算机绑定并原子重写。
                    // 迁移失败时保留旧文件继续使用，避免只读目录导致密码库不可用。
                    match VaultCrypto::set_dpapi(&mut file, &key)
                        .and_then(|()| VaultCrypto::write_file(&path, &file))
                    {
                        Ok(()) => inner.file_stamp = file_stamp(&path),
                        Err(error) => eprintln!("vault DPAPI migration failed: {error}"),
                    }
                }
            } else {
                inner.state = VaultState::Locked;
            }
            inner.file = Some(file);
        }

        Ok(Self {
            path,
            inner: RwLock::new(inner),
        })
    }

    /// 返回密码库文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 返回当前锁定状态。
    pub fn state(&self) -> VaultState {
        self.inner.read().state
    }

    /// 判断密码库是否已解锁。
    pub fn is_unlocked(&self) -> bool {
        self.state() == VaultState::Unlocked
    }

    /// 判断是否需要主密码才能解锁。
    pub fn has_master_password(&self) -> bool {
        self.inner.read().file.as_ref().is_some_and(|file| {
            matches!(file.key_wrap, crate::crypto::KeyWrap::MasterPassword { .. })
        })
    }

    /// 返回当前内存中的条目数。
    pub fn entry_count(&self) -> usize {
        let inner = self.inner.read();
        inner.entries.len().max(
            inner
                .file
                .as_ref()
                .map_or(0, |file| file.entry_count as usize),
        )
    }

    /// 返回密码库设置副本。
    pub fn settings(&self) -> VaultSettings {
        self.inner.read().settings
    }

    /// 用主密码解锁密码库并刷新内存状态。
    pub fn unlock(&self, master_password: &str) -> Result<(), VaultError> {
        let mut inner = self.inner.write();
        let file = inner.file.as_ref().ok_or(VaultError::Uninitialized)?;
        let decrypted = VaultCrypto::unlock(file, Some(master_password))?;
        apply_decrypted(&mut inner, decrypted);
        inner.file_stamp = file_stamp(&self.path);
        Ok(())
    }

    /// 未解锁时返回统一错误，供写操作前置检查。
    pub fn ensure_unlocked(&self) -> Result<(), VaultError> {
        let state = self.state();
        if state == VaultState::Unlocked {
            self.touch();
            return Ok(());
        }
        if state == VaultState::Uninitialized {
            return Ok(());
        }
        Err(VaultError::Locked)
    }

    /// 锁定密码库并清零内存中的会话密钥。
    pub fn lock(&self) {
        let mut inner = self.inner.write();
        if !requires_master_password(&inner) {
            return;
        }
        inner.entries.clear();
        if let Some(mut key) = inner.key.take() {
            zeroize::Zeroize::zeroize(&mut key);
        }
        inner.state = if inner.file.is_some() {
            VaultState::Locked
        } else {
            VaultState::Uninitialized
        };
    }

    /// 刷新最近活动时间，延迟自动锁定。
    pub fn touch(&self) {
        self.inner.write().last_activity = Instant::now();
    }

    /// 持有临时租约，阻止自动锁定。
    pub fn acquire_lease(&self) {
        let mut inner = self.inner.write();
        inner.leases = inner.leases.saturating_add(1);
        inner.last_activity = Instant::now();
    }

    /// 释放临时租约。
    pub fn release_lease(&self) {
        let mut inner = self.inner.write();
        inner.leases = inner.leases.saturating_sub(1);
        inner.last_activity = Instant::now();
    }

    /// 超过自动锁定时间且没有租约时锁定密码库。
    pub fn maybe_auto_lock(&self) -> bool {
        let mut inner = self.inner.write();
        if inner.state != VaultState::Unlocked || inner.leases > 0 {
            return false;
        }
        // 没有主密码时密码库由 DPAPI 自动解锁，锁定后无法在本会话内恢复，
        // 因此只对主密码保护的密码库执行空闲锁定和外部修改锁定。
        if !requires_master_password(&inner) {
            return false;
        }
        if let Some(stamp) = file_stamp(&self.path)
            && inner.file_stamp.is_some_and(|current| current != stamp)
        {
            inner.entries.clear();
            if let Some(mut key) = inner.key.take() {
                zeroize::Zeroize::zeroize(&mut key);
            }
            inner.state = VaultState::Locked;
            inner.file_stamp = Some(stamp);
            return true;
        }
        let minutes = inner.settings.auto_lock_minutes;
        if minutes == 0 {
            return false;
        }
        if inner.last_activity.elapsed() >= Duration::from_secs(minutes as u64 * 60) {
            inner.entries.clear();
            if let Some(mut key) = inner.key.take() {
                zeroize::Zeroize::zeroize(&mut key);
            }
            inner.state = VaultState::Locked;
            true
        } else {
            false
        }
    }

    /// 返回条目列表副本。
    pub fn entries(&self) -> Vec<PasswordEntry> {
        self.inner.read().entries.clone()
    }

    /// 按 ID 查找条目。
    pub fn find_entry(&self, id: Uuid) -> Option<PasswordEntry> {
        self.inner
            .read()
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    /// 新增条目并立即持久化。
    pub fn add_entry(&self, mut entry: PasswordEntry) -> Result<Uuid, VaultError> {
        self.ensure_unlocked()?;
        validate_pattern(entry.archive_pattern.as_deref())?;
        let mut inner = self.inner.write();
        if inner.file.is_none() && inner.key.is_none() {
            entry.updated_at = Utc::now();
            let (file, key) = VaultCrypto::new_dpapi(vec![entry.clone()], inner.settings)?;
            VaultCrypto::write_file(&self.path, &file)?;
            inner.file = Some(file);
            inner.key = Some(key);
            inner.state = VaultState::Unlocked;
            inner.file_stamp = file_stamp(&self.path);
            let id = entry.id;
            inner.entries = vec![entry];
            return Ok(id);
        }
        if inner
            .entries
            .iter()
            .any(|existing| existing.password.expose() == entry.password.expose())
        {
            return Err(VaultError::DuplicatePassword);
        }
        entry.updated_at = Utc::now();
        let id = entry.id;
        inner.entries.push(entry);
        save_inner(&self.path, &mut inner)?;
        Ok(id)
    }

    /// 更新条目并立即持久化。
    pub fn update_entry(&self, mut entry: PasswordEntry) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        validate_pattern(entry.archive_pattern.as_deref())?;
        let mut inner = self.inner.write();
        if inner
            .entries
            .iter()
            .any(|item| item.id != entry.id && item.password.expose() == entry.password.expose())
        {
            return Err(VaultError::DuplicatePassword);
        }
        let position = inner
            .entries
            .iter()
            .position(|item| item.id == entry.id)
            .ok_or(VaultError::EntryNotFound)?;
        entry.updated_at = Utc::now();
        inner.entries[position] = entry;
        save_inner(&self.path, &mut inner)
    }

    /// 删除条目并立即持久化。
    pub fn remove_entry(&self, id: Uuid) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        let mut inner = self.inner.write();
        let before = inner.entries.len();
        inner.entries.retain(|entry| entry.id != id);
        if inner.entries.len() == before {
            return Err(VaultError::EntryNotFound);
        }
        save_inner(&self.path, &mut inner)
    }

    /// 在一次加密写入中替换整份内存条目列表。
    pub fn replace_entries(&self, entries: Vec<PasswordEntry>) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        for entry in &entries {
            validate_pattern(entry.archive_pattern.as_deref())?;
        }
        for (index, entry) in entries.iter().enumerate() {
            if entries[..index]
                .iter()
                .any(|existing| existing.password.expose() == entry.password.expose())
            {
                return Err(VaultError::DuplicatePassword);
            }
        }
        let mut inner = self.inner.write();
        inner.entries = entries;
        save_inner(&self.path, &mut inner)
    }

    /// 清空全部条目并持久化。
    pub fn clear_entries(&self) -> Result<(), VaultError> {
        self.replace_entries(Vec::new())
    }

    /// 记录条目最近使用时间。
    pub fn mark_used(&self, id: Uuid) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        let mut inner = self.inner.write();
        if let Some(entry) = inner.entries.iter_mut().find(|entry| entry.id == id) {
            entry.touch_used();
            save_inner(&self.path, &mut inner)?;
        }
        Ok(())
    }

    /// 更新密码库设置并持久化。
    pub fn update_settings(&self, settings: VaultSettings) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        let mut inner = self.inner.write();
        inner.settings = settings;
        if inner.file.is_none() && inner.key.is_none() {
            let (file, key) = VaultCrypto::new_dpapi(inner.entries.clone(), inner.settings)?;
            VaultCrypto::write_file(&self.path, &file)?;
            inner.file = Some(file);
            inner.key = Some(key);
            inner.state = VaultState::Unlocked;
            inner.file_stamp = file_stamp(&self.path);
            return Ok(());
        }
        save_inner(&self.path, &mut inner)
    }

    /// 设置或更换主密码，并重新封装会话密钥。
    pub fn set_master_password(
        &self,
        current: Option<&str>,
        new_password: &str,
    ) -> Result<(), VaultError> {
        let mut inner = self.inner.write();
        if inner.file.is_none() && inner.key.is_none() {
            let (file, key) =
                VaultCrypto::new_master(inner.entries.clone(), inner.settings, new_password)?;
            VaultCrypto::write_file(&self.path, &file)?;
            inner.file = Some(file);
            inner.key = Some(key);
            inner.state = VaultState::Unlocked;
            inner.file_stamp = file_stamp(&self.path);
            return Ok(());
        }
        ensure_key(&inner)?;
        if let Some(current) = current
            && let Some(file) = inner.file.as_ref()
        {
            VaultCrypto::verify_master_password(file, current)?;
        }
        let key = inner.key.ok_or(VaultError::Locked)?;
        if let Some(file) = inner.file.as_mut() {
            VaultCrypto::set_master_password(file, &key, new_password)?;
        } else {
            let (file, _) =
                VaultCrypto::new_master(inner.entries.clone(), inner.settings, new_password)?;
            inner.file = Some(file);
        }
        save_inner(&self.path, &mut inner)
    }

    /// 移除主密码并改回 DPAPI 保护。
    pub fn remove_master_password(&self, current: &str) -> Result<(), VaultError> {
        let mut inner = self.inner.write();
        ensure_key(&inner)?;
        let file = inner.file.as_ref().ok_or(VaultError::Uninitialized)?;
        VaultCrypto::verify_master_password(file, current)?;
        let key = inner.key.ok_or(VaultError::Locked)?;
        let file = inner.file.as_mut().ok_or(VaultError::Uninitialized)?;
        VaultCrypto::set_dpapi(file, &key)?;
        save_inner(&self.path, &mut inner)
    }

    /// 为归档生成按匹配度排序的密码候选。
    pub fn candidates(
        &self,
        archive_path: &Path,
        fingerprint: Option<&str>,
    ) -> Vec<PasswordCandidate> {
        let entries = self.inner.read().entries.clone();
        PasswordResolver::candidates(&entries, archive_path, fingerprint)
    }

    /// 导出加密密码库副本。
    pub fn export(&self, path: &Path, passphrase: &str) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        let inner = self.inner.read();
        VaultCrypto::export(path, &inner.entries, &inner.settings, passphrase)
    }

    /// 把当前条目导出为明文 CSV，并返回写入的记录数。
    ///
    /// CSV 没有任何加密保护，调用方必须先向用户说明风险；密码库锁定时
    /// 内存中不保留条目，因此这里和加密导出一样要求处于解锁状态。
    pub fn export_csv(&self, path: &Path) -> Result<usize, VaultError> {
        self.ensure_unlocked()?;
        let inner = self.inner.read();
        let mut content = entries_to_csv(&inner.entries);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let result = std::fs::write(path, content.as_bytes()).map_err(VaultError::from);
        // 明文密码不应留在堆上继续被后续分配复用。
        zeroize::Zeroize::zeroize(&mut content);
        result?;
        Ok(inner.entries.len())
    }

    /// 导入密码库并合并或替换现有条目。
    pub fn import(
        &self,
        path: &Path,
        passphrase: &str,
        replace: bool,
    ) -> Result<usize, VaultError> {
        self.ensure_unlocked()?;
        let (entries, settings) = VaultCrypto::import(path, passphrase)?;
        let mut inner = self.inner.write();
        if replace {
            inner.entries = entries;
        } else {
            for entry in entries {
                if !inner
                    .entries
                    .iter()
                    .any(|existing| existing.password.expose() == entry.password.expose())
                {
                    inner.entries.push(entry);
                }
            }
        }
        inner.settings = settings;
        let count = inner.entries.len();
        save_inner(&self.path, &mut inner)?;
        Ok(count)
    }
}

/// 生成供 Excel 直接打开的 UTF-8 CSV，文件开头带 BOM，行尾使用 CRLF。
fn entries_to_csv(entries: &[PasswordEntry]) -> String {
    let mut output =
        String::from("\u{feff}名称,密码,适用压缩包,备注,创建时间,更新时间,最近使用,使用次数\r\n");
    for entry in entries {
        let fields = [
            entry.label.clone(),
            entry.password.expose().to_string(),
            entry.archive_pattern.clone().unwrap_or_default(),
            entry.note.clone(),
            entry.created_at.to_rfc3339(),
            entry.updated_at.to_rfc3339(),
            entry
                .last_used_at
                .map(|time| time.to_rfc3339())
                .unwrap_or_default(),
            entry.use_count.to_string(),
        ];
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            output.push_str(&csv_field(field));
        }
        output.push_str("\r\n");
    }
    output
}

/// 按 RFC 4180 给包含逗号、引号或换行的字段加引号。
fn csv_field(value: &str) -> String {
    if value
        .chars()
        .any(|character| matches!(character, ',' | '"' | '\r' | '\n'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// 返回会话密钥；未解锁时拒绝访问。
fn ensure_key(inner: &VaultInner) -> Result<(), VaultError> {
    if inner.key.is_some() {
        Ok(())
    } else {
        Err(VaultError::Locked)
    }
}

/// 校验密码匹配规则语法。
fn validate_pattern(pattern: Option<&str>) -> Result<(), VaultError> {
    let Some(pattern) = pattern.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    globset::Glob::new(pattern)
        .map(|_| ())
        .map_err(|_| VaultError::InvalidPattern(pattern.to_string()))
}

/// 把损坏密码库备份后重新初始化，避免启动失败。
fn recover_corrupt_vault(path: &Path) -> Result<VaultFile, VaultError> {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("vault.dat");
    let corrupt = path.with_file_name(format!("{file_name}.corrupt-{stamp}"));
    std::fs::rename(path, &corrupt)?;

    let backup = path.with_extension("bak");
    if !backup.exists() {
        return Err(VaultError::Corrupt(format!(
            "无法读取密码库，原文件已保存为 {}",
            corrupt.display()
        )));
    }
    std::fs::copy(&backup, path)?;
    VaultCrypto::read_file(path).map_err(|error| {
        VaultError::Corrupt(format!(
            "无法读取密码库，原文件已保存为 {}；备份恢复失败：{error}",
            corrupt.display()
        ))
    })
}

/// 把解密结果写入内存状态。
fn apply_decrypted(inner: &mut VaultInner, decrypted: DecryptedVault) {
    let (entries, settings, key) = decrypted.into_parts();
    inner.state = VaultState::Unlocked;
    inner.entries = entries;
    inner.settings = settings;
    inner.key = Some(key);
    inner.last_activity = Instant::now();
}

/// 用当前会话密钥重新加密并原子写回。
fn save_inner(path: &Path, inner: &mut VaultInner) -> Result<(), VaultError> {
    let key = inner.key.ok_or(VaultError::Locked)?;
    if let Some(file) = inner.file.as_mut() {
        VaultCrypto::rewrite_payload(file, &inner.entries, &inner.settings, &key)?;
        VaultCrypto::write_file(path, file)?;
        inner.file_stamp = file_stamp(path);
    } else {
        let (file, _) = VaultCrypto::new_dpapi(inner.entries.clone(), inner.settings)?;
        VaultCrypto::write_file(path, &file)?;
        inner.file = Some(file);
        inner.state = VaultState::Unlocked;
        inner.file_stamp = file_stamp(path);
    }
    Ok(())
}

/// 读取文件大小和修改时间，用于检测外部变更。
fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    std::fs::metadata(path).ok().and_then(|metadata| {
        let modified = metadata.modified().ok()?;
        let nanos = modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos() as u64;
        Some((metadata.len(), nanos))
    })
}
