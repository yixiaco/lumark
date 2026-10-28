use super::common::*;
use super::*;

#[test]
fn unsafe_paths_are_rejected() {
    assert!(safe_join(std::path::Path::new("C:\\out"), "../escape").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "C:\\absolute").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "folder\\..\\escape").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "CON").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "folder\\file.txt").is_ok());
}
#[test]
fn split_volume_archive_can_be_listed_and_extracted() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("large.bin");
    let mut state = 0x9E37_79B9u32;
    let bytes: Vec<u8> = (0..220 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&source, bytes).unwrap();
    let output = dir.path().join("volumes.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(!output.exists());
    let first = dir.path().join("volumes.zip.001");
    assert!(first.exists());
    let info = ArchiveService::list(&first, None).unwrap();
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("large.bin"))
    );
    let extraction = dir.path().join("split-out");
    ArchiveService::extract(
        &first,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(extraction.join("large.bin")).unwrap().len(),
        220 * 1024
    );
}

#[test]
fn replacing_a_split_archive_removes_stale_volumes() {
    let dir = tempdir().unwrap();
    let large = dir.path().join("large.bin");
    let mut state = 0xA5A5_5A5Au32;
    let large_bytes: Vec<u8> = (0..420 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&large, large_bytes).unwrap();
    let output = dir.path().join("volumes.zip");
    ArchiveService::create(
        &output,
        &[large],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(dir.path().join("volumes.zip.003").exists());

    let small = dir.path().join("small.bin");
    let mut state = 0x1234_5678u32;
    let small_bytes: Vec<u8> = (0..80 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&small, small_bytes).unwrap();
    ArchiveService::create(
        &output,
        &[small],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(dir.path().join("volumes.zip.001").exists());
    assert!(dir.path().join("volumes.zip.002").exists());
    assert!(!dir.path().join("volumes.zip.003").exists());
}

#[test]
fn compression_can_test_before_publish_and_delete_sources() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("delete-me.txt");
    fs::write(&source, b"test and delete").unwrap();
    let archive = dir.path().join("verified.zip");
    let report = ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(zeroize::Zeroizing::new("verify-pass".to_string())),
            test_after: true,
            delete_source: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(report.cleanup_errors.is_empty());
    assert!(archive.exists());
    assert!(!source.exists());
    assert!(ArchiveService::verify_password(&archive, "verify-pass").unwrap());
}

#[test]
fn delete_source_is_rejected_when_output_is_inside_source() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload.txt"), b"payload").unwrap();
    let output = source.join("nested.zip");
    let result = ArchiveService::create(
        &output,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            delete_source: true,
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
    assert!(source.join("payload.txt").exists());
    assert!(!output.exists());
}

#[test]
fn failed_split_creation_preserves_existing_volumes() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("payload.bin");
    fs::write(&source, vec![0x33u8; 180 * 1024]).unwrap();
    let output = dir.path().join("volumes.zip");
    fs::write(dir.path().join("volumes.zip.001"), b"existing volume").unwrap();
    fs::create_dir(dir.path().join("volumes.zip.002")).unwrap();

    let result = ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read(dir.path().join("volumes.zip.001")).unwrap(),
        b"existing volume"
    );
    assert!(dir.path().join("volumes.zip.002").is_dir());
    assert!(
        fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".lumark-split"))
    );
}
/// 验证自解压包由运行时模块、ZIP 数据和尾部页脚组成，且运行时能解析并解压。
#[test]
fn create_sfx_embeds_zip_payload_after_stub() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let stub = dir.path().join("stub.exe");
    let stub_bytes = b"MZ-fake-sfx-stub";
    fs::write(&stub, stub_bytes).unwrap();
    let output = dir.path().join("bundle.exe");
    ArchiveService::create_sfx(
        &output,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        &stub,
        |_| {},
    )
    .unwrap();

    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(stub_bytes));
    assert_eq!(
        &bytes[bytes.len() - lumark_sfx::FOOTER_LEN..][..8],
        b"LMSFX001"
    );
    let mut file = std::fs::File::open(&output).unwrap();
    let footer = lumark_sfx::read_footer(&mut file).unwrap().expect("页脚");
    assert_eq!(footer.offset, stub_bytes.len() as u64);
    let target = dir.path().join("extracted");
    let files = lumark_sfx::extract_payload(file, footer, &target).unwrap();
    assert_eq!(files, 2);
    assert_eq!(
        fs::read_to_string(target.join("source").join("hello.txt")).unwrap(),
        "hello from lumark"
    );
    assert_eq!(
        fs::read(target.join("source").join("nested").join("data.bin")).unwrap(),
        [0u8, 1, 2, 3, 4]
    );
}

/// 验证自解压生成会拒绝密码、非 ZIP 和缺失的运行时模块。
#[test]
fn create_sfx_rejects_unsupported_options() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let stub = dir.path().join("stub.exe");
    fs::write(&stub, b"MZ-fake-sfx-stub").unwrap();
    let output = dir.path().join("bundle.exe");
    let encrypted = CompressionOptions {
        format: ArchiveFormat::Zip,
        password: Some(zeroize::Zeroizing::new("secret".to_string())),
        ..Default::default()
    };
    assert!(
        ArchiveService::create_sfx(
            &output,
            std::slice::from_ref(&source),
            &encrypted,
            &stub,
            |_| {}
        )
        .is_err(),
        "自解压不应接受密码"
    );
    let seven_zip = CompressionOptions {
        format: ArchiveFormat::SevenZip,
        ..Default::default()
    };
    assert!(
        ArchiveService::create_sfx(
            &output,
            std::slice::from_ref(&source),
            &seven_zip,
            &stub,
            |_| {}
        )
        .is_err(),
        "自解压不应接受非 ZIP 格式"
    );
    assert!(
        ArchiveService::create_sfx(
            &output,
            std::slice::from_ref(&source),
            &CompressionOptions::default(),
            &dir.path().join("missing-stub.exe"),
            |_| {}
        )
        .is_err(),
        "缺少运行时模块时应失败"
    );
    assert!(!output.exists(), "失败时不应留下半成品");
}
/// 验证解压预算能拦截 ZIP 炸弹，并在写出前失败。
#[test]
fn zip_extraction_budget_stops_bomb() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("zeros.bin");
    fs::write(&source, vec![0u8; 4 * 1024 * 1024]).unwrap();
    let archive = dir.path().join("bomb.zip");
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
    let output_dir = dir.path().join("out");
    let result = extract_with_small_budget(&archive, &output_dir);
    assert!(
        matches!(result, Err(lumark_archive::ArchiveError::BudgetExceeded(_))),
        "{result:?}"
    );
    assert!(
        !output_dir.join("zeros.bin").exists(),
        "超限不应留下部分文件"
    );
}

/// 验证解压预算能拦截 7Z 炸弹。
#[test]
fn seven_zip_extraction_budget_stops_bomb() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("zeros.bin");
    fs::write(&source, vec![0u8; 4 * 1024 * 1024]).unwrap();
    let archive = dir.path().join("bomb.7z");
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
    let output_dir = dir.path().join("out");
    let result = extract_with_small_budget(&archive, &output_dir);
    assert!(
        matches!(result, Err(lumark_archive::ArchiveError::BudgetExceeded(_))),
        "{result:?}"
    );
    assert!(
        !output_dir.join("zeros.bin").exists(),
        "超限不应留下部分文件"
    );
}
