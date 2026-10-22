use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use lumark_archive::{
    ArchiveFormat, ArchiveService, CompressionOptions, EditOptions, ExtractionOptions,
    OverwritePolicy, UpdateMode, safe_join,
};
use tempfile::tempdir;

fn make_source(root: &std::path::Path) -> PathBuf {
    let source = root.join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("hello.txt"), b"hello from lumark").unwrap();
    fs::write(source.join("nested").join("data.bin"), [0u8, 1, 2, 3, 4]).unwrap();
    source
}

#[test]
fn zip_roundtrip_with_password() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("archive.zip");
    let options = CompressionOptions {
        format: ArchiveFormat::Zip,
        password: Some(zeroize::Zeroizing::new("zip-pass-123".to_string())),
        ..Default::default()
    };
    ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();

    let info = ArchiveService::list(&output, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Zip));
    assert!(info.encrypted);
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );
    assert!(ArchiveService::verify_password(&output, "zip-pass-123").unwrap());
    assert!(!ArchiveService::verify_password(&output, "wrong").unwrap());

    let extraction = dir.path().join("extracted");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            password: Some(zeroize::Zeroizing::new("zip-pass-123".to_string())),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(extraction.join("source").join("hello.txt")).unwrap(),
        b"hello from lumark"
    );
}

#[test]
fn zipcrypto_compatibility_writing_roundtrip() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("legacy.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(zeroize::Zeroizing::new("legacy-pass".to_string())),
            zip_encryption: lumark_archive::ZipEncryption::ZipCrypto,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.encrypted);
    assert!(ArchiveService::verify_password(&output, "legacy-pass").unwrap());
    let output_dir = dir.path().join("legacy-out");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: output_dir.clone(),
            overwrite: true,
            password: Some(zeroize::Zeroizing::new("legacy-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output_dir.join("source").join("hello.txt")).unwrap(),
        b"hello from lumark"
    );
}

#[test]
fn seven_zip_roundtrip_with_password() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("archive.7z");
    let options = CompressionOptions {
        format: ArchiveFormat::SevenZip,
        password: Some(zeroize::Zeroizing::new("seven-pass-123".to_string())),
        ..Default::default()
    };
    ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();

    let info = ArchiveService::list(&output, Some("seven-pass-123")).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::SevenZip));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );
    assert!(ArchiveService::verify_password(&output, "seven-pass-123").unwrap());
    assert!(!ArchiveService::verify_password(&output, "wrong").unwrap());

    let extraction = dir.path().join("extracted");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            password: Some(zeroize::Zeroizing::new("seven-pass-123".to_string())),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(extraction.join("source").join("hello.txt")).unwrap(),
        b"hello from lumark"
    );
}

#[test]
fn seven_zip_detects_entry_encryption_without_encrypted_header() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("entry-only.7z");
    let options = CompressionOptions {
        format: ArchiveFormat::SevenZip,
        password: Some(zeroize::Zeroizing::new("entry-pass-123".to_string())),
        encrypt_header: false,
        ..Default::default()
    };
    ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();

    let probe = ArchiveService::probe_encryption(&output).unwrap();
    assert!(probe.encrypted);
    assert!(!probe.header_encrypted);
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.encrypted);
    assert!(!info.header_encrypted);
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );
    assert!(ArchiveService::verify_password(&output, "entry-pass-123").unwrap());
    assert!(!ArchiveService::verify_password(&output, "wrong").unwrap());
}

#[test]
fn seven_zip_solid_roundtrip() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("solid.7z");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            level: Some(9),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.solid);
    let extraction = dir.path().join("solid-out");
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
        fs::read(extraction.join("source").join("hello.txt")).unwrap(),
        b"hello from lumark"
    );
}

#[test]
fn seven_zip_solid_blocks_split_by_size_and_keep_data() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("blocks-source");
    fs::create_dir_all(&source).unwrap();
    // 六个 300 KiB 文件配 1 MiB 块上限，每块三个文件：既验证切块，也验证块内仍是固实压缩。
    let payloads = (0..6u8)
        .map(|index| vec![index + 1; 300 * 1024])
        .collect::<Vec<_>>();
    for (index, payload) in payloads.iter().enumerate() {
        fs::write(source.join(format!("part-{index}.bin")), payload).unwrap();
    }
    let output = dir.path().join("blocks.7z");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            level: Some(1),
            solid_block_size: Some(1024 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.solid);
    assert_eq!(info.entries.len(), 6);
    let extraction = dir.path().join("blocks-out");
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
    for (index, payload) in payloads.iter().enumerate() {
        let extracted = fs::read(
            extraction
                .join("blocks-source")
                .join(format!("part-{index}.bin")),
        )
        .unwrap();
        assert!(extracted == *payload, "第 {index} 个固实块内容不一致");
    }
}

