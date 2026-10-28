//! 归档头部的确定性变异测试。
//!
//! 这不是 libFuzzer，而是把有效归档的随机字节篡改后交给列表接口，确保
//! 解析器只返回错误而不会 panic。可作为持续 fuzz 目标的最小回归基线。

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions};
use tempfile::tempdir;

/// 创建一个小 ZIP 并返回路径。
fn make_zip(root: &std::path::Path) -> std::path::PathBuf {
    let source = root.join("source.txt");
    fs::write(&source, b"fuzz fixture").unwrap();
    let archive = root.join("fixture.zip");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    archive
}

/// 对同一份有效 ZIP 做确定性字节变异，列表和测试不得 panic。
#[test]
fn mutated_zip_headers_never_panic() {
    let directory = tempdir().unwrap();
    let archive = make_zip(directory.path());
    let original = fs::read(&archive).unwrap();
    for step in 0..256usize {
        let mut bytes = original.clone();
        let index = (step * 37 + 11) % bytes.len();
        bytes[index] ^= (step as u8).wrapping_mul(31).max(1);
        let path = directory.path().join(format!("mutated-{step}.zip"));
        fs::write(&path, &bytes).unwrap();
        let listed = catch_unwind(AssertUnwindSafe(|| ArchiveService::list(&path, None)));
        assert!(listed.is_ok(), "第 {step} 次变异导致列表 panic");
        let tested = catch_unwind(AssertUnwindSafe(|| {
            ArchiveService::test(&path, None, |_| {})
        }));
        assert!(tested.is_ok(), "第 {step} 次变异导致测试 panic");
    }
}

/// 对截断归档做列表和解压，不得 panic 或写出越界路径。
#[test]
fn truncated_archives_return_errors_without_panics() {
    let directory = tempdir().unwrap();
    let archive = make_zip(directory.path());
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
