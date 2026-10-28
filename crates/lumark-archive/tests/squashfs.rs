//! SquashFS 只读解析集成测试。

use std::fs;
use std::io::Cursor;

use backhand::compression::Compressor;
use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 验证 SquashFS 识别、列表、解压和完整性测试。
#[test]
fn squashfs_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("image.squashfs");
    let mut writer = FilesystemWriter::default();
    writer.set_compressor(FilesystemCompressor::new(Compressor::Gzip, None).unwrap());
    writer.push_dir("source", NodeHeader::default()).unwrap();
    writer
        .push_dir("source/nested", NodeHeader::default())
        .unwrap();
    writer
        .push_file(
            Cursor::new(b"hello squashfs"),
            "source/hello.txt",
            NodeHeader::default(),
        )
        .map_err(|error| format!("{error:?}"))
        .expect("first push");
    writer
        .push_file(
            Cursor::new(b"nested data"),
            "source/nested/data.bin",
            NodeHeader::default(),
        )
        .expect("second push");
    writer.write(fs::File::create(&archive).unwrap()).unwrap();

    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::SquashFs);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::SquashFs));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.contains("hello.txt"))
    );

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
        fs::read_to_string(output.join("source").join("hello.txt")).unwrap(),
        "hello squashfs"
    );
    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 2);
    assert_eq!(tested.bytes, report.bytes);
}

/// 验证 XZ 压缩的 SquashFS 可以列表和解压。
#[test]
fn squashfs_xz_lists_and_extracts() {
    use backhand::FilesystemWriter;
    use std::io::Cursor;

    let directory = tempdir().unwrap();
    let archive = directory.path().join("image-xz.squashfs");
    let mut writer = FilesystemWriter::default();
    writer.set_compressor(
        backhand::FilesystemCompressor::new(backhand::compression::Compressor::Xz, None).unwrap(),
    );
    writer
        .push_file(
            Cursor::new(b"xz squashfs"),
            "hello.txt",
            backhand::NodeHeader::default(),
        )
        .unwrap();
    writer.write(fs::File::create(&archive).unwrap()).unwrap();

    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));
    let output = directory.path().join("out-xz");
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
        fs::read_to_string(output.join("hello.txt")).unwrap(),
        "xz squashfs"
    );
}
