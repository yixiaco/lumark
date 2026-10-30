//! 解压进度必须区分“当前文件”和“全部文件”。

use std::fs;

use lumark_archive::{
    ArchiveFormat, ArchiveService, CompressionOptions, ExtractProgress, ExtractionOptions,
};
use tempfile::tempdir;

/// 验证 ZIP 和 ASAR 在解压大文件时上报当前文件进度。
#[test]
fn current_file_progress_differs_from_total() {
    const BIG: usize = 2 * 1024 * 1024;
    for (format, extension) in [(ArchiveFormat::Zip, "zip"), (ArchiveFormat::Asar, "asar")] {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("big.bin"), vec![0x5a; BIG]).unwrap();
        fs::write(source.join("small.txt"), b"small").unwrap();
        let archive = directory.path().join(format!("sample.{extension}"));
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

        let output = directory.path().join("out");
        let mut events: Vec<ExtractProgress> = Vec::new();
        ArchiveService::extract(
            &archive,
            &ExtractionOptions {
                output_dir: output.clone(),
                ..Default::default()
            },
            |progress| events.push(progress),
        )
        .unwrap();

        assert!(
            events.iter().any(|event| {
                event.current_file_total == BIG as u64
                    && event.current_file_done > 0
                    && event.current_file_done < BIG as u64
            }),
            "{extension}: 没有收到当前文件的中间进度"
        );
        assert!(
            events.iter().any(|event| {
                event.current_file_total == BIG as u64
                    && event.bytes_total > event.current_file_total
            }),
            "{extension}: 当前文件大小与总大小没有区分"
        );
        assert!(output.join("source").join("big.bin").is_file());
    }
}

/// 确认 `ExtractProgress` 的默认字段不会影响现有调用。
#[test]
fn default_progress_fields_are_zero() {
    let progress = ExtractProgress {
        current: "x".to_string(),
        bytes_done: 0,
        bytes_total: 0,
        entries_done: 0,
        entries_total: 0,
        current_file_done: 0,
        current_file_total: 0,
    };
    assert_eq!(progress.current_file_done, 0);
    assert_eq!(progress.current_file_total, 0);
}
