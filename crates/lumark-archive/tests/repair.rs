//! ZIP 家族修复回归测试。

use std::fs;
use std::io::Write;
use std::path::Path;

use lumark_archive::{
    ArchiveError, ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions,
    repair::repair_zip,
};
use tempfile::tempdir;
use zeroize::Zeroizing;

/// 找到 EOCD 起点并截断中央目录之后的部分，模拟目录损坏。
fn truncate_eocd(path: &Path) {
    let bytes = fs::read(path).unwrap();
    let position = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .unwrap();
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(position as u64).unwrap();
}

/// 修复普通 ZIP 并验证条目与内容。
#[test]
fn repairs_zip_after_eocd_damage() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("hello.txt"), b"hello repair").unwrap();
    fs::write(source.join("nested").join("data.bin"), vec![7u8; 4096]).unwrap();
    let archive = directory.path().join("broken.zip");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    truncate_eocd(&archive);
    assert!(ArchiveService::list(&archive, None).is_err());

    let repaired = directory.path().join("repaired.zip");
    let report = repair_zip(&archive, &repaired).unwrap();
    assert_eq!(report.entries, 2);
    assert!(report.skipped.is_empty());
    assert!(ArchiveService::list(&repaired, None).is_ok());
    let output = directory.path().join("out");
    ArchiveService::extract(
        &repaired,
        &ExtractionOptions {
            output_dir: output.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("source/hello.txt")).unwrap(),
        b"hello repair"
    );
    assert_eq!(
        fs::read(output.join("source/nested/data.bin"))
            .unwrap()
            .len(),
        4096
    );
}

/// 修复带数据描述符的 ZIP（大小只在描述符之后/中央目录中）。
#[test]
fn repairs_zip_with_data_descriptors() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("descriptor.zip");
    let file = fs::File::create(&archive).unwrap();
    let mut writer = zip::ZipWriter::new_stream(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("hello.txt", options).unwrap();
    writer.write_all(b"descriptor data").unwrap();
    writer.finish().unwrap();
    let repaired = directory.path().join("descriptor-repaired.zip");
    let report = repair_zip(&archive, &repaired).unwrap();
    assert_eq!(report.entries, 1);
    assert!(report.skipped.is_empty());
    let output = directory.path().join("out");
    ArchiveService::extract(
        &repaired,
        &ExtractionOptions {
            output_dir: output.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("hello.txt")).unwrap(),
        b"descriptor data"
    );
}

/// 修复加密 ZIP 后密码仍可用。
#[test]
fn repairs_encrypted_zip() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("secret.txt"), b"encrypted contents").unwrap();
    let archive = directory.path().join("encrypted.zip");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(Zeroizing::new("repair-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    truncate_eocd(&archive);
    let repaired = directory.path().join("encrypted-repaired.zip");
    let report = repair_zip(&archive, &repaired).unwrap();
    assert_eq!(report.entries, 1);
    let info = ArchiveService::list(&repaired, Some("repair-pass")).unwrap();
    assert!(info.encrypted);
    let output = directory.path().join("out");
    ArchiveService::extract(
        &repaired,
        &ExtractionOptions {
            output_dir: output.clone(),
            password: Some(Zeroizing::new("repair-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("source/secret.txt")).unwrap(),
        b"encrypted contents"
    );
}

/// 非 ZIP 家族和输出覆盖都会被拒绝。
#[test]
fn rejects_unsupported_or_unsafe_output() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("hello.txt"), b"hi").unwrap();
    let archive = directory.path().join("sample.7z");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(matches!(
        repair_zip(&archive, &directory.path().join("out.7z")),
        Err(ArchiveError::UnsupportedFormat(_))
    ));

    let zip = directory.path().join("sample.zip");
    ArchiveService::create(
        &zip,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(matches!(
        repair_zip(&zip, &zip),
        Err(ArchiveError::InvalidRequest(_))
    ));
    assert!(repair_zip(&zip, &directory.path().join("existing.zip")).is_ok());
    // 第二次输出到同一路径必须拒绝，不能覆盖已有修复结果。
    assert!(matches!(
        repair_zip(&zip, &directory.path().join("existing.zip")),
        Err(ArchiveError::InvalidRequest(_))
    ));
}
