//! 自解压库解压进度与取消能力的集成测试。
//!
//! 覆盖 ZIP/7Z 进度总量的单调递增、预取消、中途取消、跳过已有文件时的
//! 进度推进，以及加密 7Z 的密码错误与取消不会被互相误报。

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use lumark_sfx::{
    SfxError, SfxExtractOptions, SfxFooter, SfxFormat, SfxProgress, extract_payload_with_options,
    read_footer,
};

/// 组装 v2 页脚；`header_encrypted` 对应标志字节第 2 位。
fn footer_bytes(
    offset: u64,
    length: u64,
    format: SfxFormat,
    encrypted: bool,
    header_encrypted: bool,
) -> Vec<u8> {
    let mut footer = Vec::new();
    footer.extend_from_slice(b"LMSFX002");
    footer.push(match format {
        SfxFormat::Zip => 0,
        SfxFormat::SevenZip => 1,
    });
    footer.push(u8::from(encrypted) | (u8::from(header_encrypted) << 1));
    footer.extend_from_slice(&[0u8; 6]);
    footer.extend_from_slice(&offset.to_le_bytes());
    footer.extend_from_slice(&length.to_le_bytes());
    footer
}

/// 把运行时模块占位字节、载荷和页脚拼接成自解压文件。
fn write_sfx(
    directory: &Path,
    payload: &[u8],
    format: SfxFormat,
    encrypted: bool,
    header_encrypted: bool,
) -> PathBuf {
    let stub = b"MZlumark-sfx-progress-test";
    let path = directory.join("sample.exe");
    let mut file = File::create(&path).expect("创建自解压文件");
    file.write_all(stub).expect("写入运行时模块");
    file.write_all(payload).expect("写入载荷");
    file.write_all(&footer_bytes(
        stub.len() as u64,
        payload.len() as u64,
        format,
        encrypted,
        header_encrypted,
    ))
    .expect("写入页脚");
    path
}

/// 打开自解压文件并解析页脚。
fn open_sfx(path: &Path) -> (File, SfxFooter) {
    let mut file = File::open(path).expect("打开自解压文件");
    let footer = read_footer(&mut file).expect("读取页脚").expect("页脚存在");
    (file, footer)
}

/// 生成含指定文件条目的测试 ZIP。
fn zip_payload(directory: &Path, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let path = directory.join("payload.zip");
    {
        let mut writer = zip::ZipWriter::new(File::create(&path).expect("创建 ZIP"));
        for (name, contents) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("开始 ZIP 条目");
            writer.write_all(contents).expect("写入 ZIP 条目");
        }
        writer.finish().expect("完成 ZIP");
    }
    fs::read(&path).expect("读取 ZIP")
}

/// 生成含指定文件条目的测试 7Z，可选用密码加密包头。
fn seven_zip_payload(
    directory: &Path,
    entries: &[(&str, &[u8])],
    password: Option<&str>,
) -> Vec<u8> {
    let path = directory.join("payload.7z");
    let mut writer = sevenz_rust2::ArchiveWriter::create(&path).expect("创建 7Z");
    if let Some(password) = password {
        writer.set_content_methods(vec![
            sevenz_rust2::EncoderConfiguration::new(sevenz_rust2::EncoderMethod::LZMA2),
            sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from(
                password,
            ))
            .into(),
        ]);
        writer.set_encrypt_header(true);
    }
    for (name, contents) in entries {
        let mut input: &[u8] = contents;
        writer
            .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(&mut input))
            .expect("写入 7Z 条目");
    }
    writer.finish().expect("完成 7Z");
    fs::read(&path).expect("读取 7Z")
}

/// 解压自解压文件并收集进度快照。
fn extract_with_progress(
    path: &Path,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
    cancel: Option<&AtomicBool>,
) -> (Result<usize, SfxError>, Vec<SfxProgress>) {
    let (file, footer) = open_sfx(path);
    let (sender, receiver) = mpsc::channel();
    let mut progress = move |snapshot: SfxProgress| {
        let _ = sender.send(snapshot);
    };
    let result = extract_payload_with_options(
        file,
        footer,
        target,
        SfxExtractOptions {
            password,
            overwrite,
            cancel,
            progress: Some(&mut progress),
        },
    );
    drop(progress);
    (result, receiver.try_iter().collect())
}

