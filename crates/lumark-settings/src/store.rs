use std::path::{Path, PathBuf};

use parking_lot::RwLock;

use crate::model::Settings;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("设置文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("设置文件格式错误：{0}")]
    Json(#[from] serde_json::Error),
}

pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Lumark")
            .join("settings.json")
    }

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

    pub fn get(&self) -> Settings {
        self.current.read().clone()
    }

    pub fn update<R>(&self, update: impl FnOnce(&mut Settings) -> R) -> Result<R, SettingsError> {
        let mut current = self.current.write();
        let result = update(&mut current);
        current.version = 1;
        save(&self.path, &current)?;
        Ok(result)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

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
