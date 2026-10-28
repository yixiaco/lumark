//! NSIS 只读解析集成测试。
//!
//! 测试用安装包来自 nsis-rs 项目的 Apache-2.0 测试夹具，见
//! `fixtures/nsis/NOTICE.txt`。

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 验证 NSIS 识别、列表、解压和完整性测试。
#[test]
fn nsis_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("installer.exe");
    fs::write(&archive, include_bytes!("fixtures/nsis/installer.exe")).unwrap();
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Nsis);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Nsis));
    eprintln!(
        "entries: {:?}",
        info.entries
            .iter()
            .map(|entry| (&entry.path, entry.size))
            .collect::<Vec<_>>()
    );
    assert!(!info.entries.is_empty());

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
    assert_eq!(report.files, info.entries.len());
    assert!(report.bytes > 0);
    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, report.files);
    assert_eq!(tested.bytes, report.bytes);
}
