//! 归档头部的确定性变异测试。
//!
//! 这不是 libFuzzer，而是把有效归档的随机字节篡改后交给列表接口，确保
//! 解析器只返回错误而不会 panic。可作为持续 fuzz 目标的最小回归基线。

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions};
use tempfile::tempdir;

/// 创建一个小归档并返回路径。
fn make_archive(root: &std::path::Path, format: ArchiveFormat) -> std::path::PathBuf {
    let source = root.join("source.txt");
    fs::write(&source, b"fuzz fixture").unwrap();
    let archive = root.join(format!("fixture.{}", format.extension()));
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
    archive
}

/// 对同一份有效 ZIP 做确定性字节变异，列表和测试不得 panic。
#[test]
fn mutated_archive_headers_never_panic() {
    let directory = tempdir().unwrap();
    for format in [
        ArchiveFormat::Zip,
        ArchiveFormat::SevenZip,
        ArchiveFormat::Tar,
        ArchiveFormat::Gzip,
    ] {
        let archive = make_archive(directory.path(), format);
        let original = fs::read(&archive).unwrap();
        for step in 0..64usize {
            let mut bytes = original.clone();
            let index = (step * 37 + 11) % bytes.len();
            bytes[index] ^= (step as u8).wrapping_mul(31).max(1);
            let path = directory
                .path()
                .join(format!("mutated-{format:?}-{step}.{}", format.extension()));
            fs::write(&path, &bytes).unwrap();
            let listed = catch_unwind(AssertUnwindSafe(|| ArchiveService::list(&path, None)));
            assert!(listed.is_ok(), "{format:?} 第 {step} 次变异导致列表 panic");
            let tested = catch_unwind(AssertUnwindSafe(|| {
                ArchiveService::test(&path, None, |_| {})
            }));
            assert!(tested.is_ok(), "{format:?} 第 {step} 次变异导致测试 panic");
        }
    }
}

/// 对截断归档做列表和解压，不得 panic 或写出越界路径。
#[test]
fn truncated_archives_return_errors_without_panics() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path(), ArchiveFormat::Zip);
    let original = fs::read(&archive).unwrap();
    for fraction in [1usize, 3, 7, 15, 31, 63] {
        let length = original.len() * fraction / 64;
        let path = directory.path().join(format!("truncated-{fraction}.zip"));
        fs::write(&path, &original[..length]).unwrap();
        let listed = catch_unwind(AssertUnwindSafe(|| ArchiveService::list(&path, None)));
        assert!(listed.is_ok(), "截断 {fraction}/64 导致列表 panic");
        let output = directory.path().join(format!("out-{fraction}"));
        let extracted = catch_unwind(AssertUnwindSafe(|| {
            ArchiveService::extract(
                &path,
                &ExtractionOptions {
                    output_dir: output.clone(),
                    overwrite: true,
                    ..Default::default()
                },
                |_| {},
            )
        }));
        assert!(extracted.is_ok(), "截断 {fraction}/64 导致解压 panic");
    }
}

/// 构造 SquashFS 测试镜像。
fn squashfs_bytes() -> Vec<u8> {
    use backhand::compression::Compressor;
    use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
    use std::io::Cursor;

    let mut writer = FilesystemWriter::default();
    writer.set_compressor(FilesystemCompressor::new(Compressor::Gzip, None).unwrap());
    writer
        .push_file(
            Cursor::new(b"fuzz squashfs"),
            "hello.txt",
            NodeHeader::default(),
        )
        .unwrap();
    let mut bytes = Vec::new();
    writer.write(Cursor::new(&mut bytes)).unwrap();
    bytes
}

/// 构造 HFS+ DMG 测试镜像。
fn dmg_bytes() -> Vec<u8> {
    use std::io::Cursor;

    let mut builder = hfsplus::testutil::HfsPlusImageBuilder::new();
    builder.add_file("hello.txt", b"fuzz dmg", 0o644);
    let hfs = builder.build();
    let mut bytes = Vec::new();
    let mut writer = udif::DmgWriter::new(Cursor::new(&mut bytes))
        .compression(udif::CompressionMethod::Raw)
        .skip_checksums(true);
    writer.add_partition("Apple_HFSX", &hfs).unwrap();
    writer.finish().unwrap();
    bytes
}

