//! TAR、单文件流和 LZH 往返测试。

use super::common::*;
use super::*;

#[test]
/// 验证 TAR.GZ 的创建、列表和解压。
fn tar_gz_roundtrip() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("archive.tar.gz");
    let options = CompressionOptions {
        format: ArchiveFormat::TarGz,
        ..Default::default()
    };
    ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::TarGz));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );

    let extraction = dir.path().join("extracted");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(extraction.join("source").join("hello.txt").exists());
}

#[test]
/// 验证 TAR 各压缩变体往返。
fn tar_compression_variants_roundtrip() {
    for (format, extension) in [
        (ArchiveFormat::Tar, "tar"),
        (ArchiveFormat::TarBr, "tar.br"),
        (ArchiveFormat::TarLz4, "tar.lz4"),
        (ArchiveFormat::TarLz, "tar.lz"),
        (ArchiveFormat::TarLzma, "tar.lzma"),
        (ArchiveFormat::TarBz2, "tar.bz2"),
        (ArchiveFormat::TarXz, "tar.xz"),
        (ArchiveFormat::TarZst, "tar.zst"),
    ] {
        let dir = tempdir().unwrap();
        let source = make_source(dir.path());
        let output = dir.path().join(format!("archive.{extension}"));
        ArchiveService::create(
            &output,
            &[source],
            &CompressionOptions {
                format,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let info = ArchiveService::list(&output, None).unwrap();
        assert_eq!(info.format, Some(format));
        assert!(
            info.entries
                .iter()
                .any(|entry| entry.path.ends_with("hello.txt"))
        );
        let extraction = dir.path().join("out");
        ArchiveService::extract(
            &output,
            &ExtractionOptions {
                output_dir: extraction.clone(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let extracted = extraction.join("source").join("hello.txt");
        assert_eq!(
            fs::read_to_string(&extracted).unwrap_or_else(|error| {
                panic!("{format:?} 缺少解压结果 {}：{error}", extracted.display())
            }),
            "hello from lumark"
        );
        ArchiveService::test(&output, None, |_| {}).unwrap();
    }
}

#[test]
/// 验证单文件流格式的往返。
fn single_stream_formats_roundtrip() {
    for (format, extension) in [
        (ArchiveFormat::Gzip, "gz"),
        (ArchiveFormat::Bzip2, "bz2"),
        (ArchiveFormat::Xz, "xz"),
        (ArchiveFormat::Zstd, "zst"),
        (ArchiveFormat::Brotli, "br"),
        (ArchiveFormat::Lz4, "lz4"),
        (ArchiveFormat::Snappy, "sz"),
        (ArchiveFormat::Bz3, "bz3"),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join("payload.bin");
        fs::write(&source, b"stream payload for lumark").unwrap();
        let output = dir.path().join(format!("payload.bin.{extension}"));
        ArchiveService::create(
            &output,
            &[source],
            &CompressionOptions {
                format,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        // 单流格式在界面中通过列表打开；列表必须与创建、解压保持同一组格式，
        // 否则压缩窗口生成的 BZ2/XZ/ZST 会双击打不开。
        let info = ArchiveService::list(&output, None).unwrap();
        assert_eq!(info.format, Some(format));
        assert_eq!(info.entries.len(), 1);
        assert_eq!(info.entries[0].path, "payload.bin");
        let extraction = dir.path().join("out");
        ArchiveService::extract(
            &output,
            &ExtractionOptions {
                output_dir: extraction.clone(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(
            fs::read(extraction.join("payload.bin")).unwrap(),
            b"stream payload for lumark"
        );
    }
}

/// 验证 LZH 创建、列表、解压和完整性测试。
#[test]
fn lzh_roundtrip() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("archive.lzh");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Lzh,
            test_after: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Lzh));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );
    let extraction = dir.path().join("out");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(extraction.join("source").join("hello.txt")).unwrap(),
        "hello from lumark"
    );
    let tested = ArchiveService::test(&output, None, |_| {}).unwrap();
    assert!(tested.files > 0);
}

/// 验证 LZH 流式写入跨多个 1 MiB 分块，Store 和 lh5 都能往返。
#[test]
fn lzh_streaming_handles_multiple_chunks() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("large.bin");
    let bytes = (0..3 * 1024 * 1024 + 123)
        .map(|index| ((index * 37 + 11) % 251) as u8)
        .collect::<Vec<_>>();
    fs::write(&source, &bytes).unwrap();

    for (name, level) in [("store.lzh", Some(0)), ("lh5.lzh", None)] {
        let output = dir.path().join(name);
        ArchiveService::create(
            &output,
            std::slice::from_ref(&source),
            &CompressionOptions {
                format: ArchiveFormat::Lzh,
                level,
                test_after: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let extraction = dir.path().join(format!("{name}-out"));
        ArchiveService::extract(
            &output,
            &ExtractionOptions {
                output_dir: extraction.clone(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(
            fs::read(extraction.join("large.bin")).unwrap(),
            bytes,
            "{name} 解压内容不一致"
        );
    }
}

/// 单流格式遇到多个源或文件夹时返回可操作错误，而不是“归档损坏”。
#[test]
fn single_stream_rejects_multiple_sources_with_actionable_error() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("a.docx");
    let second = dir.path().join("b.docx");
    fs::write(&first, b"a").unwrap();
    fs::write(&second, b"b").unwrap();
    let output = dir.path().join("out.br");
    let error = ArchiveService::create(
        &output,
        &[first.clone(), second],
        &CompressionOptions {
            format: ArchiveFormat::Brotli,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, ArchiveError::InvalidRequest(_)), "{error}");
    assert!(error.to_string().contains("TAR.BR"), "{error}");
    assert!(!output.exists());

    let folder = dir.path().join("folder");
    fs::create_dir(&folder).unwrap();
    let error = ArchiveService::create(
        &output,
        &[folder],
        &CompressionOptions {
            format: ArchiveFormat::Brotli,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, ArchiveError::InvalidRequest(_)), "{error}");
    assert!(error.to_string().contains("TAR.BR"), "{error}");
}
/// 验证单文件流在复制过程中按预算中止，并清理半成品。
#[test]
fn stream_extraction_budget_stops_bomb() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("zeros.bin");
    fs::write(&source, vec![0u8; 4 * 1024 * 1024]).unwrap();
    let archive = dir.path().join("bomb.gz");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Gzip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let output_dir = dir.path().join("out");
    let result = extract_with_small_budget(&archive, &output_dir);
    assert!(
        matches!(result, Err(lumark_archive::ArchiveError::BudgetExceeded(_))),
        "{result:?}"
    );
    assert!(!output_dir.join("bomb").exists(), "超限不应留下部分文件");
}

#[test]
/// Raw/Hadoop Snappy 流应提示只支持 Framed，而不是笼统的损坏。
fn raw_snappy_reports_framed_requirement() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("raw.sz");
    fs::write(&archive, b"not a framed snappy stream").unwrap();
    let result = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: dir.path().join("out"),
            ..Default::default()
        },
        |_| {},
    );
    let error = result.expect_err("Raw Snappy 不应被当成有效流");
    assert!(
        error.to_string().contains("Framed"),
        "错误应说明只支持 Framed：{error}"
    );
}
