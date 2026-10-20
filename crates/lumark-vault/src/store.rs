use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::RwLock;
use uuid::Uuid;

use crate::crypto::{DecryptedVault, VaultCrypto, VaultFile};
use crate::model::{PasswordEntry, VaultError, VaultSettings, VaultState};
use crate::resolver::{PasswordCandidate, PasswordResolver};

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

pub struct VaultStore {
    path: PathBuf,
    inner: RwLock<VaultInner>,
}

impl VaultStore {
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Lumark")
            .join("vault")
            .join("vault.dat")
    }

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
            let file = match VaultCrypto::read_file(&path) {
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

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn state(&self) -> VaultState {
        self.inner.read().state
    }

    pub fn is_unlocked(&self) -> bool {
        self.state() == VaultState::Unlocked
    }

    pub fn has_master_password(&self) -> bool {
        self.inner.read().file.as_ref().is_some_and(|file| {
            matches!(file.key_wrap, crate::crypto::KeyWrap::MasterPassword { .. })
        })
    }

    pub fn entry_count(&self) -> usize {
        let inner = self.inner.read();
        inner.entries.len().max(
            inner
                .file
                .as_ref()
                .map_or(0, |file| file.entry_count as usize),
        )
    }

    pub fn settings(&self) -> VaultSettings {
        self.inner.read().settings
    }

    pub fn unlock(&self, master_password: &str) -> Result<(), VaultError> {
        let mut inner = self.inner.write();
        let file = inner.file.as_ref().ok_or(VaultError::Uninitialized)?;
        let decrypted = VaultCrypto::unlock(file, Some(master_password))?;
        apply_decrypted(&mut inner, decrypted);
        inner.file_stamp = file_stamp(&self.path);
        Ok(())
    }

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

    pub fn lock(&self) {
        let mut inner = self.inner.write();
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

    pub fn touch(&self) {
        self.inner.write().last_activity = Instant::now();
    }

    pub fn acquire_lease(&self) {
        let mut inner = self.inner.write();
        inner.leases = inner.leases.saturating_add(1);
        inner.last_activity = Instant::now();
    }

    pub fn release_lease(&self) {
        let mut inner = self.inner.write();
        inner.leases = inner.leases.saturating_sub(1);
        inner.last_activity = Instant::now();
    }

    pub fn maybe_auto_lock(&self) -> bool {
        let mut inner = self.inner.write();
        if inner.state != VaultState::Unlocked || inner.leases > 0 {
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

    pub fn entries(&self) -> Vec<PasswordEntry> {
        self.inner.read().entries.clone()
    }

    pub fn find_entry(&self, id: Uuid) -> Option<PasswordEntry> {
        self.inner
            .read()
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

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

    pub fn clear_entries(&self) -> Result<(), VaultError> {
        self.replace_entries(Vec::new())
    }

    pub fn mark_used(&self, id: Uuid) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        let mut inner = self.inner.write();
        if let Some(entry) = inner.entries.iter_mut().find(|entry| entry.id == id) {
            entry.touch_used();
            save_inner(&self.path, &mut inner)?;
        }
        Ok(())
    }

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

    pub fn candidates(
        &self,
        archive_path: &Path,
        fingerprint: Option<&str>,
    ) -> Vec<PasswordCandidate> {
        let entries = self.inner.read().entries.clone();
        PasswordResolver::candidates(&entries, archive_path, fingerprint)
    }

    pub fn export(&self, path: &Path, passphrase: &str) -> Result<(), VaultError> {
        self.ensure_unlocked()?;
        let inner = self.inner.read();
        VaultCrypto::export(path, &inner.entries, &inner.settings, passphrase)
    }

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

fn ensure_key(inner: &VaultInner) -> Result<(), VaultError> {
    if inner.key.is_some() {
        Ok(())
    } else {
        Err(VaultError::Locked)
    }
}

fn validate_pattern(pattern: Option<&str>) -> Result<(), VaultError> {
    let Some(pattern) = pattern.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    globset::Glob::new(pattern)
        .map(|_| ())
        .map_err(|_| VaultError::InvalidPattern(pattern.to_string()))
}

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

fn apply_decrypted(inner: &mut VaultInner, decrypted: DecryptedVault) {
    let (entries, settings, key) = decrypted.into_parts();
    inner.state = VaultState::Unlocked;
    inner.entries = entries;
    inner.settings = settings;
    inner.key = Some(key);
    inner.last_activity = Instant::now();
}

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
