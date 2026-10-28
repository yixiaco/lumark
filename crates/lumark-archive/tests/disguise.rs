//! 伪装容器的跨格式集成测试。
//!
//! 覆盖所有可写格式的创建/浏览/解压/测试/还原闭环、任意文件伪装、
//! 只读格式通过物化副本浏览、损坏尾部、取消和还原冲突。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use lumark_archive::{
    ArchiveFormat, ArchiveService, CompressionOptions, DisguiseFileOptions, DisguiseMask,
    DisguiseOptions, DisguisePayloadKind, EditOptions, ExtractionOptions,
};
use tempfile::tempdir;

/// 生成一个只包含一个文本文件的源目录。
fn make_source(root: &Path) -> PathBuf {
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("hello.txt"), b"hello disguise").unwrap();
    source
}

/// 在解压目录中递归查找指定内容。
fn contains_content(directory: &Path, expected: &[u8]) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if contains_content(&path, expected) {
                return true;
            }
        } else if fs::read(&path).is_ok_and(|bytes| bytes == expected) {
            return true;
        }
    }
    false
}

/// 构造带伪装的压缩选项。
fn disguise_options(format: ArchiveFormat) -> CompressionOptions {
    CompressionOptions {
        format,
        disguise: Some(DisguiseOptions {
            mask: DisguiseMask::PresetImage,
            original_name: None,
        }),
        ..Default::default()
    }
}

