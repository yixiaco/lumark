//! Lumark 的加密归档密码库。

pub mod crypto;
pub mod model;
pub mod resolver;
pub mod store;

pub use crypto::VaultCrypto;
pub use model::{
    PasswordEntry, PasswordMatch, PasswordSource, SecretString, VaultError, VaultSettings,
    VaultState,
};
pub use resolver::{PasswordCandidate, PasswordResolver};
pub use store::VaultStore;

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let secret = model::SecretString::new("very-secret");
        assert!(!format!("{secret:?}").contains("very-secret"));
    }

    #[test]
    fn master_password_roundtrip_and_tamper_detection() {
        let mut entry = PasswordEntry::new("工作压缩包", "s3cret!");
        entry.archive_pattern = Some("C:\\work\\data.zip".to_string());
        let settings = VaultSettings::default();
        let (mut file, _) =
            VaultCrypto::new_master(vec![entry], settings, "correct horse").unwrap();
        assert_eq!(file.version, 2);
        assert_eq!(file.payload.algorithm, "xchacha20poly1305");
        assert_eq!(file.payload.nonce.len(), 32);
        let unlocked = VaultCrypto::unlock(&file, Some("correct horse")).unwrap();
        assert_eq!(unlocked.entries.len(), 1);
        assert_eq!(unlocked.entries[0].password.expose(), "s3cret!");

        assert!(matches!(
            VaultCrypto::unlock(&file, Some("wrong password")),
            Err(VaultError::InvalidMasterPassword)
        ));

        let mut bytes = file.payload.ciphertext.into_bytes();
        bytes[0] = if bytes[0] == b'A' { b'B' } else { b'A' };
        file.payload.ciphertext = String::from_utf8(bytes).unwrap();
        assert!(matches!(
            VaultCrypto::unlock(&file, Some("correct horse")),
            Err(VaultError::Corrupt(_))
        ));
    }

    #[test]
    fn resolver_prioritizes_path_and_deduplicates() {
        let path = std::path::Path::new("C:\\Downloads\\archive.zip");
        let mut exact = PasswordEntry::new("exact", "same-password");
        exact.archive_pattern = Some("C:\\Downloads\\archive.zip".to_string());
        let mut glob = PasswordEntry::new("glob", "other-password");
        glob.archive_pattern = Some("C:\\Downloads\\*.zip".to_string());
        let generic = PasswordEntry::new("generic", "generic-password");
        let duplicate = PasswordEntry::new("duplicate", "same-password");
        let candidates =
            PasswordResolver::candidates(&[generic, glob, exact, duplicate], path, None);
        assert_eq!(candidates[0].label, "exact");
        assert_eq!(candidates[0].password.expose(), "same-password");
        assert!(
            candidates
                .iter()
                .filter(|candidate| candidate.password.expose() == "same-password")
                .count()
                == 1
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == "generic")
        );
    }

    #[test]
    #[cfg(windows)]
    fn dpapi_vault_roundtrip_and_store_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let store = VaultStore::open(&path).unwrap();
        store
            .add_entry(PasswordEntry::new("测试", "abc12345"))
            .unwrap();
        store
            .add_entry(PasswordEntry::new("第二个", "def67890"))
            .unwrap();
        assert_eq!(store.entry_count(), 2);
        assert!(path.exists());
        let reopened = VaultStore::open(&path).unwrap();
        assert!(reopened.is_unlocked());
        assert_eq!(reopened.entry_count(), 2);
        reopened.lock();
        assert!(!reopened.is_unlocked());
        assert_eq!(reopened.entry_count(), 2);
    }

    #[test]
    #[cfg(windows)]
    fn encrypted_export_import_roundtrip() {
        let dir = tempdir().unwrap();
        let store_path = dir.path().join("vault.dat");
        let export_path = dir.path().join("backup.lumarkvault");
        let store = VaultStore::open(&store_path).unwrap();
        store
            .add_entry(PasswordEntry::new("导出", "export-pass-123"))
            .unwrap();
        store.export(&export_path, "export-password").unwrap();
        let imported = VaultStore::open(dir.path().join("other.dat")).unwrap();
        imported
            .add_entry(PasswordEntry::new("本地", "local-pass-456"))
            .unwrap();
        let count = imported
            .import(&export_path, "export-password", false)
            .unwrap();
        assert_eq!(count, 2);
        assert!(
            imported
                .entries()
                .iter()
                .any(|entry| entry.password.expose() == "export-pass-123")
        );
    }

    #[test]
    fn archive_fingerprint_changes_with_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.bin");
        fs::write(&path, b"first").unwrap();
        let first = PasswordResolver::archive_fingerprint(&path).unwrap();
        fs::write(&path, b"second").unwrap();
        let second = PasswordResolver::archive_fingerprint(&path).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    #[cfg(windows)]
    fn corrupt_vault_is_preserved_and_restored_from_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let store = VaultStore::open(&path).unwrap();
        store
            .add_entry(PasswordEntry::new("第一版", "first-pass-123"))
            .unwrap();
        store
            .add_entry(PasswordEntry::new("第二版", "second-pass-456"))
            .unwrap();
        fs::write(&path, b"not a vault").unwrap();

        let recovered = VaultStore::open(&path).unwrap();
        assert!(recovered.is_unlocked());
        assert_eq!(recovered.entry_count(), 1);
        assert!(
            fs::read_dir(dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        );
    }

    #[test]
    #[cfg(windows)]
    fn invalid_archive_pattern_is_rejected() {
        let dir = tempdir().unwrap();
        let store = VaultStore::open(dir.path().join("vault.dat")).unwrap();
        let entry = PasswordEntry::new("invalid", "pattern-pass-123").with_pattern("[");
        assert!(matches!(
            store.add_entry(entry),
            Err(VaultError::InvalidPattern(_))
        ));
    }

    #[test]
    #[cfg(windows)]
    fn external_vault_change_forces_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let store = VaultStore::open(&path).unwrap();
        store
            .add_entry(PasswordEntry::new("外部修改", "external-pass-123"))
            .unwrap();
        fs::write(&path, b"changed").unwrap();
        assert!(store.maybe_auto_lock());
        assert_eq!(store.state(), VaultState::Locked);
    }
}
