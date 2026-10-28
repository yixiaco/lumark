//! 多窗口共享的设置和密码库存储。

use super::*;

/// 多个窗口共享的持久化服务。
///
/// 设置和密码库必须进程内唯一：每个窗口各自打开会互相覆盖设置文件，
/// 并让密码库出现多份互不可见的解锁状态。
#[derive(Clone)]
pub(crate) struct SharedStores {
    /// 共享设置存储。
    pub(super) settings: Arc<SettingsStore>,
    /// 共享密码库。
    pub(super) vault: Arc<VaultStore>,
    /// 设置打开错误。
    pub(super) settings_error: Option<String>,
    /// 密码库打开错误。
    pub(super) vault_error: Option<String>,
}

impl SharedStores {
    /// 读取设置，并按设置中记录的密码库位置打开共享存储。
    pub(crate) fn open(settings_path: PathBuf) -> Self {
        let (settings, settings_error) = open_settings(settings_path);
        let vault_path = settings
            .get()
            .vault_path
            .unwrap_or_else(VaultStore::default_path);
        Self::with_vault(settings, settings_error, vault_path)
    }

    /// 使用调用方指定的密码库位置打开，供自定义路径和集成测试使用。
    pub(crate) fn open_with(vault_path: PathBuf, settings_path: PathBuf) -> Self {
        let (settings, settings_error) = open_settings(settings_path);
        Self::with_vault(settings, settings_error, vault_path)
    }

    /// 打开密码库；失败时退回临时目录，保证窗口仍能启动并提示错误。
    fn with_vault(
        settings: Arc<SettingsStore>,
        settings_error: Option<String>,
        vault_path: PathBuf,
    ) -> Self {
        let (vault, vault_error) = match VaultStore::open(vault_path) {
            Ok(vault) => (Arc::new(vault), None),
            Err(error) => {
                eprintln!("vault open failed: {error}");
                (
                    Arc::new(
                        VaultStore::open(
                            std::env::temp_dir().join("lumark-vault").join("vault.dat"),
                        )
                        .expect("temporary vault"),
                    ),
                    Some(error.to_string()),
                )
            }
        };
        Self {
            settings,
            vault,
            settings_error,
            vault_error,
        }
    }
}

/// 读取设置文件；损坏或不可读时退回临时目录并保留错误文本。
fn open_settings(settings_path: PathBuf) -> (Arc<SettingsStore>, Option<String>) {
    match SettingsStore::open(settings_path) {
        Ok(settings) => (Arc::new(settings), None),
        Err(error) => {
            let fallback = std::env::temp_dir()
                .join("lumark-settings")
                .join("settings.json");
            (
                Arc::new(SettingsStore::open(fallback).expect("temporary Lumark settings store")),
                Some(error.to_string()),
            )
        }
    }
}