/// 所有可写格式都必须能伪装、浏览、解压、测试并还原。
#[test]
fn every_writable_format_supports_disguise() {
    let directory = tempdir().unwrap();
    let source_dir = make_source(directory.path());
    // 单文件流格式只接受一个普通文件；用同一个文件覆盖所有格式。
    let source = source_dir.join("hello.txt");
    let mut failures = Vec::new();
    for (index, format) in ArchiveFormat::ALL
        .iter()
        .copied()
        .filter(|format| format.is_writable())
        .enumerate()
    {
        let output = directory.path().join(format!("cover-{index}.mp4"));
        let options = disguise_options(format);
        let created =
            ArchiveService::create(&output, std::slice::from_ref(&source), &options, |_| {});
        if let Err(error) = created {
            failures.push(format!("{format:?} 创建失败：{error}"));
            continue;
        }
        if ArchiveService::detect(&output) != ArchiveFormat::Disguised {
            failures.push(format!("{format:?} 未被识别为伪装文件"));
            continue;
        }
        if let Err(error) = ArchiveService::list(&output, None) {
            failures.push(format!("{format:?} 列表失败：{error}"));
            continue;
        }
        if let Err(error) = ArchiveService::test(&output, None, |_| {}) {
            failures.push(format!("{format:?} 测试失败：{error}"));
            continue;
        }
        let extracted = directory.path().join(format!("extract-{index}"));
        if let Err(error) = ArchiveService::extract(
            &output,
            &ExtractionOptions {
                output_dir: extracted.clone(),
                overwrite: true,
                ..Default::default()
            },
            |_| {},
        ) {
            failures.push(format!("{format:?} 解压失败：{error}"));
            continue;
        }
        if !contains_content(&extracted, b"hello disguise") {
            failures.push(format!("{format:?} 解压内容不匹配"));
            continue;
        }
        let restored_dir = directory.path().join(format!("restore-{index}"));
        match ArchiveService::restore_disguise(&output, &restored_dir, None, |_| {}) {
            Ok(report) => {
                if ArchiveService::detect(&report.output) != format {
                    failures.push(format!(
                        "{format:?} 还原到 {} 后识别为 {:?}",
                        report.output.display(),
                        ArchiveService::detect(&report.output)
                    ));
                } else if let Err(error) = ArchiveService::list(&report.output, None) {
                    failures.push(format!("{format:?} 还原后无法打开：{error}"));
                }
            }
            Err(error) => failures.push(format!("{format:?} 还原失败：{error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 密码保护的 ZIP 与头加密的 7Z 伪装后仍能校验密码并解压。
#[test]
fn encrypted_disguise_round_trip() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    for format in [ArchiveFormat::Zip, ArchiveFormat::SevenZip] {
        let output = directory.path().join(format!("encrypted-{format:?}.jpg"));
        let options = CompressionOptions {
            format,
            password: Some(zeroize::Zeroizing::new("secret-pass".to_string())),
            disguise: Some(DisguiseOptions {
                mask: DisguiseMask::PresetImage,
                original_name: None,
            }),
            ..Default::default()
        };
        ArchiveService::create(&output, std::slice::from_ref(&source), &options, |_| {}).unwrap();
        let encryption = ArchiveService::probe_encryption(&output).unwrap();
        assert!(encryption.encrypted, "{format:?}");
        assert!(
            ArchiveService::verify_password(&output, "secret-pass").unwrap(),
            "{format:?}"
        );
        assert!(
            !ArchiveService::verify_password(&output, "wrong-pass").unwrap(),
            "{format:?}"
        );
        let extracted = directory.path().join(format!("encrypted-out-{format:?}"));
        ArchiveService::extract(
            &output,
            &ExtractionOptions {
                output_dir: extracted.clone(),
                overwrite: true,
                password: Some(zeroize::Zeroizing::new("secret-pass".to_string())),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert!(
            contains_content(&extracted, b"hello disguise"),
            "{format:?}"
        );
    }
}

/// 任意文件伪装：原始载荷只能还原，不能浏览。
#[test]
fn raw_file_disguise_restores_original() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("report.dat");
    fs::write(&source, vec![0x5a; 4096]).unwrap();
    let output_dir = directory.path().join("out");
    let report = ArchiveService::disguise_files(
        std::slice::from_ref(&source),
        &DisguiseFileOptions {
            mask: DisguiseMask::PresetImage,
            output_dir: output_dir.clone(),
            delete_source: false,
            cancel: None,
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.files, 1);
    let disguised = &report.outputs[0];
    assert_eq!(
        ArchiveService::detect(disguised),
        ArchiveFormat::Unknown,
        "原始载荷不应被当作可浏览归档"
    );
    let info = ArchiveService::probe_disguise(disguised)
        .unwrap()
        .expect("伪装信息");
    assert_eq!(info.payload_kind, DisguisePayloadKind::RawFile);
    let restored = ArchiveService::restore_disguise(disguised, &output_dir, None, |_| {}).unwrap();
    assert_eq!(restored.output.file_name().unwrap(), "report.dat");
    assert_eq!(fs::read(&restored.output).unwrap(), vec![0x5a; 4096]);
}

/// 已有归档伪装后仍然可以直接浏览；还原得到原归档。
#[test]
fn existing_archive_disguise_is_browsable() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let archive = directory.path().join("plain.zip");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &CompressionOptions::default(),
        |_| {},
    )
    .unwrap();
    let output_dir = directory.path().join("out");
    let report = ArchiveService::disguise_files(
        std::slice::from_ref(&archive),
        &DisguiseFileOptions {
            mask: DisguiseMask::PresetVideo,
            output_dir: output_dir.clone(),
            delete_source: true,
            cancel: None,
        },
        |_| {},
    )
    .unwrap();
    let disguised = &report.outputs[0];
    assert_eq!(ArchiveService::detect(disguised), ArchiveFormat::Disguised);
    let info = ArchiveService::list(disguised, None).unwrap();
    assert!(!info.entries.is_empty());
    assert_eq!(info.format, Some(ArchiveFormat::Disguised));
    let restored = ArchiveService::restore_disguise(disguised, &output_dir, None, |_| {}).unwrap();
    assert_eq!(
        restored.output.file_name().unwrap(),
        "plain.zip",
        "还原路径：{}",
        restored.output.display()
    );
    assert!(!archive.exists(), "delete_source 应删除源归档");
}

/// 只读传统格式伪装后通过物化副本打开。
#[test]
fn legacy_payload_opens_through_materialized_copy() {
    let directory = tempdir().unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy/zoo/default.zoo");
    let source = directory.path().join("legacy.zoo");
    fs::copy(&fixture, &source).unwrap();
    let output_dir = directory.path().join("out");
    let report = ArchiveService::disguise_files(
        std::slice::from_ref(&source),
        &DisguiseFileOptions {
            mask: DisguiseMask::PresetImage,
            output_dir,
            delete_source: false,
            cancel: None,
        },
        |_| {},
    )
    .unwrap();
    let disguised = &report.outputs[0];
    assert_eq!(ArchiveService::detect(disguised), ArchiveFormat::Disguised);
    let info = ArchiveService::list(disguised, None).unwrap();
    assert!(!info.entries.is_empty(), "传统格式物化后应能列出条目");
}

/// 伪装与非 ZIP 自解压、分卷和只读格式互斥。
#[test]
fn unsupported_disguise_combinations_are_rejected() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let output = directory.path().join("cover.jpg");
    let mut with_split = disguise_options(ArchiveFormat::Zip);
    with_split.split_size = Some(100 * 1024 * 1024);
    assert!(
        ArchiveService::create(&output, std::slice::from_ref(&source), &with_split, |_| {})
            .is_err()
    );
    let mut with_sfx = disguise_options(ArchiveFormat::Zip);
    with_sfx.sfx = true;
    assert!(
        ArchiveService::create(&output, std::slice::from_ref(&source), &with_sfx, |_| {}).is_err()
    );
    let read_only = disguise_options(ArchiveFormat::Rar);
    assert!(ArchiveService::create(&output, &[source], &read_only, |_| {}).is_err());
    assert!(!output.exists());
}

/// 取消后不能留下半成品或破坏旧输出。
#[test]
fn cancel_leaves_no_partial_output() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let output = directory.path().join("cover.jpg");
    fs::write(&output, b"old-output").unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let options = CompressionOptions {
        cancel: Some(cancel),
        ..disguise_options(ArchiveFormat::Zip)
    };
    let error = ArchiveService::create(&output, &[source], &options, |_| {}).unwrap_err();
    assert!(matches!(error, lumark_archive::ArchiveError::Cancelled));
    assert_eq!(fs::read(&output).unwrap(), b"old-output");
    let leftovers: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".lumark-disguise")
        })
        .collect();
    assert!(leftovers.is_empty(), "取消后不应留下临时文件");
}