#[test]
fn seven_zip_solid_selective_extraction_and_skip_keep_streams_aligned() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("solid-source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("first.txt"),
        b"first payload that must be skipped",
    )
    .unwrap();
    fs::write(
        source.join("middle.txt"),
        b"middle payload must remain correct",
    )
    .unwrap();
    fs::write(source.join("last.txt"), b"last payload must remain correct").unwrap();
    let archive = dir.path().join("solid-selection.7z");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    let selected_output = dir.path().join("selected");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: selected_output.clone(),
            selected_paths: vec!["solid-source/middle.txt".to_string()],
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(selected_output.join("solid-source").join("middle.txt")).unwrap(),
        b"middle payload must remain correct"
    );
    assert!(
        !selected_output
            .join("solid-source")
            .join("first.txt")
            .exists()
    );

    let skipped_output = dir.path().join("skipped");
    fs::create_dir_all(skipped_output.join("solid-source")).unwrap();
    fs::write(
        skipped_output.join("solid-source").join("first.txt"),
        b"existing",
    )
    .unwrap();
    let report = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: skipped_output.clone(),
            overwrite_policy: OverwritePolicy::Skip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 1);
    assert_eq!(
        fs::read(skipped_output.join("solid-source").join("last.txt")).unwrap(),
        b"last payload must remain correct"
    );
}

#[test]
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
fn tar_compression_variants_roundtrip() {
    for (format, extension) in [
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
    }
}

#[test]
fn single_stream_formats_roundtrip() {
    for (format, extension) in [
        (ArchiveFormat::Gzip, "gz"),
        (ArchiveFormat::Bzip2, "bz2"),
        (ArchiveFormat::Xz, "xz"),
        (ArchiveFormat::Zstd, "zst"),
        (ArchiveFormat::Brotli, "br"),
        (ArchiveFormat::Lz4, "lz4"),
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

#[test]
fn unsafe_paths_are_rejected() {
    assert!(safe_join(std::path::Path::new("C:\\out"), "../escape").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "C:\\absolute").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "folder\\..\\escape").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "CON").is_err());
    assert!(safe_join(std::path::Path::new("C:\\out"), "folder\\file.txt").is_ok());
}

#[test]
fn zip_edit_is_atomic_and_supports_delete_rename_add() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("edit.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let added = dir.path().join("added.txt");
    fs::write(&added, b"added").unwrap();
    let original = fs::read(&output).unwrap();
    ArchiveService::edit(
        &output,
        &EditOptions {
            delete_paths: vec!["source/nested".to_string()],
            rename: vec![("source/hello.txt".to_string(), "renamed.txt".to_string())],
            additions: vec![("added.txt".to_string(), added)],
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "renamed.txt"));
    assert!(info.entries.iter().any(|entry| entry.path == "added.txt"));
    assert!(
        !info
            .entries
            .iter()
            .any(|entry| entry.path.contains("nested"))
    );
    assert_ne!(fs::read(&output).unwrap(), original);
}

#[test]
fn seven_zip_edit_supports_delete_rename_add() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("edit.7z");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let added = dir.path().join("added.txt");
    fs::write(&added, b"added-seven").unwrap();
    ArchiveService::edit(
        &output,
        &EditOptions {
            delete_paths: vec!["source/nested".to_string()],
            rename: vec![("source/hello.txt".to_string(), "renamed.txt".to_string())],
            additions: vec![("added.txt".to_string(), added)],
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&output, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "renamed.txt"));
    assert!(info.entries.iter().any(|entry| entry.path == "added.txt"));
    assert!(
        !info
            .entries
            .iter()
            .any(|entry| entry.path.contains("nested"))
    );
}