/// 断言进度序列单调不减，且总量信息保持不变。
fn assert_monotonic(snapshots: &[SfxProgress], bytes_total: u64, entries_total: usize) {
    if let Some(first) = snapshots.first() {
        assert_eq!(first.bytes_total, bytes_total);
        assert_eq!(first.entries_total, entries_total);
    }
    for window in snapshots.windows(2) {
        assert!(
            window[1].bytes_done >= window[0].bytes_done,
            "已处理字节数必须单调递增"
        );
        assert!(
            window[1].entries_done >= window[0].entries_done,
            "已处理条目数必须单调递增"
        );
        assert_eq!(window[0].bytes_total, bytes_total);
        assert_eq!(window[0].entries_total, entries_total);
    }
}

/// ZIP 解压应给出完整总量，进度只增不减，并写出全部文件。
#[test]
fn zip_progress_reports_totals_and_grows_monotonically() {
    let directory = tempfile::tempdir().unwrap();
    let large = vec![b'a'; 600 * 1024];
    let small = b"tiny".to_vec();
    let payload = zip_payload(
        directory.path(),
        &[
            ("large.bin", large.as_slice()),
            ("small.txt", small.as_slice()),
        ],
    );
    let sfx = write_sfx(directory.path(), &payload, SfxFormat::Zip, false, false);
    let target = directory.path().join("out");
    let expected_total = (large.len() + small.len()) as u64;
    let (result, snapshots) = extract_with_progress(&sfx, &target, None, true, None);
    assert_eq!(result.unwrap(), 2);
    assert!(!snapshots.is_empty(), "应产生进度回调");
    assert_monotonic(&snapshots, expected_total, 2);
    let last = snapshots.last().expect("至少有一次进度回调");
    assert_eq!(last.bytes_done, expected_total);
    assert_eq!(last.entries_done, 2);
    assert!(
        snapshots
            .iter()
            .any(|snapshot| snapshot.current == "large.bin"),
        "进度应包含条目名称"
    );
    assert_eq!(fs::read(target.join("large.bin")).unwrap(), large);
    assert_eq!(fs::read(target.join("small.txt")).unwrap(), small);
}

/// 7Z 解压应给出完整总量，进度只增不减，并写出全部文件。
#[test]
fn seven_zip_progress_reports_totals_and_grows_monotonically() {
    let directory = tempfile::tempdir().unwrap();
    let first = vec![b'7'; 300 * 1024];
    let second = b"seven".to_vec();
    let payload = seven_zip_payload(
        directory.path(),
        &[
            ("first.bin", first.as_slice()),
            ("second.txt", second.as_slice()),
        ],
        None,
    );
    let sfx = write_sfx(
        directory.path(),
        &payload,
        SfxFormat::SevenZip,
        false,
        false,
    );
    let target = directory.path().join("out");
    let expected_total = (first.len() + second.len()) as u64;
    let (result, snapshots) = extract_with_progress(&sfx, &target, None, true, None);
    assert_eq!(result.unwrap(), 2);
    assert!(!snapshots.is_empty(), "应产生进度回调");
    assert_monotonic(&snapshots, expected_total, 2);
    let last = snapshots.last().expect("至少有一次进度回调");
    assert_eq!(last.bytes_done, expected_total);
    assert_eq!(last.entries_done, 2);
    assert_eq!(fs::read(target.join("first.bin")).unwrap(), first);
    assert_eq!(fs::read(target.join("second.txt")).unwrap(), second);
}

/// 解压开始前已置位取消标记时直接返回 Cancelled，不写出文件。
#[test]
fn cancelled_before_start_returns_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(directory.path(), &[("hello.txt", b"hello")]);
    let sfx = write_sfx(directory.path(), &payload, SfxFormat::Zip, false, false);
    let target = directory.path().join("out");
    let cancel = AtomicBool::new(true);
    let (result, snapshots) = extract_with_progress(&sfx, &target, None, false, Some(&cancel));
    assert_eq!(result, Err(SfxError::Cancelled));
    assert!(snapshots.is_empty(), "预取消不应产生进度回调");
    assert!(!target.join("hello.txt").exists(), "预取消不应写出文件");
}

/// 加密 7Z 预取消时返回 Cancelled，而不是密码错误。
#[test]
fn cancelled_encrypted_seven_zip_is_not_a_password_error() {
    let directory = tempfile::tempdir().unwrap();
    let payload = seven_zip_payload(
        directory.path(),
        &[("secret.txt", b"top secret")],
        Some("right-pass"),
    );
    let sfx = write_sfx(directory.path(), &payload, SfxFormat::SevenZip, true, true);
    let target = directory.path().join("out");
    let cancel = AtomicBool::new(true);
    let (result, _) = extract_with_progress(&sfx, &target, None, false, Some(&cancel));
    assert_eq!(result, Err(SfxError::Cancelled));
}

