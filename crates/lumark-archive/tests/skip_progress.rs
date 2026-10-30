//! 跳过同名文件时仍上报进度，避免拖放进度条停在起点。

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use lumark_archive::{
    ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions, OverwritePolicy,
};
use tempfile::tempdir;

/// 创建包含一个文件的归档并预置同名目标文件。
fn prepare(
    directory: &std::path::Path,
    format: ArchiveFormat,
    extension: &str,
) -> (PathBuf, PathBuf, PathBuf) {
    let source = directory.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("hello.txt"), b"new contents").unwrap();
    let archive = directory.join(format!("sample.{extension}"));
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
    let output = directory.join("out");
    let existing = output.join("source").join("hello.txt");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"old contents").unwrap();
    (archive, output, existing)
}

/// 验证 ZIP 和 ASAR 在跳过同名文件时都会触发进度回调。
#[test]
fn skip_reports_progress_for_zip_and_asar() {
    for (format, extension) in [(ArchiveFormat::Zip, "zip"), (ArchiveFormat::Asar, "asar")] {
        let directory = tempdir().unwrap();
        let (archive, output, existing) = prepare(directory.path(), format, extension);
        let progress = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&progress);
        let report = ArchiveService::extract(
            &archive,
            &ExtractionOptions {
                output_dir: output,
                overwrite_policy: OverwritePolicy::Skip,
                overwrite: false,
                ..Default::default()
            },
            move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            },
        )
        .unwrap();
        assert_eq!(report.skipped, 1, "{extension}");
        assert_eq!(fs::read(existing).unwrap(), b"old contents", "{extension}");
        assert!(progress.load(Ordering::Relaxed) > 0, "{extension}");
    }
}

/// 验证 ZIP 和 ASAR 的覆盖策略会真正替换同名文件。
#[test]
fn overwrite_replaces_existing_files_for_zip_and_asar() {
    for (format, extension) in [(ArchiveFormat::Zip, "zip"), (ArchiveFormat::Asar, "asar")] {
        let directory = tempdir().unwrap();
        let (archive, output, existing) = prepare(directory.path(), format, extension);
        let progress = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&progress);
        let report = ArchiveService::extract(
            &archive,
            &ExtractionOptions {
                output_dir: output,
                overwrite_policy: OverwritePolicy::Overwrite,
                overwrite: true,
                ..Default::default()
            },
            move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            },
        )
        .unwrap();
        assert_eq!(report.files, 1, "{extension}");
        assert_eq!(report.skipped, 0, "{extension}");
        assert_eq!(fs::read(existing).unwrap(), b"new contents", "{extension}");
        assert!(progress.load(Ordering::Relaxed) > 0, "{extension}");
    }
}
