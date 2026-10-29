//! WIM 创建（LZX/LZMS）、列表、解压和测试。

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions};
use tempfile::tempdir;
use wim::{Compression, OpenOptions, Wim};

/// 验证 LZX WIM 创建后可由只读后端打开并还原目录内容。
#[test]
fn compressed_wim_round_trip() {
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
    let reopened = Wim::open(&archive, OpenOptions::default()).unwrap();
    assert_eq!(reopened.info().unwrap().compression, Compression::Lzx);
    drop(reopened);
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

/// 生成确定性的伪随机内容，避免 WIM 去重或过度压缩影响分卷测试。
fn payload(seed: u64, size: usize) -> Vec<u8> {
    let mut state = seed | 1;
    let mut bytes = Vec::with_capacity(size);
    for _ in 0..size {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        bytes.push(state as u8);
    }
    bytes
}

/// 验证 LZMS 压缩的 WIM 可列表、解压并通过内容校验。
#[test]
fn lzms_wim_round_trip() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("lzms-source");
    fs::create_dir_all(&source).unwrap();
    let expected = payload(0x4c5a4d53, 256 * 1024);
    fs::write(source.join("payload.bin"), &expected).unwrap();
    let archive = directory.path().join("lzms.wim");
    let mut image = Wim::new(Compression::Lzms).unwrap();
    image.capture_image(&source).unwrap();
    image.write(&archive).unwrap();
    drop(image);

    let reopened = Wim::open(&archive, OpenOptions::default()).unwrap();
    assert_eq!(
        reopened.info().unwrap().compression,
        Compression::Lzms,
        "测试 WIM 必须真的使用 LZMS"
    );
    drop(reopened);

    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("payload.bin"))
    );
    let output = directory.path().join("lzms-out");
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
    let restored = fs::read(output.join("payload.bin")).unwrap();
    assert_eq!(restored, expected);
}

/// 验证 LZX 压缩 WIM 拆分出的多分卷可自动发现、列表和解压。
#[test]
fn split_wim_round_trip() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("split-source");
    fs::create_dir_all(&source).unwrap();
    let mut expected = Vec::new();
    for index in 0..4u64 {
        let bytes = payload(0x53574d00 + index, 200 * 1024);
        expected.push(bytes.clone());
        fs::write(source.join(format!("part-{index}.bin")), bytes).unwrap();
    }
    let whole = directory.path().join("whole.wim");
    let mut image = Wim::new(Compression::Lzx).unwrap();
    image.capture_image(&source).unwrap();
    image.write(&whole).unwrap();
    drop(image);

    let bytes = fs::read(&whole).unwrap();
    let parts = wim_format::split_join::split_archive(&bytes, 250 * 1024, [0x5a; 16], false)
        .expect("split WIM");
    assert!(parts.len() > 1, "测试必须生成多个 SWM 分卷");
    let first = directory.path().join("archive.swm");
    for (index, part) in parts.iter().enumerate() {
        let path = if index == 0 {
            first.clone()
        } else {
            directory.path().join(format!("archive{}.swm", index + 1))
        };
        fs::write(path, part).unwrap();
    }

    assert_eq!(ArchiveService::detect(&first), ArchiveFormat::Wim);
    let info = ArchiveService::list(&first, None).unwrap();
    assert_eq!(info.entries.iter().filter(|entry| !entry.is_dir).count(), 4);
    let output = directory.path().join("split-out");
    ArchiveService::extract(
        &first,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    for (index, bytes) in expected.iter().enumerate() {
        let restored = fs::read(output.join(format!("part-{index}.bin"))).unwrap();
        assert_eq!(&restored, bytes, "分卷条目 {index} 内容不一致");
    }
}
