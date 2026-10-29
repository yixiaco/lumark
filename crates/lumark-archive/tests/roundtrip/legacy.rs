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

/// 返回仓库内 RAR5 测试样本路径。
fn rar5_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("rar5")
        .join(name)
}

#[test]
/// 验证 RAR5 样本能识别、列出并提取，方法列显示 RAR5。
fn rar5_samples_list_and_extract() {
    for name in ["comment.rar", "unicode.rar"] {
        let archive = rar5_fixture(name);
        let info = ArchiveService::list(&archive, None)
            .unwrap_or_else(|error| panic!("{name} list failed: {error}"));
        assert_eq!(info.format, Some(ArchiveFormat::Rar));
        assert!(!info.entries.is_empty(), "{name}");
        assert!(
            info.entries.iter().all(|entry| entry.method == "RAR5"),
            "{name} 未显示 RAR5 方法"
        );
        let output = tempdir().unwrap();
        let report = ArchiveService::extract(
            &archive,
            &ExtractionOptions {
                output_dir: output.path().to_path_buf(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert!(report.files > 0, "{name} 没有提取文件");
        ArchiveService::test(&archive, None, |_| {}).unwrap();
    }
}

#[test]
/// 验证固实 RAR5 标记和 Unicode 文件名。
fn rar5_solid_and_unicode_are_reported() {
    let solid = ArchiveService::list(&rar5_fixture("solid.rar"), None).unwrap();
    assert!(solid.solid, "solid.rar 应报告固实标记");

    let unicode = ArchiveService::list(&rar5_fixture("unicode.rar"), None).unwrap();
    let names = unicode
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    assert!(
        names.iter().any(|path| !path.is_ascii()),
        "Unicode 样本缺少非 ASCII 文件名：{names:?}"
    );
}

#[test]
/// 验证 RAR5 头加密在无密码时失败、正确密码时可列出。
fn rar5_header_encryption_requires_password() {
    let archive = rar5_fixture("comment-hpw-password.rar");
    assert!(
        ArchiveService::list(&archive, None).is_err(),
        "头加密 RAR5 不应在无密码时列出"
    );
    let info = ArchiveService::list(&archive, Some("password"))
        .unwrap_or_else(|error| panic!("头加密 RAR5 使用正确密码仍失败：{error}"));
    assert!(info.header_encrypted, "头加密标记未上报");
    assert!(!info.entries.is_empty());
    ArchiveService::test(&archive, Some("password"), |_| {}).unwrap();
}

#[test]
/// 验证后续分卷被拒绝，而不是当成独立归档。
fn rar5_subsequent_volume_is_rejected() {
    assert!(ArchiveService::list(&rar5_fixture("100M.part00002.rar"), None).is_err());
}
