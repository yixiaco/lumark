//! EGG 只读解析集成测试。

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 构造只含一个 Store 条目的最小 EGG。
fn make_egg_with_method(path: &std::path::Path, name: &str, data: &[u8], method: u8) {
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
    bytes.push(method);
    bytes.push(0);
    push_u32(&mut bytes, data.len() as u32);
    push_u32(&mut bytes, data.len() as u32);
    push_u32(&mut bytes, crc32fast::hash(data));
    push_u32(&mut bytes, END_MARKER);
    bytes.extend_from_slice(data);
    push_u32(&mut bytes, END_MARKER);
    fs::write(path, bytes).unwrap();
}

/// 构造 Store 条目的最小 EGG。
fn make_egg(path: &std::path::Path, name: &str, data: &[u8]) {
    make_egg_with_method(path, name, data, 0);
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

/// AZO 条目在缺少解码器时应明确报不支持，而不是 panic 或误报 CRC。
#[test]
fn azo_entries_report_unsupported() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("azo.egg");
    make_egg_with_method(&archive, "hello.bin", b"azo", 3);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.entries[0].method, "AZO");
    let error = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: directory.path().join("out"),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("AZO"), "{error}");
}

/// 把单卷 EGG 拆成两卷，验证自动分卷发现。
#[test]
fn egg_discovers_split_volumes() {
    let directory = tempdir().unwrap();
    let base_path = directory.path().join("base.egg");
    make_egg(&base_path, "hello.txt", b"split egg data");
    let base = fs::read(&base_path).unwrap();
    let split = base.len() / 2;
    let mut first = Vec::new();
    first.extend_from_slice(&base[..14]);
    push_u32(&mut first, 0x24F5_A262);
    first.push(0);
    push_u16(&mut first, 8);
    push_u32(&mut first, 0);
    push_u32(&mut first, 2);
    push_u32(&mut first, 0x08E2_8222);
    first.extend_from_slice(&base[14..split]);

    let mut second = Vec::new();
    second.extend_from_slice(&base[..6]);
    second.extend_from_slice(&2u32.to_le_bytes());
    second.extend_from_slice(&base[10..14]);
    push_u32(&mut second, 0x08E2_8222);
    second.extend_from_slice(&base[split..]);

    let first_path = directory.path().join("part1.egg");
    let second_path = directory.path().join("part2.egg");
    fs::write(&first_path, first).unwrap();
    fs::write(&second_path, second).unwrap();

    let info = ArchiveService::list(&first_path, None).unwrap();
    assert_eq!(info.entries[0].path, "hello.txt");
    let output = directory.path().join("out");
    ArchiveService::extract(
        &first_path,
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
        "split egg data"
    );
}

/// 构造带空块的目录条目，验证 EGG 目录解析。
#[test]
fn egg_directory_entry_with_empty_block() {
    const EGG_HEADER: u32 = 0x4147_4745;
    const END_MARKER: u32 = 0x08E2_8222;
    const FILE_HEADER: u32 = 0x0A85_90E3;
    const FILENAME: u32 = 0x0A85_91AC;
    const POSIX_INFO: u32 = 0x1EE9_22E5;
    const BLOCK_HEADER: u32 = 0x02B5_0C13;

    let directory = tempdir().unwrap();
    let archive = directory.path().join("dir.egg");
    let mut bytes = Vec::new();
    push_u32(&mut bytes, EGG_HEADER);
    push_u16(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, END_MARKER);
    push_u32(&mut bytes, FILE_HEADER);
    push_u32(&mut bytes, 1);
    bytes.extend_from_slice(&0u64.to_le_bytes());
    push_u32(&mut bytes, FILENAME);
    bytes.push(0);
    push_u16(&mut bytes, 5);
    bytes.extend_from_slice(b"empty");
    push_u32(&mut bytes, POSIX_INFO);
    bytes.push(0);
    push_u16(&mut bytes, 20);
    push_u32(&mut bytes, 0o040755);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes.extend_from_slice(&0u64.to_le_bytes());
    push_u32(&mut bytes, END_MARKER);
    push_u32(&mut bytes, BLOCK_HEADER);
    bytes.push(0);
    bytes.push(0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, END_MARKER);
    push_u32(&mut bytes, END_MARKER);
    fs::write(&archive, bytes).unwrap();

    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.is_dir));
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
    assert!(output.join("empty").is_dir());
}
