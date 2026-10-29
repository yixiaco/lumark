//! LZH lh4/lh5/lh7 跨块流式创建、列表和解压。

use std::fs;

use lumark_archive::{
    ArchiveFormat, ArchiveService, CompressionOptions, CompressionPreset, ExtractionOptions,
};
use tempfile::tempdir;

/// 生成跨多个 1 MiB 读取块、包含重复和随机段的测试内容。
fn sample_bytes(size: usize) -> Vec<u8> {
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut bytes = Vec::with_capacity(size);
    let pattern = b"Lumark LZH streaming block ";
    while bytes.len() < size {
        if bytes.len() % (1024 * 1024) < 512 * 1024 {
            let remaining = size - bytes.len();
            let take = remaining.min(pattern.len());
            bytes.extend_from_slice(&pattern[..take]);
        } else {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            bytes.push(state as u8);
        }
    }
    bytes
}

/// 三种预设都应走 lh4/lh5/lh7 流式编码并可完整还原。
#[test]
fn lzh_streaming_presets_round_trip() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    let payload = sample_bytes(6 * 1024 * 1024 + 123);
    fs::write(source.join("payload.bin"), &payload).unwrap();
    fs::write(source.join("note.txt"), b"streaming lzh").unwrap();

    for (preset, name) in [
        (CompressionPreset::Speed, "speed"),
        (CompressionPreset::Balanced, "balanced"),
        (CompressionPreset::Size, "size"),
    ] {
        let archive = directory.path().join(format!("archive-{name}.lzh"));
        ArchiveService::create(
            &archive,
            std::slice::from_ref(&source),
            &CompressionOptions {
                format: ArchiveFormat::Lzh,
                preset,
                test_after: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Lzh);
        let info = ArchiveService::list(&archive, None).unwrap();
        assert_eq!(info.entries.iter().filter(|entry| !entry.is_dir).count(), 2);

        let output = directory.path().join(format!("out-{name}"));
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
            fs::read(output.join("source").join("payload.bin")).unwrap(),
            payload
        );
        assert_eq!(
            fs::read(output.join("source").join("note.txt")).unwrap(),
            b"streaming lzh"
        );
    }
}

/// Store 预设仍使用分块直写，并保留同一头部回填路径。
#[test]
fn lzh_store_round_trip() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("store-source");
    fs::create_dir_all(&source).unwrap();
    let payload = sample_bytes(3 * 1024 * 1024);
    fs::write(source.join("payload.bin"), &payload).unwrap();
    let archive = directory.path().join("store.lzh");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Lzh,
            level: Some(0),
            test_after: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let output = directory.path().join("store-out");
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
        fs::read(output.join("store-source").join("payload.bin")).unwrap(),
        payload
    );
}