/// 尾部损坏时探测返回错误，打开流程给出明确失败。
#[test]
fn corrupted_trailer_is_reported() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let output = directory.path().join("cover.jpg");
    ArchiveService::create(
        &output,
        std::slice::from_ref(&source),
        &disguise_options(ArchiveFormat::Zip),
        |_| {},
    )
    .unwrap();
    let len = fs::metadata(&output).unwrap().len();
    let mut file = fs::OpenOptions::new().write(true).open(&output).unwrap();
    use std::io::{Seek, SeekFrom, Write};
    file.seek(SeekFrom::Start(len - 512 + 8)).unwrap();
    file.write_all(&[9]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let error = ArchiveService::probe_disguise(&output).unwrap_err();
    assert!(matches!(error, lumark_archive::ArchiveError::Corrupt(_)));
    assert_eq!(ArchiveService::detect(&output), ArchiveFormat::Disguised);
    assert!(ArchiveService::list(&output, None).is_err());
}

/// 还原遇到同名文件时追加序号，不覆盖已有文件。
#[test]
fn restore_avoids_overwriting_existing_file() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("payload.bin");
    fs::write(&source, b"payload").unwrap();
    let output_dir = directory.path().join("out");
    let report = ArchiveService::disguise_files(
        std::slice::from_ref(&source),
        &DisguiseFileOptions {
            mask: DisguiseMask::PresetImage,
            output_dir: output_dir.clone(),
            delete_source: false,
            cancel: None,
        },
        |_| {},
    )
    .unwrap();
    fs::create_dir_all(&output_dir).unwrap();
    fs::write(output_dir.join("payload.bin"), b"existing").unwrap();
    let restored =
        ArchiveService::restore_disguise(&report.outputs[0], &output_dir, None, |_| {}).unwrap();
    assert_eq!(restored.output.file_name().unwrap(), "payload (1).bin");
    assert_eq!(fs::read(&restored.output).unwrap(), b"payload");
}

/// 自定义面具必须原样出现在输出开头。
#[test]
fn custom_mask_is_prefixed_verbatim() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let mask_path = directory.path().join("cover.jpg");
    let mask_bytes: Vec<u8> = (0..8192).map(|index| (index % 251) as u8).collect();
    fs::write(&mask_path, &mask_bytes).unwrap();
    let output = directory.path().join("custom.mp4");
    let options = CompressionOptions {
        disguise: Some(DisguiseOptions {
            mask: DisguiseMask::File(mask_path),
            original_name: None,
        }),
        ..Default::default()
    };
    ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();
    let raw = fs::read(&output).unwrap();
    assert!(raw.starts_with(&mask_bytes));
}

/// 内置图片面具必须原样出现在输出开头。
#[test]
fn preset_mask_is_prefixed() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let output = directory.path().join("preset.jpg");
    ArchiveService::create(
        &output,
        std::slice::from_ref(&source),
        &disguise_options(ArchiveFormat::Zip),
        |_| {},
    )
    .unwrap();
    let raw = fs::read(&output).unwrap();
    assert!(raw.starts_with(include_bytes!("../assets/disguise/cover.jpg")));
}

/// 压缩后测试与删除源文件在伪装创建中同样生效。
#[test]
fn disguise_with_test_after_and_delete_source() {
    let directory = tempdir().unwrap();
    let source = make_source(directory.path());
    let file = source.join("hello.txt");
    let output = directory.path().join("checked.jpg");
    let options = CompressionOptions {
        test_after: true,
        delete_source: true,
        ..disguise_options(ArchiveFormat::Zip)
    };
    ArchiveService::create(&output, std::slice::from_ref(&file), &options, |_| {}).unwrap();
    assert_eq!(ArchiveService::detect(&output), ArchiveFormat::Disguised);
    assert!(!file.exists(), "成功后应删除源文件");
    assert!(
        !ArchiveService::list(&output, None)
            .unwrap()
            .entries
            .is_empty()
    );
}

