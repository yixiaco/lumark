//! CHM 只读解析集成测试。
//!
//! 仓库不内置微软帮助文件；测试只在系统存在标准 CHM 样例时运行。

use std::path::PathBuf;

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 查找系统自带的 CHM 样例。
fn sample_chm() -> Option<PathBuf> {
    [
        r"C:\Windows\Help\mui\0804\mmc.CHM",
        r"C:\Windows\Help\mmc.CHM",
        r"C:\Windows\Help\mui\0409\mmc.CHM",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

/// 验证 CHM 识别、列表、解压和完整性测试。
#[test]
fn chm_lists_extracts_and_tests() {
    let Some(sample) = sample_chm() else {
        eprintln!("系统中没有 CHM 样例，跳过");
        return;
    };
    assert_eq!(ArchiveService::detect(&sample), ArchiveFormat::Chm);
    let info = ArchiveService::list(&sample, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Chm));
    assert!(info.entries.iter().any(|entry| !entry.is_dir));

    let directory = tempdir().unwrap();
    let output = directory.path().join("out");
    let report = ArchiveService::extract(
        &sample,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(report.files > 0);
    let tested = ArchiveService::test(&sample, None, |_| {}).unwrap();
    assert_eq!(tested.files, report.files);
    assert_eq!(tested.bytes, report.bytes);
}