#[test]
fn seven_zip_solid_edit_keeps_remaining_entry_contents() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("solid-edit-source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("first.txt"), b"delete this entry").unwrap();
    fs::write(source.join("keep.txt"), b"keep this payload intact").unwrap();
    let archive = dir.path().join("solid-edit.7z");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    ArchiveService::edit(
        &archive,
        &EditOptions {
            delete_paths: vec!["solid-edit-source/first.txt".to_string()],
            rename: vec![(
                "solid-edit-source/keep.txt".to_string(),
                "kept.txt".to_string(),
            )],
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(
        !info
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("first.txt"))
    );
    assert!(info.entries.iter().any(|entry| entry.path == "kept.txt"));
    let output = dir.path().join("solid-edit-out");
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
        fs::read(output.join("kept.txt")).unwrap(),
        b"keep this payload intact"
    );
}

#[test]
fn adding_an_existing_name_replaces_without_duplicate_entries() {
    for (format, extension) in [(ArchiveFormat::Zip, "zip"), (ArchiveFormat::SevenZip, "7z")] {
        let dir = tempdir().unwrap();
        let source = make_source(dir.path());
        let archive = dir.path().join(format!("replace.{extension}"));
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
        let replacement = dir.path().join("replacement.txt");
        fs::write(&replacement, b"replacement payload").unwrap();
        ArchiveService::edit(
            &archive,
            &EditOptions {
                additions: vec![("source/hello.txt".to_string(), replacement)],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let info = ArchiveService::list(&archive, None).unwrap();
        assert_eq!(
            info.entries
                .iter()
                .filter(|entry| entry.path == "source/hello.txt")
                .count(),
            1
        );
        let output = dir.path().join("replacement-out");
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
            fs::read(output.join("source").join("hello.txt")).unwrap(),
            b"replacement payload"
        );
    }
}

#[test]
fn failed_edit_leaves_original_archive_untouched() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("safe.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let original = fs::read(&output).unwrap();
    let result = ArchiveService::edit(
        &output,
        &EditOptions {
            additions: vec![(
                "missing.txt".to_string(),
                dir.path().join("does-not-exist.txt"),
            )],
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&output).unwrap(), original);
}

#[test]
fn cancelled_edit_leaves_original_archive_untouched() {
    let dir = tempdir().unwrap();
    let source = make_source(dir.path());
    let output = dir.path().join("cancel-edit.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let original = fs::read(&output).unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let result = ArchiveService::edit(
        &output,
        &EditOptions {
            rename: vec![("source/hello.txt".to_string(), "renamed.txt".to_string())],
            cancel: Some(cancel),
            ..Default::default()
        },
        |_| {},
    );
    assert!(matches!(
        result,
        Err(lumark_archive::ArchiveError::Cancelled)
    ));
    assert_eq!(fs::read(&output).unwrap(), original);
}

#[test]
fn split_volume_archive_can_be_listed_and_extracted() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("large.bin");
    let mut state = 0x9E37_79B9u32;
    let bytes: Vec<u8> = (0..220 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&source, bytes).unwrap();
    let output = dir.path().join("volumes.zip");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(!output.exists());
    let first = dir.path().join("volumes.zip.001");
    assert!(first.exists());
    let info = ArchiveService::list(&first, None).unwrap();
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("large.bin"))
    );
    let extraction = dir.path().join("split-out");
    ArchiveService::extract(
        &first,
        &ExtractionOptions {
            output_dir: extraction.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(extraction.join("large.bin")).unwrap().len(),
        220 * 1024
    );
}

