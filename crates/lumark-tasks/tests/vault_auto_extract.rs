//! 密码库自动解压集成测试。

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions};
use lumark_vault::{PasswordEntry, PasswordResolver, VaultCrypto, VaultSettings, VaultStore};
use tempfile::tempdir;

#[test]
/// 验证记住的密码会成为候选并自动解压。
fn remembered_password_is_candidate_and_extracts_archive() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("secret.txt"), b"auto password").unwrap();
    let archive = dir.path().join("secret.zip");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(zeroize::Zeroizing::new("vault-pass-123".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    let vault_path = dir.path().join("vault.dat");
    let (file, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass-123").unwrap();
    VaultCrypto::write_file(&vault_path, &file).unwrap();
    let store = VaultStore::open(&vault_path).unwrap();
    store.unlock("master-pass-123").unwrap();
    let mut entry = PasswordEntry::new("secret archive", "vault-pass-123");
    entry.archive_pattern = Some("*.zip".to_string());
    store.add_entry(entry).unwrap();
    let fingerprint = PasswordResolver::archive_fingerprint(&archive).unwrap();
    let candidates = store.candidates(&archive, Some(&fingerprint));
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].password.expose(), "vault-pass-123");
    assert!(ArchiveService::verify_password(&archive, candidates[0].password.expose()).unwrap());

    let output = dir.path().join("out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            password: Some(zeroize::Zeroizing::new(
                candidates[0].password.expose().to_string(),
            )),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("source").join("secret.txt")).unwrap(),
        b"auto password"
    );
}
