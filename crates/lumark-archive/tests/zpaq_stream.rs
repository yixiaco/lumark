//! ZPAQ 流式归档写入器生成的文件应能被现有列表/解压后端读取。

use std::fs::{self, File};

use lumark_archive::{ArchiveService, ExtractionOptions};
use tempfile::tempdir;
use zpaq_rs::ArchiveWriter;

/// 验证多 segment 流式归档可被 zpaq list/extract 识别。
#[test]
fn streaming_archive_lists_and_extracts() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("stream.zpaq");
    let file = File::create(&archive).unwrap();
    let mut writer = ArchiveWriter::new(file, "1").unwrap();
    writer
        .add_entry(
            "docs/readme.txt",
            None,
            std::io::Cursor::new(b"streaming zpaq"),
        )
        .unwrap();
    writer
        .add_entry("data.bin", None, std::io::Cursor::new(vec![0x5a; 200_000]))
        .unwrap();
    writer.finish().unwrap();

    let info = ArchiveService::list(&archive, None).unwrap();
    let names = info
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"docs/readme.txt"), "entries: {names:?}");
    assert!(names.contains(&"data.bin"), "entries: {names:?}");

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
        fs::read(output.join("docs").join("readme.txt")).unwrap(),
        b"streaming zpaq"
    );
    assert_eq!(
        fs::read(output.join("data.bin")).unwrap(),
        vec![0x5a; 200_000]
    );
    ArchiveService::test(&archive, None, |_| {}).unwrap();
}
