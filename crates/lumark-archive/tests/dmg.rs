//! DMG 只读解析集成测试。

use std::fs;
use std::io::Cursor;

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 用原始 HFS+ 分区构造最小 DMG。
fn make_dmg(path: &std::path::Path) {
    let mut builder = hfsplus::testutil::HfsPlusImageBuilder::new();
    builder.add_file("hello.txt", b"hello dmg", 0o644);
    builder.add_file("data.bin", b"12345", 0o644);
    let hfs_image = builder.build();
    let mut bytes = Vec::new();
    {
        let cursor = Cursor::new(&mut bytes);
        let mut writer = udif::DmgWriter::new(cursor)
            .compression(udif::CompressionMethod::Raw)
            .skip_checksums(true);
        writer.add_partition("Apple_HFSX", &hfs_image).unwrap();
        writer.finish().unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/// 验证 DMG 识别、列表、解压和完整性测试。
#[test]
fn dmg_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("image.dmg");
    make_dmg(&archive);
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Dmg);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Dmg));
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));

    let output = directory.path().join("out");
    let report = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.files, 2);
    assert_eq!(
        fs::read_to_string(output.join("hello.txt")).unwrap(),
        "hello dmg"
    );
    assert_eq!(fs::read(output.join("data.bin")).unwrap(), b"12345");
    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, report.files);
    assert_eq!(tested.bytes, report.bytes);
}
