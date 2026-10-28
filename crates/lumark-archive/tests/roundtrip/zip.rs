//! ZIP 往返、编辑和更新模式测试。

use super::common::*;
use super::*;

#[test]
/// 验证带密码 ZIP 的创建、列表和解压。
fn zip_roundtrip_with_password() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("archive.zip");
    let options = CompressionOptions {
        format: ArchiveFormat::Zip,
        password: Some(zeroize::Zeroizing::new("zip-pass-123".to_string())),
        ..Default::default()
    };
    ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();

    let info = ArchiveService::list(&output, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Zip));
    assert!(info.encrypted);
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );
    assert!(ArchiveService::verify_password(&output, "zip-pass-123").unwrap());
    assert!(!ArchiveService::verify_password(&output, "wrong").unwrap());

    let extraction = dir.path().join("extracted");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            password: Some(zeroize::Zeroizing::new("zip-pass-123".to_string())),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(extraction.join("source").join("hello.txt")).unwrap(),
        b"hello from lumark"
    );
}

#[test]
/// 验证 ZipCrypto 兼容写入往返。
fn zipcrypto_compatibility_writing_roundtrip() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("legacy.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(zeroize::Zeroizing::new("legacy-pass".to_string())),
            zip_encryption: lumark_archive::ZipEncryption::ZipCrypto,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.encrypted);
    assert!(ArchiveService::verify_password(&output, "legacy-pass").unwrap());
    let output_dir = dir.path().join("legacy-out");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: output_dir.clone(),
            overwrite: true,
            password: Some(zeroize::Zeroizing::new("legacy-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output_dir.join("source").join("hello.txt")).unwrap(),
        b"hello from lumark"
    );
}
#[test]
/// 验证 ZIP 编辑的原子性及删除、重命名和添加。
fn zip_edit_is_atomic_and_supports_delete_rename_add() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("edit.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let added = dir.path().join("added.txt");
    fs::write(&added, b"added").unwrap();
    let original = fs::read(&output).unwrap();
    ArchiveService::edit(
        &output,
        &EditOptions {
            delete_paths: vec!["source/nested".to_string()],
            rename: vec![("source/hello.txt".to_string(), "renamed.txt".to_string())],
            additions: vec![("added.txt".to_string(), added)],
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "renamed.txt"));
    assert!(info.entries.iter().any(|entry| entry.path == "added.txt"));
    assert!(
        !info
            .entries
            .iter()
            .any(|entry| entry.path.contains("nested"))
    );
    assert_ne!(fs::read(&output).unwrap(), original);
}
#[test]
/// 验证添加同名条目会替换而不是重复。
fn adding_an_existing_name_replaces_without_duplicate_entries() {
    for (format, extension) in [(ArchiveFormat::Zip, "zip"), (ArchiveFormat::SevenZip, "7z")] {
        let dir = tempdir().unwrap();
        let source = make_source(dir.path());
        let archive = dir.path().join(format!("replace.{extension}"));
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let replacement = dir.path().join("replacement.txt");
        fs::write(&replacement, b"replacement payload").unwrap();
        ArchiveService::edit(
            &archive,
            &EditOptions {
                additions: vec![("source/hello.txt".to_string(), replacement)],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let info = ArchiveService::list(&archive, None).unwrap();
        assert_eq!(
            info.entries
                .iter()
                .filter(|entry| entry.path == "source/hello.txt")
                .count(),
            1
        );
        let output = dir.path().join("replacement-out");
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
        assert_eq!(
            fs::read(output.join("source").join("hello.txt")).unwrap(),
            b"replacement payload"
        );
    }
}

#[test]
/// 验证编辑失败时原归档不变。
fn failed_edit_leaves_original_archive_untouched() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("safe.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let original = fs::read(&output).unwrap();
    let result = ArchiveService::edit(
        &output,
        &EditOptions {
            additions: vec![(
                "missing.txt".to_string(),
                dir.path().join("does-not-exist.txt"),
            )],
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&output).unwrap(), original);
}

#[test]
/// 验证编辑取消时原归档不变。
fn cancelled_edit_leaves_original_archive_untouched() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("cancel-edit.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let original = fs::read(&output).unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let result = ArchiveService::edit(
        &output,
        &EditOptions {
            rename: vec![("source/hello.txt".to_string(), "renamed.txt".to_string())],
            cancel: Some(cancel),
            ..Default::default()
        },
        |_| {},
    );
    assert!(matches!(
        result,
        Err(lumark_archive::ArchiveError::Cancelled)
    ));
    assert_eq!(fs::read(&output).unwrap(), original);
}
/// 验证“只更新较新文件”会保留较旧的归档条目，并替换更新的文件。
#[test]
fn zip_edit_update_mode_replaces_only_newer_files() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("hello.txt");
    fs::write(&source, b"original").unwrap();
    let base = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    set_modified(&source, base);
    let archive = dir.path().join("update.zip");
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

    // 较旧的文件必须被跳过，归档内容保持原样。
    fs::write(&source, b"stale").unwrap();
    set_modified(&source, base - std::time::Duration::from_secs(3600));
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            additions: vec![("hello.txt".to_string(), source.clone())],
            update_mode: UpdateMode::Newer,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 1);
    assert_eq!(
        extract_text(&archive, &dir.path().join("stale"), "hello.txt"),
        "original"
    );

    // 更新的文件应替换条目内容。
    fs::write(&source, b"fresh").unwrap();
    set_modified(&source, base + std::time::Duration::from_secs(3600));
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            additions: vec![("hello.txt".to_string(), source)],
            update_mode: UpdateMode::Newer,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 0);
    assert_eq!(
        extract_text(&archive, &dir.path().join("fresh"), "hello.txt"),
        "fresh"
    );
}