#[test]
fn replacing_a_split_archive_removes_stale_volumes() {
    let dir = tempdir().unwrap();
    let large = dir.path().join("large.bin");
    let mut state = 0xA5A5_5A5Au32;
    let large_bytes: Vec<u8> = (0..420 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&large, large_bytes).unwrap();
    let output = dir.path().join("volumes.zip");
    ArchiveService::create(
        &output,
        &[large],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(dir.path().join("volumes.zip.003").exists());

    let small = dir.path().join("small.bin");
    let mut state = 0x1234_5678u32;
    let small_bytes: Vec<u8> = (0..80 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&small, small_bytes).unwrap();
    ArchiveService::create(
        &output,
        &[small],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(dir.path().join("volumes.zip.001").exists());
    assert!(dir.path().join("volumes.zip.002").exists());
    assert!(!dir.path().join("volumes.zip.003").exists());
}

#[test]
fn compression_can_test_before_publish_and_delete_sources() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("delete-me.txt");
    fs::write(&source, b"test and delete").unwrap();
    let archive = dir.path().join("verified.zip");
    let report = ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(zeroize::Zeroizing::new("verify-pass".to_string())),
            test_after: true,
            delete_source: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(report.cleanup_errors.is_empty());
    assert!(archive.exists());
    assert!(!source.exists());
    assert!(ArchiveService::verify_password(&archive, "verify-pass").unwrap());
}

#[test]
fn delete_source_is_rejected_when_output_is_inside_source() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload.txt"), b"payload").unwrap();
    let output = source.join("nested.zip");
    let result = ArchiveService::create(
        &output,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            delete_source: true,
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
    assert!(source.join("payload.txt").exists());
    assert!(!output.exists());
}

#[test]
fn failed_split_creation_preserves_existing_volumes() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("payload.bin");
    fs::write(&source, vec![0x33u8; 180 * 1024]).unwrap();
    let output = dir.path().join("volumes.zip");
    fs::write(dir.path().join("volumes.zip.001"), b"existing volume").unwrap();
    fs::create_dir(dir.path().join("volumes.zip.002")).unwrap();

    let result = ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            split_size: Some(64 * 1024),
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read(dir.path().join("volumes.zip.001")).unwrap(),
        b"existing volume"
    );
    assert!(dir.path().join("volumes.zip.002").is_dir());
    assert!(
        fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".lumark-split"))
    );
}

#[test]
fn rar_sample_can_be_listed_when_provided() {
    let Ok(sample) = std::env::var("LUMARK_RAR_SAMPLE") else {
        return;
    };
    let path = PathBuf::from(sample);
    if !path.exists() {
        return;
    }
    let info = ArchiveService::list(&path, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Rar));
    assert!(!info.entries.is_empty());
    if !info.encrypted {
        let output = tempdir().unwrap();
        let report = ArchiveService::extract(
            &path,
            &ExtractionOptions {
                output_dir: output.path().to_path_buf(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert!(report.files > 0);
    }
}

#[test]
fn corrupted_rar_is_not_reported_as_encrypted() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("broken.rar");
    fs::write(&archive, b"this is not a rar archive").unwrap();
    assert!(
        ArchiveService::probe_encryption(&archive).is_err(),
        "损坏归档必须返回解析错误，而不是加密状态"
    );
}

#[test]
fn malformed_unix_compress_is_reported_as_corrupt() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("broken.Z");
    fs::write(&archive, b"\x1f\x9d\x90broken unix compress").unwrap();
    let result = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: dir.path().join("out"),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    );
    assert!(result.is_err());
}

#[test]
fn aes_encrypted_container_lists_verifies_and_extracts() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("secret.txt.enc");
    let mut input = std::io::Cursor::new(b"encrypted container payload");
    let mut encrypted = Vec::new();
    geezipx_core::archive::aes::aes_encrypt(&mut input, &mut encrypted, "container-pass").unwrap();
    fs::write(&archive, encrypted).unwrap();

    let info = ArchiveService::list(&archive, Some("container-pass")).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Enc));
    assert!(info.encrypted);
    assert!(info.entries[0].size > 0);
    assert!(ArchiveService::verify_password(&archive, "container-pass").unwrap());
    assert!(!ArchiveService::verify_password(&archive, "wrong-pass").unwrap());

    let output = dir.path().join("enc-out");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            password: Some(zeroize::Zeroizing::new("container-pass".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("secret.txt")).unwrap(),
        b"encrypted container payload"
    );
}

#[test]
fn extraction_overwrite_policies_skip_or_rename() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("payload.txt");
    fs::write(&source, b"payload").unwrap();
    let archive = dir.path().join("policy.zip");
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
    let output = dir.path().join("out");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("payload.txt"), b"keep").unwrap();
    let skipped = ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite_policy: OverwritePolicy::Skip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(skipped.skipped, 1);
    assert_eq!(fs::read(output.join("payload.txt")).unwrap(), b"keep");
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output.clone(),
            overwrite_policy: OverwritePolicy::Rename,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("payload (1).txt")).unwrap(),
        b"payload"
    );
}

#[test]
fn cpio_legacy_reader_lists_and_extracts() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("sample.cpio");
    let mut bytes = Vec::new();
    append_newc_entry(&mut bytes, "hello.txt", b"legacy cpio");
    append_newc_entry(&mut bytes, "TRAILER!!!", b"");
    fs::write(&archive, bytes).unwrap();
    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Cpio));
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));
    let output = dir.path().join("cpio-out");
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
    assert_eq!(fs::read(output.join("hello.txt")).unwrap(), b"legacy cpio");
}

