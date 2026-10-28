//! MSI 内嵌 CAB 只读解析集成测试。

use std::fs;
use std::io::{Cursor, Write};

use cab::{CabinetBuilder, CompressionType};
use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use msi::{Column, Insert, Package, PackageType, Value};
use tempfile::tempdir;

/// 生成一个只含 `file1` 的未压缩 CAB。
fn make_cab(content: &[u8]) -> Vec<u8> {
    let mut builder = CabinetBuilder::new();
    builder.add_folder(CompressionType::None).add_file("file1");
    let mut writer = builder.build(Cursor::new(Vec::new())).unwrap();
    while let Some(mut file) = writer.next_file().unwrap() {
        file.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// 生成包含 File/Media 表和内嵌 CAB 的 MSI。
fn make_msi_with_cab(path: &std::path::Path, content: &[u8], cabinet: &str) {
    let cab = make_cab(content);
    let mut package = Package::create(PackageType::Installer, Cursor::new(Vec::new())).unwrap();
    package
        .create_table(
            "File",
            vec![
                Column::build("File").primary_key().string(64),
                Column::build("FileName").string(255),
                Column::build("FileSize").int32(),
            ],
        )
        .expect("create File table");
    package
        .insert_rows(Insert::into("File").row(vec![
            Value::from("file1"),
            Value::from("hello.txt"),
            Value::Int(content.len() as i32),
        ]))
        .unwrap();
    package
        .create_table(
            "Media",
            vec![
                Column::build("DiskId").primary_key().int16(),
                Column::build("LastSequence").int16(),
                Column::build("Cabinet").string(255),
            ],
        )
        .unwrap();
    package
        .insert_rows(Insert::into("Media").row(vec![
            Value::Int(1),
            Value::Int(1),
            Value::from(cabinet),
        ]))
        .unwrap();
    if let Some(name) = cabinet.strip_prefix('#') {
        let mut stream = package.write_stream(name).unwrap();
        stream.write_all(&cab).unwrap();
    } else {
        fs::write(path.parent().unwrap().join(cabinet), &cab).unwrap();
    }
    let cursor = package.into_inner().unwrap();
    fs::write(path, cursor.into_inner()).unwrap();
}

/// 生成使用内嵌 CAB 的最小 MSI。
fn make_msi(path: &std::path::Path, content: &[u8]) {
    make_msi_with_cab(path, content, "#cab1");
}

/// 验证 MSI 识别、列表、解压和完整性测试。
#[test]
fn msi_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("sample.msi");
    make_msi(&archive, b"hello msi");
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Msi);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Msi));
    assert_eq!(info.entries.len(), 1);
    assert_eq!(info.entries[0].path, "hello.txt");
    assert_eq!(info.entries[0].size, 9);

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
    assert_eq!(report.files, 1);
    assert_eq!(
        fs::read_to_string(output.join("hello.txt")).unwrap(),
        "hello msi"
    );
    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 1);
    assert_eq!(tested.bytes, 9);
}

/// 验证 Media 表引用同目录外部 CAB 时也能列表和解压。
#[test]
fn msi_reads_external_cab() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("external.msi");
    make_msi_with_cab(&archive, b"external msi", "external.cab");
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.entries.len(), 1);
    assert_eq!(info.entries[0].path, "hello.txt");
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
        fs::read_to_string(output.join("hello.txt")).unwrap(),
        "external msi"
    );
}
