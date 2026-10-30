//! ASAR 写入、读取、客户端兼容与错误路径测试。

use super::common::make_source;
use super::*;
use zeroize::Zeroizing;

/// 解析 ASAR 头部的 JSON 部分。
fn asar_json(bytes: &[u8]) -> serde_json::Value {
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[16..16 + length]).unwrap()
}

/// 创建包含文本、嵌套二进制和超过一个 4MiB 分块的大文件。
fn asar_source(root: &std::path::Path) -> PathBuf {
    let source = make_source(root);
    fs::write(
        source.join("nested").join("large.bin"),
        vec![0x5a; 5 * 1024 * 1024],
    )
    .unwrap();
    source
}

/// 验证创建、列表、解压、测试和 Electron header 字段。
#[test]
fn asar_roundtrip_and_header_integrity() {
    let directory = tempdir().unwrap();
    let source = asar_source(directory.path());
    let archive = directory.path().join("sample.asar");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Asar,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Asar);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Asar));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "source/hello.txt")
    );

    ArchiveService::test(&archive, None, |_| {}).unwrap();
    let output = directory.path().join("asar-out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("source/hello.txt")).unwrap(),
        b"hello from lumark"
    );
    assert_eq!(
        fs::metadata(output.join("source/nested/large.bin"))
            .unwrap()
            .len(),
        5 * 1024 * 1024
    );

    let bytes = fs::read(&archive).unwrap();
    let json = asar_json(&bytes);
    let large = &json["files"]["source"]["files"]["nested"]["files"]["large.bin"];
    assert_eq!(large["size"].as_u64(), Some(5 * 1024 * 1024));
    assert!(large["offset"].is_string());
    assert_eq!(large["integrity"]["algorithm"], "SHA256");
    assert_eq!(
        large["integrity"]["blockSize"].as_u64(),
        Some(4 * 1024 * 1024)
    );
    assert_eq!(large["integrity"]["blocks"].as_array().unwrap().len(), 2);

    // 用独立的 asar crate 解析，确认不会被自家读取器“自洽”掩盖。
    let reader = ::asar::AsarReader::new(&bytes, None).unwrap();
    let file = reader
        .files()
        .get(std::path::Path::new("source/hello.txt"))
        .unwrap();
    assert_eq!(file.data(), b"hello from lumark");
}

/// 验证没有扩展名时仍能按内容识别 ASAR。
#[test]
fn asar_is_detected_by_content_without_extension() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let archive = directory.path().join("payload");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Asar,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Asar);
    assert!(ArchiveService::list(&archive, None).is_ok());
}

/// 验证空文件写入空 blocks 数组，密码、重复路径和取消都明确失败。
#[test]
fn asar_empty_file_and_failure_paths() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("empty-source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("empty.txt"), b"").unwrap();
    let archive = directory.path().join("empty.asar");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Asar,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let json = asar_json(&fs::read(&archive).unwrap());
    let empty = &json["files"]["empty-source"]["files"]["empty.txt"];
    assert_eq!(empty["size"], 0);
    assert!(empty["integrity"]["blocks"].as_array().unwrap().is_empty());

    let error = ArchiveService::create(
        &directory.path().join("password.asar"),
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Asar,
            password: Some(Zeroizing::new("secret".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, ArchiveError::InvalidRequest(_)));

    let hello = source.join("empty.txt");
    let error = ArchiveService::create(
        &directory.path().join("duplicate.asar"),
        &[hello.clone(), hello],
        &CompressionOptions {
            format: ArchiveFormat::Asar,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("重复"));

    let cancel = Arc::new(AtomicBool::new(true));
    let error = ArchiveService::create(
        &directory.path().join("cancel.asar"),
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Asar,
            cancel: Some(cancel),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, ArchiveError::Cancelled));
}