fn append_newc_entry(output: &mut Vec<u8>, name: &str, data: &[u8]) {
    let name_len = name.len() + 1;
    let header = format!(
        "070701{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
        1,
        0o100644,
        0,
        0,
        1,
        0,
        data.len(),
        0,
        0,
        0,
        0,
        name_len,
        0
    );
    output.extend_from_slice(header.as_bytes());
    output.extend_from_slice(name.as_bytes());
    output.push(0);
    while !output.len().is_multiple_of(4) {
        output.push(0);
    }
    output.extend_from_slice(data);
    while !output.len().is_multiple_of(4) {
        output.push(0);
    }
}

/// 验证固实 7Z 在统一压缩阶段仍会按实际读取字节报告当前文件和总进度。
#[test]
fn seven_zip_solid_reports_byte_progress_during_compression() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("large.bin"), vec![0x5au8; 512 * 1024]).unwrap();
    fs::write(source.join("small.bin"), vec![0xa5u8; 1024]).unwrap();
    let output = dir.path().join("solid-progress.7z");
    let mut snapshots = Vec::new();
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            ..Default::default()
        },
        |progress| snapshots.push(progress),
    )
    .unwrap();
    assert!(
        snapshots.iter().any(|progress| {
            progress.bytes_done > 0 && progress.bytes_done < progress.bytes_total
        })
    );
    assert!(snapshots.iter().any(|progress| {
        progress.current_bytes_done > 0
            && progress.current_bytes_done < progress.current_bytes_total
    }));
}

/// 设置文件修改时间，供更新模式的时间比较使用。
fn set_modified(path: &std::path::Path, when: std::time::SystemTime) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(when).unwrap();
}

/// 解压归档并返回指定条目的文本内容。
fn extract_text(archive: &std::path::Path, output: &std::path::Path, name: &str) -> String {
    ArchiveService::extract(
        archive,
        &ExtractionOptions {
            output_dir: output.to_path_buf(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    String::from_utf8(fs::read(output.join(name)).unwrap()).unwrap()
}

/// 验证“只更新较新文件”会保留较旧的归档条目，并替换更新的文件。
#[test]
fn zip_edit_update_mode_replaces_only_newer_files() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("hello.txt");
    fs::write(&source, b"original").unwrap();
    let base = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    set_modified(&source, base);
    let archive = dir.path().join("update.zip");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    // 较旧的文件必须被跳过，归档内容保持原样。
    fs::write(&source, b"stale").unwrap();
    set_modified(&source, base - std::time::Duration::from_secs(3600));
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            additions: vec![("hello.txt".to_string(), source.clone())],
            update_mode: UpdateMode::Newer,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 1);
    assert_eq!(
        extract_text(&archive, &dir.path().join("stale"), "hello.txt"),
        "original"
    );

    // 更新的文件应替换条目内容。
    fs::write(&source, b"fresh").unwrap();
    set_modified(&source, base + std::time::Duration::from_secs(3600));
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            additions: vec![("hello.txt".to_string(), source)],
            update_mode: UpdateMode::Newer,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 0);
    assert_eq!(
        extract_text(&archive, &dir.path().join("fresh"), "hello.txt"),
        "fresh"
    );
}

/// 验证“仅刷新已有条目”更新旧条目但不会添加新文件。
#[test]
fn zip_edit_freshen_mode_skips_new_files() {
    let dir = tempdir().unwrap();
    let existing = dir.path().join("hello.txt");
    fs::write(&existing, b"original").unwrap();
    let archive = dir.path().join("freshen.zip");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&existing),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    fs::write(&existing, b"updated").unwrap();
    let added = dir.path().join("new.txt");
    fs::write(&added, b"new file").unwrap();
    let report = ArchiveService::edit(
        &archive,
        &EditOptions {
            additions: vec![
                ("hello.txt".to_string(), existing),
                ("new.txt".to_string(), added),
            ],
            update_mode: UpdateMode::Freshen,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.skipped, 1);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(
        !info.entries.iter().any(|entry| entry.path == "new.txt"),
        "仅刷新模式不应加入新条目"
    );
    assert_eq!(
        extract_text(&archive, &dir.path().join("updated"), "hello.txt"),
        "updated"
    );
}