/// ZIP 复制过程中取消应返回 Cancelled。
#[test]
fn cancelling_during_zip_copy_returns_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let contents = vec![b'x'; 4 * 1024 * 1024];
    let payload = zip_payload(directory.path(), &[("big.bin", contents.as_slice())]);
    let sfx = write_sfx(directory.path(), &payload, SfxFormat::Zip, false, false);
    let target = directory.path().join("out");
    let (file, footer) = open_sfx(&sfx);
    let cancel = AtomicBool::new(false);
    let (sender, receiver) = mpsc::channel();
    let mut progress = |snapshot: SfxProgress| {
        if snapshot.current_file_done > 0 {
            cancel.store(true, Ordering::Relaxed);
        }
        let _ = sender.send(snapshot);
    };
    let result = extract_payload_with_options(
        file,
        footer,
        &target,
        SfxExtractOptions {
            password: None,
            overwrite: true,
            cancel: Some(&cancel),
            progress: Some(&mut progress),
        },
    );
    let snapshots: Vec<SfxProgress> = receiver.try_iter().collect();
    assert_eq!(result, Err(SfxError::Cancelled));
    assert!(!snapshots.is_empty(), "取消前应已上报过进度");
}

/// 7Z 复制过程中取消应返回 Cancelled，而不是密码或数据错误。
#[test]
fn cancelling_during_seven_zip_copy_returns_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let contents = vec![b'y'; 2 * 1024 * 1024];
    let payload = seven_zip_payload(directory.path(), &[("big.bin", contents.as_slice())], None);
    let sfx = write_sfx(
        directory.path(),
        &payload,
        SfxFormat::SevenZip,
        false,
        false,
    );
    let target = directory.path().join("out");
    let (file, footer) = open_sfx(&sfx);
    let cancel = AtomicBool::new(false);
    let (sender, receiver) = mpsc::channel();
    let mut progress = |snapshot: SfxProgress| {
        if snapshot.current_file_done > 0 {
            cancel.store(true, Ordering::Relaxed);
        }
        let _ = sender.send(snapshot);
    };
    let result = extract_payload_with_options(
        file,
        footer,
        &target,
        SfxExtractOptions {
            password: None,
            overwrite: true,
            cancel: Some(&cancel),
            progress: Some(&mut progress),
        },
    );
    let snapshots: Vec<SfxProgress> = receiver.try_iter().collect();
    assert_eq!(result, Err(SfxError::Cancelled));
    assert!(!snapshots.is_empty(), "取消前应已上报过进度");
}

/// 覆盖关闭时跳过已有文件，但跳过部分仍计入已完成进度。
#[test]
fn skipped_existing_files_advance_progress() {
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(directory.path(), &[("hello.txt", b"from-archive")]);
    let sfx = write_sfx(directory.path(), &payload, SfxFormat::Zip, false, false);
    let target = directory.path().join("out");
    let (first, _) = extract_with_progress(&sfx, &target, None, true, None);
    assert_eq!(first.unwrap(), 1);
    fs::write(target.join("hello.txt"), b"keep-me").unwrap();
    let (second, snapshots) = extract_with_progress(&sfx, &target, None, false, None);
    assert_eq!(second.unwrap(), 0, "覆盖关闭时不应写出文件");
    assert_eq!(fs::read(target.join("hello.txt")).unwrap(), b"keep-me");
    let last = snapshots.last().expect("跳过也应上报进度");
    assert_eq!(last.bytes_done, last.bytes_total);
    assert_eq!(last.entries_done, last.entries_total);
}

/// 加密 7Z 包头解析失败仍映射为密码错误，不会被取消逻辑吞掉。
#[test]
fn encrypted_seven_zip_wrong_password_stays_wrong_password() {
    let directory = tempfile::tempdir().unwrap();
    let payload = seven_zip_payload(
        directory.path(),
        &[("secret.txt", b"top secret")],
        Some("right-pass"),
    );
    let sfx = write_sfx(directory.path(), &payload, SfxFormat::SevenZip, true, true);
    let target = directory.path().join("out");
    let (result, snapshots) = extract_with_progress(&sfx, &target, Some("wrong-pass"), false, None);
    assert_eq!(result, Err(SfxError::WrongPassword));
    assert!(snapshots.is_empty(), "包头解析失败时不应产生进度");
}
