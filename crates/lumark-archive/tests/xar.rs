//! XAR/PKG 只读解析集成测试。

use std::fs;
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 用 zlib 压缩一段字节。
fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// 生成一个包含文件、目录和符号链接的最小 XAR。
fn make_xar(path: &std::path::Path, payload: &[u8]) {
    let compressed_payload = zlib(payload);
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xar><toc>
<file id="1"><name>hello.txt</name><type>file</type><size>{size}</size><data><length>{length}</length><offset>0</offset><encoding style="application/x-gzip"/></data></file>
<file id="2"><name>dir</name><type>directory</type><size>0</size></file>
<file id="3"><name>link</name><type>symlink</type><link>hello.txt</link></file>
</toc></xar>"#,
        size = payload.len(),
        length = compressed_payload.len()
    );
    let compressed_xml = zlib(xml.as_bytes());
    let mut file = fs::File::create(path).unwrap();
    file.write_all(b"xar!").unwrap();
    file.write_all(&28u16.to_be_bytes()).unwrap();
    file.write_all(&1u16.to_be_bytes()).unwrap();
    file.write_all(&(compressed_xml.len() as u64).to_be_bytes())
        .unwrap();
    file.write_all(&(xml.len() as u64).to_be_bytes()).unwrap();
    file.write_all(&0u32.to_be_bytes()).unwrap();
    file.write_all(&compressed_xml).unwrap();
    file.write_all(&compressed_payload).unwrap();
}

/// 验证扩展名和魔数识别、列表、解压、符号链接跳过和完整性测试。
#[test]
fn xar_lists_extracts_and_tests() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("sample.pkg");
    make_xar(&archive, b"hello xar");
    assert_eq!(ArchiveService::detect(&archive), ArchiveFormat::Xar);
    assert_eq!(
        lumark_archive::format::detect_magic(&fs::read(&archive).unwrap()[..4]),
        ArchiveFormat::Xar
    );
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Xar));
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "dir" && entry.is_dir)
    );
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "link" && entry.method.contains("符号链接"))
    );

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
    assert_eq!(report.skipped, 1, "符号链接应被跳过");
    assert!(output.join("dir").is_dir());
    assert_eq!(
        fs::read_to_string(output.join("hello.txt")).unwrap(),
        "hello xar"
    );

    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 1);
    assert_eq!(tested.bytes, 9);
}