/// 验证“仅刷新已有条目”更新旧条目但不会添加新文件。
#[test]
fn zip_edit_freshen_mode_skips_new_files() {
    let dir = tempdir().unwrap();
    let existing = dir.path().join("hello.txt");
    fs::write(&existing, b"original").unwrap();
    let archive = dir.path().join("freshen.zip");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&existing),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    fs::write(&existing, b"updated").unwrap();
    let added = dir.path().join("new.txt");
    fs::write(&added, b"new file").unwrap();
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            additions: vec![
                ("hello.txt".to_string(), existing),
                ("new.txt".to_string(), added),
            ],
            update_mode: UpdateMode::Freshen,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 1);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(
        !info.entries.iter().any(|entry| entry.path == "new.txt"),
        "仅刷新模式不应加入新条目"
    );
    assert_eq!(
        extract_text(&archive, &dir.path().join("updated"), "hello.txt"),
        "updated"
    );
}

/// 验证同步模式会新增文件、更新较新文件并删除磁盘上已不存在的条目。
#[test]
fn zip_edit_sync_mode_mirrors_source_directory() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    for name in ["a.txt", "b.txt", "c.txt"] {
        let path = source.join(name);
        fs::write(&path, format!("old-{name}")).unwrap();
        set_modified(&path, old);
    }
    let archive = dir.path().join("sync.zip");
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

    // 修改 a、新增 d、删除 b，c 保持不变。
    fs::write(source.join("a.txt"), b"new-a").unwrap();
    fs::write(source.join("d.txt"), b"new-d").unwrap();
    fs::remove_file(source.join("b.txt")).unwrap();
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            roots: vec![EditRoot {
                prefix: "source".to_string(),
                directory: source,
            }],
            update_mode: UpdateMode::Sync,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.deleted, 1, "磁盘上已删除的条目应同步移除");
    assert_eq!(report.skipped, 1, "未变化的文件不应重写");
    let info = ArchiveService::list(&archive, None).unwrap();
    let names: Vec<&str> = info
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert!(names.contains(&"source/a.txt"));
    assert!(names.contains(&"source/c.txt"));
    assert!(names.contains(&"source/d.txt"));
    assert!(!names.contains(&"source/b.txt"));

    let synced = dir.path().join("synced");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: synced.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(synced.join("source").join("a.txt")).unwrap(),
        "new-a"
    );
    assert_eq!(
        fs::read_to_string(synced.join("source").join("c.txt")).unwrap(),
        "old-c.txt"
    );
    assert_eq!(
        fs::read_to_string(synced.join("source").join("d.txt")).unwrap(),
        "new-d"
    );
}
