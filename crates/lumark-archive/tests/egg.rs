//! EGG 只读解析集成测试。

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 构造只含一个 Store 条目的最小 EGG。
fn make_egg(path: &std::path::Path, name: &str, data: &[u8]) {
    const EGG_HEADER: u32 = 0x4147_4745;
    const END_MARKER: u32 = 0x08E2_8222;
    const FILE_HEADER: u32 = 0x0A85_90E3;
    const FILENAME: u32 = 0x0A85_91AC;
    const BLOCK_HEADER: u32 = 0x02B5_0C13;

    let mut bytes = Vec::new();
    push_u32(&mut bytes, EGG_HEADER);
    push_u16(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, END_MARKER);

    push_u32(&mut bytes, FILE_HEADER);
    push_u32(&mut bytes, 1);
    bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
    push_u32(&mut bytes, FILENAME);
    bytes.push(0); // extra flags：u16 长度
    push_u16(&mut bytes, name.len() as u16);
    bytes.extend_from_slice(name.as_bytes());
    push_u32(&mut bytes, END_MARKER);

    push_u32(&mut bytes, BLOCK_HEADER);
    bytes.push(0); // Store
    bytes.push(0);
    push_u32(&mut bytes, data.len() as u32);
    push_u32(&mut bytes, data.len() as u32);
    push_u32(&mut bytes, crc32fast::hash(data));
    push_u32(&mut bytes, END_MARKER);
    bytes.extend_from_slice(data);
    push_u32(&mut bytes, END_MARKER);
    fs::write(path, bytes).unwrap();
}

/// 追加小端 u16。
fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// 追加小端 u32。
fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// 验证 EGG 识别、列表、解压和完整性测试。
#[test]
fn egg_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("sample.egg");
    make_egg(&archive, "hello.txt", b"hello egg");
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Egg);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Egg));
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
        "hello egg"
    );
    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 1);
    assert_eq!(tested.bytes, 9);
}
