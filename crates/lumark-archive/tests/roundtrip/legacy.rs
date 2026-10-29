//! RAR、加密容器和传统格式样本测试。

use super::common::*;
use super::*;

#[test]
/// 验证提供 RAR 样本时的列表。
fn rar_sample_can_be_listed_when_provided() {
    let Ok(sample) = std::env::var("LUMARK_RAR_SAMPLE") else {
        return;
    };
    let path = PathBuf::from(sample);
    if !path.exists() {
        return;
    }
    let info = ArchiveService::list(&path, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Rar));
    assert!(!info.entries.is_empty());
    if !info.encrypted {
        let output = tempdir().unwrap();
        let report = ArchiveService::extract(
            &path,
            &ExtractionOptions {
                output_dir: output.path().to_path_buf(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert!(report.files > 0);
        let tested = ArchiveService::test(&path, None, |_| {}).unwrap();
        assert!(tested.files > 0);
    }
}

#[test]
/// 验证损坏 RAR 不会被误报为加密。
fn corrupted_rar_is_not_reported_as_encrypted() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("broken.rar");
    fs::write(&archive, b"this is not a rar archive").unwrap();
    assert!(
        ArchiveService::probe_encryption(&archive).is_err(),
        "损坏归档必须返回解析错误，而不是加密状态"
    );
}

#[test]
/// 验证畸形 .Z 被报告为损坏。
fn malformed_unix_compress_is_reported_as_corrupt() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("broken.Z");
    fs::write(&archive, b"\x1f\x9d\x90broken unix compress").unwrap();
    let result = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: dir.path().join("out"),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
}

#[test]
/// 验证 AES 加密容器的列表、密码验证和解压。
fn aes_encrypted_container_lists_verifies_and_extracts() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("secret.txt.enc");
    let options = enc_file::EncryptOptions {
        alg: enc_file::AeadAlg::Aes256GcmSiv,
        ..Default::default()
    };
    let encrypted = enc_file::encrypt_bytes(
        b"encrypted container payload",
        secrecy::SecretString::new("container-pass".to_string().into()),
        &options,
    )
    .unwrap();
    fs::write(&archive, encrypted).unwrap();

    let info = ArchiveService::list(&archive, Some("container-pass")).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Enc));
    assert!(info.encrypted);
    assert!(info.entries[0].size > 0);
    assert!(ArchiveService::verify_password(&archive, "container-pass").unwrap());
    assert!(!ArchiveService::verify_password(&archive, "wrong-pass").unwrap());

    let output = dir.path().join("enc-out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            password: Some(zeroize::Zeroizing::new("container-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("secret.txt")).unwrap(),
        b"encrypted container payload"
    );
}

#[test]
/// 验证解压覆盖策略的跳过和重命名。
fn extraction_overwrite_policies_skip_or_rename() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("payload.txt");
    fs::write(&source, b"payload").unwrap();
    let archive = dir.path().join("policy.zip");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let output = dir.path().join("out");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("payload.txt"), b"keep").unwrap();
    let skipped = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite_policy: OverwritePolicy::Skip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(skipped.skipped, 1);
    assert_eq!(fs::read(output.join("payload.txt")).unwrap(), b"keep");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite_policy: OverwritePolicy::Rename,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("payload (1).txt")).unwrap(),
        b"payload"
    );
}

#[test]
/// 验证 CPIO 传统读取器的列表和解压。
fn cpio_legacy_reader_lists_and_extracts() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("sample.cpio");
    let mut bytes = Vec::new();
    append_newc_entry(&mut bytes, "hello.txt", b"legacy cpio");
    append_newc_entry(&mut bytes, "TRAILER!!!", b"");
    fs::write(&archive, bytes).unwrap();
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Cpio));
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));
    let output = dir.path().join("cpio-out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(fs::read(output.join("hello.txt")).unwrap(), b"legacy cpio");
}
