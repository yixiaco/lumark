use super::common::*;
use super::*;

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

/// 多线程固实 7Z 必须按编码块完成量上报进度，而不是按提前读取的字节。
///
/// 20 MiB 源在 16 MiB 块下会产生至少两个编码块；进度应当出现中间值、
/// 单调递增，并在结束前不会提前报满。
#[test]
fn seven_zip_mt_solid_reports_completed_chunk_progress() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("large.bin"), vec![0x5au8; 20 * 1024 * 1024]).unwrap();
    let output = dir.path().join("solid-chunks.7z");
    let mut snapshots = Vec::new();
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            threads: 4,
            dictionary_size: Some(16 * 1024 * 1024),
            ..Default::default()
        },
        |progress| snapshots.push(progress),
    )
    .unwrap();
    assert!(
        snapshots
            .windows(2)
            .all(|pair| pair[0].bytes_done <= pair[1].bytes_done)
    );
    assert!(
        snapshots.iter().any(|progress| {
            progress.bytes_done > 0 && progress.bytes_done < progress.bytes_total
        })
    );
    let last = snapshots.last().unwrap();
    assert_eq!(last.bytes_done, last.bytes_total);
    assert_eq!(last.entries_done, last.entries_total);
    let restored = dir.path().join("restored");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: restored.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::metadata(restored.join("source").join("large.bin"))
            .unwrap()
            .len(),
        20 * 1024 * 1024
    );
}

/// 总量不足一个编码块时退回单线程，进度连续且归档仍完整。
#[test]
fn seven_zip_small_solid_roundtrips_with_single_thread() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("small.bin"), vec![0x77u8; 2 * 1024 * 1024]).unwrap();
    let output = dir.path().join("small-solid.7z");
    ArchiveService::create(
        &output,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: true,
            threads: 4,
            dictionary_size: Some(16 * 1024 * 1024),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let restored = dir.path().join("restored");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: restored.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::metadata(restored.join("source").join("small.bin"))
            .unwrap()
            .len(),
        2 * 1024 * 1024
    );
}

/// 非固实多线程 7Z 也应按完成编码块上报大文件内部进度。
#[test]
fn seven_zip_mt_entry_reports_completed_chunk_progress() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("large.bin");
    fs::write(&source, vec![0x33u8; 20 * 1024 * 1024]).unwrap();
    let output = dir.path().join("entry-chunks.7z");
    let mut snapshots = Vec::new();
    ArchiveService::create(
        &output,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::SevenZip,
            solid: false,
            threads: 4,
            dictionary_size: Some(16 * 1024 * 1024),
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
    assert_eq!(
        snapshots.last().unwrap().bytes_done,
        snapshots.last().unwrap().bytes_total
    );
}