/// 批量伪装多个文件时逐个生成输出，并支持删除源文件。
#[test]
fn batch_file_disguise_removes_sources() {
    let directory = tempdir().unwrap();
    let first = directory.path().join("one.txt");
    let second = directory.path().join("two.txt");
    fs::write(&first, b"one").unwrap();
    fs::write(&second, b"two").unwrap();
    let output_dir = directory.path().join("out");
    let report = ArchiveService::disguise_files(
        &[first.clone(), second.clone()],
        &DisguiseFileOptions {
            mask: DisguiseMask::PresetVideo,
            output_dir: output_dir.clone(),
            delete_source: true,
            cancel: None,
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(report.files, 2);
    assert_eq!(report.outputs.len(), 2);
    assert!(!first.exists(), "第一个源文件应已删除");
    assert!(!second.exists(), "第二个源文件应已删除");
    for output in &report.outputs {
        assert!(ArchiveService::probe_disguise(output).unwrap().is_some());
    }
    let restored =
        ArchiveService::restore_disguise(&report.outputs[0], &output_dir, None, |_| {}).unwrap();
    assert_eq!(fs::read(&restored.output).unwrap(), b"one");
}

/// 伪装 ZIP/7Z 支持原地重命名、删除和添加，并保留原面具和原文件名。
#[test]
fn disguised_archive_can_be_edited_in_place() {
    for format in [ArchiveFormat::Zip, ArchiveFormat::SevenZip] {
        let directory = tempdir().unwrap();
        let source = make_source(directory.path());
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested").join("remove.bin"), b"remove").unwrap();
        let output = directory.path().join("editable.jpg");
        let options = CompressionOptions {
            disguise: Some(DisguiseOptions {
                mask: DisguiseMask::PresetImage,
                original_name: Some(format!("editable.{}", format.extension())),
            }),
            ..disguise_options(format)
        };
        ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();
        let before = ArchiveService::probe_disguise(&output)
            .unwrap()
            .expect("伪装尾部");
        let added = directory.path().join("added.txt");
        fs::write(&added, b"added").unwrap();
        let report = ArchiveService::edit(
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
        assert_eq!(report.output, output);
        assert_eq!(ArchiveService::detect(&output), ArchiveFormat::Disguised);
        let after = ArchiveService::probe_disguise(&output)
            .unwrap()
            .expect("编辑后的伪装尾部");
        assert_eq!(after.original_name, before.original_name);
        assert_eq!(after.mask_len, before.mask_len);
        let info = ArchiveService::list(&output, None).unwrap();
        assert!(info.entries.iter().any(|entry| entry.path == "renamed.txt"));
        assert!(info.entries.iter().any(|entry| entry.path == "added.txt"));
        assert!(
            !info
                .entries
                .iter()
                .any(|entry| entry.path.contains("nested"))
        );
        let extraction = directory.path().join("out");
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
            fs::read_to_string(extraction.join("renamed.txt")).unwrap(),
            "hello disguise"
        );
        assert_eq!(
            fs::read_to_string(extraction.join("added.txt")).unwrap(),
            "added"
        );
    }
}

/// LZH 多条目伪装：每个条目头都在载荷中回填，验证跨条目 seek/patch 正确。
#[test]
fn lzh_disguise_round_trip_handles_multiple_entries() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("lzh-source");
    fs::create_dir_all(&source).unwrap();
    let large = vec![0x5Au8; 2 * 1024 * 1024];
    fs::write(source.join("large.bin"), &large).unwrap();
    fs::write(source.join("note.txt"), b"lzh disguise note").unwrap();

    let output = directory.path().join("lzh-cover.jpg");
    let options = disguise_options(ArchiveFormat::Lzh);
    ArchiveService::create(&output, std::slice::from_ref(&source), &options, |_| {}).unwrap();
    assert_eq!(ArchiveService::detect(&output), ArchiveFormat::Disguised);

    let info = ArchiveService::list(&output, None).unwrap();
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("large.bin"))
    );
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("note.txt"))
    );

    let extracted = directory.path().join("lzh-extract");
    ArchiveService::extract(
        &output,
        &ExtractionOptions {
            output_dir: extracted.clone(),
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(extracted.join("lzh-source/large.bin")).unwrap(),
        large
    );
    assert_eq!(
        fs::read(extracted.join("lzh-source/note.txt")).unwrap(),
        b"lzh disguise note"
    );
    ArchiveService::test(&output, None, |_| {}).unwrap();
}