/// 对 NSIS、SquashFS、DMG 依次做单字节变异，解析器不得 panic。
#[test]
fn mutated_new_formats_never_panic() {
    let directory = tempdir().unwrap();
    let fixtures = [
        (
            "nsis.exe",
            include_bytes!("fixtures/nsis/installer.exe").to_vec(),
        ),
        ("image.squashfs", squashfs_bytes()),
        ("image.dmg", dmg_bytes()),
    ];
    for (name, original) in fixtures {
        for step in 0..32usize {
            let mut bytes = original.clone();
            let index = (step * 41 + 7) % bytes.len();
            bytes[index] ^= (step as u8).wrapping_mul(17).max(1);
            let path = directory.path().join(format!("{step}-{name}"));
            fs::write(&path, &bytes).unwrap();
            let listed = catch_unwind(AssertUnwindSafe(|| ArchiveService::list(&path, None)));
            assert!(listed.is_ok(), "{name} 第 {step} 次变异导致列表 panic");
            let tested = catch_unwind(AssertUnwindSafe(|| {
                ArchiveService::extract(
                    &path,
                    &ExtractionOptions {
                        output_dir: directory.path().join(format!("out-{step}-{name}")),
                        overwrite: true,
                        ..Default::default()
                    },
                    |_| {},
                )
            }));
            assert!(tested.is_ok(), "{name} 第 {step} 次变异导致解压 panic");
        }
    }
}

/// 构造只含一个 Store 条目的最小 EGG 样本。
fn egg_bytes() -> Vec<u8> {
    const EGG_HEADER: u32 = 0x4147_4745;
    const END_MARKER: u32 = 0x08E2_8222;
    const FILE_HEADER: u32 = 0x0A85_90E3;
    const FILENAME: u32 = 0x0A85_91AC;
    const BLOCK_HEADER: u32 = 0x02B5_0C13;

    let data = b"fuzz egg";
    let mut bytes = Vec::new();
    push_egg_u32(&mut bytes, EGG_HEADER);
    push_egg_u16(&mut bytes, 1);
    push_egg_u32(&mut bytes, 1);
    push_egg_u32(&mut bytes, 0);
    push_egg_u32(&mut bytes, END_MARKER);
    push_egg_u32(&mut bytes, FILE_HEADER);
    push_egg_u32(&mut bytes, 1);
    bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
    push_egg_u32(&mut bytes, FILENAME);
    bytes.push(0);
    push_egg_u16(&mut bytes, 9);
    bytes.extend_from_slice(b"hello.txt");
    push_egg_u32(&mut bytes, END_MARKER);
    push_egg_u32(&mut bytes, BLOCK_HEADER);
    bytes.push(0);
    bytes.push(0);
    push_egg_u32(&mut bytes, data.len() as u32);
    push_egg_u32(&mut bytes, data.len() as u32);
    push_egg_u32(&mut bytes, crc32fast::hash(data));
    push_egg_u32(&mut bytes, END_MARKER);
    bytes.extend_from_slice(data);
    push_egg_u32(&mut bytes, END_MARKER);
    bytes
}

/// 追加 EGG 测试用的小端 u16。
fn push_egg_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// 追加 EGG 测试用的小端 u32。
fn push_egg_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// 对 EGG 做确定性变异，列表和解压不得 panic。
#[test]
fn mutated_egg_never_panics() {
    let directory = tempdir().unwrap();
    let original = egg_bytes();
    for step in 0..32usize {
        let mut bytes = original.clone();
        let index = (step * 43 + 5) % bytes.len();
        bytes[index] ^= (step as u8).wrapping_mul(19).max(1);
        let path = directory.path().join(format!("mutated-{step}.egg"));
        fs::write(&path, &bytes).unwrap();
        let listed = catch_unwind(AssertUnwindSafe(|| ArchiveService::list(&path, None)));
        assert!(listed.is_ok(), "EGG 第 {step} 次变异导致列表 panic");
        let extracted = catch_unwind(AssertUnwindSafe(|| {
            ArchiveService::extract(
                &path,
                &ExtractionOptions {
                    output_dir: directory.path().join(format!("out-{step}")),
                    overwrite: true,
                    ..Default::default()
                },
                |_| {},
            )
        }));
        assert!(extracted.is_ok(), "EGG 第 {step} 次变异导致解压 panic");
    }
}
