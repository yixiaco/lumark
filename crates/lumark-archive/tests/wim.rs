//! 未压缩 WIM 创建、列表、解压和测试。

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions};
use tempfile::tempdir;

/// 验证 WIM 创建后可由只读后端打开并还原目录内容。
#[test]
fn uncompressed_wim_round_trip() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("hello.txt"), b"hello wim").unwrap();
    fs::write(source.join("nested").join("data.bin"), b"12345").unwrap();
    let archive = directory.path().join("archive.wim");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Wim,
            test_after: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Wim);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Wim));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );

    let output = directory.path().join("out");
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
        fs::read_to_string(output.join("source").join("hello.txt")).unwrap(),
        "hello wim"
    );
    assert_eq!(
        fs::read(output.join("source").join("nested").join("data.bin")).unwrap(),
        b"12345"
    );
}
