//! RPM 包只读解析集成测试。

use std::fs;
use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 构造 16 字节 RPM header（count=0、data=0）。
fn write_empty_header(file: &mut fs::File, magic: &[u8; 3]) {
    file.write_all(magic).unwrap();
    file.write_all(&[1, 0, 0, 0, 0]).unwrap();
    file.write_all(&0u32.to_be_bytes()).unwrap();
    file.write_all(&0u32.to_be_bytes()).unwrap();
}

/// 追加一个 newc cpio 条目。
fn write_cpio_entry(out: &mut Vec<u8>, name: &str, mode: u32, data: &[u8]) {
    let namesize = name.len() + 1;
    out.extend_from_slice(b"070701");
    for value in [
        1,
        mode as u64,
        0,
        0,
        1,
        0,
        data.len() as u64,
        0,
        0,
        0,
        0,
        namesize as u64,
        0,
    ] {
        out.extend_from_slice(format!("{value:08x}").as_bytes());
    }
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// 生成包含一个文本文件的最小 RPM。
fn make_rpm(path: &std::path::Path, content: &[u8]) {
    let mut cpio = Vec::new();
    write_cpio_entry(&mut cpio, "hello.txt", 0o100644, content);
    write_cpio_entry(&mut cpio, "TRAILER!!!", 0, b"");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&cpio).unwrap();
    let payload = encoder.finish().unwrap();

    let mut file = fs::File::create(path).unwrap();
    let mut lead = [0u8; 96];
    lead[0..4].copy_from_slice(&[0xed, 0xab, 0xee, 0xdb]);
    file.write_all(&lead).unwrap();
    write_empty_header(&mut file, &[0x8e, 0xad, 0xe8]);
    write_empty_header(&mut file, &[0x8e, 0xad, 0xe8]);
    file.write_all(&payload).unwrap();
}

/// 验证 RPM 识别、列表、解压和完整性测试。
#[test]
fn rpm_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("sample.rpm");
    make_rpm(&archive, b"hello rpm");
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Rpm);
    assert_eq!(
        lumark_archive::format::detect_magic(&fs::read(&archive).unwrap()[..4]),
        ArchiveFormat::Rpm
    );
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Rpm));
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
        "hello rpm"
    );
    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 1);
    assert_eq!(tested.bytes, 9);
}
