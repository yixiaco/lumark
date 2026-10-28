//! 设置的原子持久化和线程安全访问。

use std::path::{Path, PathBuf};

use parking_lot::RwLock;

use crate::model::Settings;

#[derive(Debug, thiserror::Error)]
/// 设置读取、写入和解析错误。
pub enum SettingsError {
    #[error("设置文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("设置文件格式错误：{0}")]
    Json(#[from] serde_json::Error),
}

/// 线程安全的设置存储。
pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    /// 返回当前用户的设置文件路径。
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Lumark")
            .join("settings.json")
    }

    /// 打开或创建设置文件。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, SettingsError> {
        let path = path.into();
        let current = if path.exists() {
            let bytes = std::fs::read(&path)?;
            match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings) => settings,
                Err(_) => {
                    let corrupt = path.with_file_name(format!(
                        "{}.corrupt-{}",
                        path.file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("settings.json"),
                        chrono::Utc::now().format("%Y%m%d-%H%M%S")
                    ));
                    let _ = std::fs::rename(&path, corrupt);
                    Settings::default()
                }
            }
        } else {
            Settings::default()
        };
        Ok(Self {
            path,
            current: RwLock::new(current),
        })
    }

    /// 返回设置快照。
    pub fn get(&self) -> Settings {
        self.current.read().clone()
    }

    /// 修改设置并持久化。
    pub fn update<R>(&self, update: impl FnOnce(&mut Settings) -> R) -> Result<R, SettingsError> {
        let mut current = self.current.write();
        let result = update(&mut current);
        current.version = 1;
        save(&self.path, &current)?;
        Ok(result)
    }

    /// 返回设置文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 把设置原子写回磁盘。
fn save(path: &Path, settings: &Settings) -> Result<(), SettingsError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(settings)?;
    let temp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut output = std::fs::File::create(&temp)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
    }
    if let Err(error) = atomic_replace(path, &temp, &path.with_extension("bak")) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(windows)]
/// 原子替换设置文件，并按需保留备份。
fn atomic_replace(path: &Path, temp: &Path, backup: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    if !path.exists() {
        return std::fs::rename(temp, path);
    }
    let path_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let temp_wide: Vec<u16> = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let backup_wide: Vec<u16> = backup
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let ok = unsafe {
        ReplaceFileW(
            path_wide.as_ptr(),
            temp_wide.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
/// 原子替换设置文件，并按需保留备份。
fn atomic_replace(path: &Path, temp: &Path, backup: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::copy(path, backup)?;
    }
    std::fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证设置更新后重新打开仍保留。
    fn settings_update_twice_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = SettingsStore::open(&path).unwrap();
        store
            .update(|settings| settings.theme = crate::ThemePreference::Dark)
            .unwrap();
        store
            .update(|settings| settings.auto_lock_minutes = 30)
            .unwrap();
        let reopened = SettingsStore::open(&path).unwrap();
        assert_eq!(reopened.get().theme, crate::ThemePreference::Dark);
        assert_eq!(reopened.get().auto_lock_minutes, 30);
        assert_eq!(reopened.get().version, 1);
    }
}
