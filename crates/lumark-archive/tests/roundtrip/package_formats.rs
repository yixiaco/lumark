//! APK/JAR/PAX 容器读写回归测试。

use super::*;
use zeroize::Zeroizing;

/// 创建包含中文长路径的测试源目录。
fn package_source(root: &std::path::Path) -> PathBuf {
    let source = root.join("package-source");
    fs::create_dir_all(source.join("docs").join("中文目录")).unwrap();
    fs::write(
        source.join("docs").join("中文目录").join("说明文档.txt"),
        "pax 内容",
    )
    .unwrap();
    fs::write(source.join("hello.txt"), b"package hello").unwrap();
    source
}

/// 验证 APK 以 ZIP 容器创建、带密码读写并保持格式识别。
#[test]
fn apk_roundtrip_with_password() {
    let directory = tempdir().unwrap();
    let source = package_source(directory.path());
    let archive = directory.path().join("sample.apk");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Apk,
            password: Some(Zeroizing::new("apk-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Apk);
    let info = ArchiveService::list(&archive, Some("apk-pass")).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Apk));
    assert!(info.encrypted);
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("说明文档.txt"))
    );

    let output = directory.path().join("apk-out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            password: Some(Zeroizing::new("apk-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(
        fs::read_dir(&output)
            .unwrap()
            .flatten()
            .any(|entry| entry.path().join("hello.txt").is_file())
    );
}

/// 验证 JAR 复用 ZIP 注释和读取路径，并以 `.jar` 作为输出扩展名。
#[test]
fn jar_roundtrip_keeps_comment() {
    let directory = tempdir().unwrap();
    let source = package_source(directory.path());
    let archive = directory.path().join("sample.jar");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Jar,
            comment: Some("jar comment".to_string()),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Jar);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Jar));
    assert_eq!(info.comment.as_deref(), Some("jar comment"));
}

/// 验证 PAX 创建与读取，并确认写出了 pax 扩展头。
#[test]
fn pax_roundtrip_writes_extensions() {
    let directory = tempdir().unwrap();
    let source = package_source(directory.path());
    let archive = directory.path().join("sample.pax");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Pax,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Pax);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Pax));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("说明文档.txt"))
    );

    let file = fs::File::open(&archive).unwrap();
    let mut reader = tar::Archive::new(file);
    let mut found = false;
    for entry in reader.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().replace('\\', "/");
        if !path.ends_with("说明文档.txt") {
            continue;
        }
        found = true;
        let extensions = entry.pax_extensions().unwrap().expect("缺少 pax 扩展头");
        let keys = extensions
            .map(|extension| extension.unwrap().key().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"path".to_string()));
        assert!(keys.contains(&"mtime".to_string()));
    }
    assert!(found, "PAX 中没有找到中文路径条目");

    let output = directory.path().join("pax-out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
}

/// 验证三种扩展名在没有魔数时也能正确识别。
#[test]
fn package_extensions_are_detected() {
    let directory = tempdir().unwrap();
    for (name, expected) in [
        ("sample.apk", ArchiveFormat::Apk),
        ("sample.jar", ArchiveFormat::Jar),
        ("sample.pax", ArchiveFormat::Pax),
    ] {
        let path = directory.path().join(name);
        fs::write(&path, b"placeholder").unwrap();
        assert_eq!(ArchiveService::detect(&path), expected, "{name}");
    }
}
