//! ZPAQ 后端集成测试：验证真实内嵌 JIDAC 引擎的创建、列表、解压和密码链路。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lumark_archive::{
    ArchiveError, ArchiveFormat, ArchiveService, CompressionOptions, CompressionPreset,
    ExtractionBudget, ExtractionOptions, OverwritePolicy, PathMode,
};
use tempfile::TempDir;
use zeroize::Zeroizing;

/// 构造一个包含嵌套目录、空文件、中文和空格名称的源目录。
fn make_tree(root: &Path) {
    fs::create_dir_all(root.join("dir/sub")).unwrap();
    fs::write(root.join("dir/a.txt"), b"alpha alpha alpha\n").unwrap();
    fs::write(root.join("dir/sub/b.bin"), [0u8, 1, 2, 3, 4]).unwrap();
    fs::write(root.join("空 格.txt"), "中文内容".as_bytes()).unwrap();
    fs::write(root.join("empty.txt"), b"").unwrap();
}

/// 生成只关注格式的低级压缩参数。
fn create_options(format: ArchiveFormat) -> CompressionOptions {
    CompressionOptions {
        format,
        level: Some(1),
        preset: CompressionPreset::Speed,
        threads: 2,
        ..Default::default()
    }
}

/// 默认解压参数：指定输出目录并允许覆盖。
fn extract_options(output: PathBuf) -> ExtractionOptions {
    ExtractionOptions {
        output_dir: output,
        overwrite: true,
        ..Default::default()
    }
}

/// 读取归档列表中的路径集合。
fn listed_paths(archive: &Path) -> Vec<String> {
    ArchiveService::list(archive, None)
        .expect("ZPAQ 列表应成功")
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect()
}

/// 读取文件修改时间对应的 Unix 秒数。
fn modified_seconds(path: &Path) -> i64 {
    fs::metadata(path)
        .expect("应能读取文件元数据")
        .modified()
        .expect("应有修改时间")
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_else(|error| -(error.duration().as_secs() as i64))
}

/// 创建 ZPAQ 归档后应能列出全部条目并完整解压。
#[test]
fn zpaq_roundtrip_lists_and_extracts_all_entries() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source");
    make_tree(&source);
    let archive = directory.path().join("archive.zpaq");
    let report = ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &create_options(ArchiveFormat::Zpaq),
        |_| {},
    )
    .expect("ZPAQ 创建应成功");
    assert!(archive.is_file());
    assert_eq!(report.files, 4);

    let info = ArchiveService::list(&archive, None).expect("ZPAQ 列表应成功");
    assert_eq!(info.format, Some(ArchiveFormat::Zpaq));
    let paths = info
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        paths.contains(&"source/dir/a.txt"),
        "列表缺少嵌套文件：{paths:?}"
    );
    assert!(
        paths.contains(&"source/空 格.txt"),
        "列表缺少中文文件：{paths:?}"
    );
    assert!(
        paths.contains(&"source/empty.txt"),
        "列表缺少空文件：{paths:?}"
    );

    let output = directory.path().join("out");
    let extracted = ArchiveService::extract(&archive, &extract_options(output.clone()), |_| {})
        .expect("ZPAQ 解压应成功");
    assert_eq!(extracted.files, 4);
    assert_eq!(
        fs::read(output.join("source/dir/a.txt")).unwrap(),
        b"alpha alpha alpha\n"
    );
    assert_eq!(
        fs::read(output.join("source/dir/sub/b.bin")).unwrap(),
        [0u8, 1, 2, 3, 4]
    );
    assert_eq!(
        fs::read(output.join("source/空 格.txt")).unwrap(),
        "中文内容".as_bytes()
    );
    assert!(output.join("source/empty.txt").is_file());
    assert_eq!(
        fs::metadata(output.join("source/empty.txt")).unwrap().len(),
        0
    );
}

/// 带密码归档应能正确探测加密、校验密码、测试并解压。
#[test]
fn zpaq_password_probe_verify_and_extract() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source");
    make_tree(&source);
    let archive = directory.path().join("secret.zpaq");
    let mut options = create_options(ArchiveFormat::Zpaq);
    options.password = Some(Zeroizing::new("zpaq-pass".to_string()));
    options.test_after = true;
    ArchiveService::create(&archive, std::slice::from_ref(&source), &options, |_| {})
        .expect("加密 ZPAQ 创建应成功");

    let probe = ArchiveService::probe_encryption(&archive).expect("加密探测应成功");
    assert!(probe.encrypted);
    assert!(probe.header_encrypted);

    assert!(matches!(
        ArchiveService::list(&archive, None),
        Err(ArchiveError::PasswordRequired)
    ));
    assert!(matches!(
        ArchiveService::list(&archive, Some("wrong")),
        Err(ArchiveError::WrongPassword)
    ));
    assert!(ArchiveService::verify_password(&archive, "zpaq-pass").unwrap());
    assert!(!ArchiveService::verify_password(&archive, "wrong").unwrap());
    let info = ArchiveService::list(&archive, Some("zpaq-pass")).expect("正确密码应能列表");
    assert!(info.encrypted);
    assert_eq!(info.entries.len(), 4);

    let tested =
        ArchiveService::test(&archive, Some("zpaq-pass"), |_| {}).expect("ZPAQ 测试应通过");
    assert_eq!(tested.files, 4);

    let output = directory.path().join("out");
    let mut extract = extract_options(output.clone());
    extract.password = Some(Zeroizing::new("zpaq-pass".to_string()));
    let report = ArchiveService::extract(&archive, &extract, |_| {}).expect("正确密码应能解压");
    assert_eq!(report.files, 4);
    assert_eq!(
        fs::read(output.join("source/dir/a.txt")).unwrap(),
        b"alpha alpha alpha\n"
    );
}

/// 解压覆盖策略应分别实现跳过、重命名和覆盖。
#[test]
fn zpaq_extract_overwrite_policies() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("a.txt"), b"new content").unwrap();
    let archive = directory.path().join("policy.zpaq");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &create_options(ArchiveFormat::Zpaq),
        |_| {},
    )
    .unwrap();

    let prepare = |name: &str| {
        let output = directory.path().join(name);
        fs::create_dir_all(output.join("source")).unwrap();
        fs::write(output.join("source/a.txt"), b"old content").unwrap();
        output
    };

    let skip_output = prepare("skip");
    let mut skip = extract_options(skip_output.clone());
    skip.overwrite = false;
    skip.overwrite_policy = OverwritePolicy::Skip;
    let report = ArchiveService::extract(&archive, &skip, |_| {}).unwrap();
    assert_eq!(report.files, 0);
    assert_eq!(report.skipped, 1);
    assert_eq!(
        fs::read(skip_output.join("source/a.txt")).unwrap(),
        b"old content"
    );

    let rename_output = prepare("rename");
    let mut rename = extract_options(rename_output.clone());
    rename.overwrite_policy = OverwritePolicy::Rename;
    let report = ArchiveService::extract(&archive, &rename, |_| {}).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(
        fs::read(rename_output.join("source/a.txt")).unwrap(),
        b"old content"
    );
    assert_eq!(
        fs::read(rename_output.join("source/a (1).txt")).unwrap(),
        b"new content"
    );

    let overwrite_output = prepare("overwrite");
    let mut overwrite = extract_options(overwrite_output.clone());
    overwrite.overwrite_policy = OverwritePolicy::Overwrite;
    ArchiveService::extract(&archive, &overwrite, |_| {}).unwrap();
    assert_eq!(
        fs::read(overwrite_output.join("source/a.txt")).unwrap(),
        b"new content"
    );
}

/// 解压预算应在写入用户目录前拦截超限归档。
#[test]
fn zpaq_extract_enforces_budget() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("big.bin"), vec![7u8; 4096]).unwrap();
    let archive = directory.path().join("budget.zpaq");
    ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &create_options(ArchiveFormat::Zpaq),
        |_| {},
    )
    .unwrap();

    let output = directory.path().join("out");
    let mut options = extract_options(output.clone());
    options.budget = ExtractionBudget {
        max_bytes: 1024,
        max_entries: 10,
    };
    assert!(matches!(
        ArchiveService::extract(&archive, &options, |_| {}),
        Err(ArchiveError::BudgetExceeded(_))
    ));
    assert!(!output.join("source/big.bin").exists());

    // 流式归档没有声明大小，预算必须在暂存解压后用实际大小拦截。
    let streaming = directory.path().join("streaming.zpaq");
    let payload = vec![9u8; 4096];
    let bytes = zpaq_rs::archive_from_entries(
        &[zpaq_rs::ArchiveEntry {
            path: "streaming/big.bin",
            data: &payload,
            comment: None,
        }],
        "1",
    )
    .unwrap();
    fs::write(&streaming, bytes).unwrap();
    let streaming_output = directory.path().join("streaming-out");
    let mut streaming_options = extract_options(streaming_output.clone());
    streaming_options.budget = ExtractionBudget {
        max_bytes: 1024,
        max_entries: 10,
    };
    assert!(matches!(
        ArchiveService::extract(&streaming, &streaming_options, |_| {}),
        Err(ArchiveError::BudgetExceeded(_))
    ));
    assert!(!streaming_output.join("streaming/big.bin").exists());
}

/// 压缩过程中取消应删除半成品并保留源文件。
#[test]
fn zpaq_create_cancels_between_batches() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("many");
    fs::create_dir_all(&source).unwrap();
    for index in 0..200 {
        fs::write(
            source.join(format!("file-{index:03}.txt")),
            vec![b'x'; 1024],
        )
        .unwrap();
    }
    let archive = directory.path().join("cancelled.zpaq");
    let cancel = Arc::new(AtomicBool::new(false));
    let mut options = create_options(ArchiveFormat::Zpaq);
    options.cancel = Some(cancel.clone());
    let flag = cancel.clone();
    let result = ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &options,
        |progress| {
            if progress.entries_done > 0 {
                flag.store(true, Ordering::Relaxed);
            }
        },
    );
    assert!(matches!(result, Err(ArchiveError::Cancelled)));
    assert!(!archive.exists());
    assert!(source.join("file-000.txt").is_file());
}

/// 路径模式、排除模式和时间筛选应在创建计划阶段生效。
#[test]
fn zpaq_create_respects_path_modes_and_filters() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("root");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("a.txt"), b"a").unwrap();
    fs::write(source.join("sub/b.txt"), b"b").unwrap();
    fs::write(source.join("skip.tmp"), b"tmp").unwrap();
    let old = SystemTime::now() - std::time::Duration::from_secs(86_400 * 30);
    filetime::set_file_mtime(
        source.join("a.txt"),
        filetime::FileTime::from_system_time(old),
    )
    .unwrap();

    let relative = directory.path().join("relative.zpaq");
    ArchiveService::create(
        &relative,
        std::slice::from_ref(&source),
        &create_options(ArchiveFormat::Zpaq),
        |_| {},
    )
    .unwrap();
    let paths = listed_paths(&relative);
    assert!(paths.contains(&"root/a.txt".to_string()));
    assert!(paths.contains(&"root/sub/b.txt".to_string()));

    let no_root = directory.path().join("no-root.zpaq");
    let mut no_root_options = create_options(ArchiveFormat::Zpaq);
    no_root_options.path_mode = PathMode::NoRoot;
    ArchiveService::create(
        &no_root,
        std::slice::from_ref(&source),
        &no_root_options,
        |_| {},
    )
    .unwrap();
    let paths = listed_paths(&no_root);
    assert!(paths.contains(&"a.txt".to_string()));
    assert!(paths.contains(&"sub/b.txt".to_string()));

    let flat = directory.path().join("flat.zpaq");
    let mut flat_options = create_options(ArchiveFormat::Zpaq);
    flat_options.path_mode = PathMode::Flatten;
    ArchiveService::create(&flat, std::slice::from_ref(&source), &flat_options, |_| {}).unwrap();
    let paths = listed_paths(&flat);
    assert!(paths.contains(&"a.txt".to_string()));
    assert!(paths.contains(&"b.txt".to_string()));

    let filtered = directory.path().join("filtered.zpaq");
    let mut filtered_options = create_options(ArchiveFormat::Zpaq);
    filtered_options.exclude_patterns = vec!["**/*.tmp".to_string()];
    filtered_options.file_time_filter = lumark_archive::FileTimeFilter::After;
    filtered_options.modified_after = Some(chrono::DateTime::<chrono::Utc>::from(
        SystemTime::now() - std::time::Duration::from_secs(86_400),
    ));
    ArchiveService::create(
        &filtered,
        std::slice::from_ref(&source),
        &filtered_options,
        |_| {},
    )
    .unwrap();
    let paths = listed_paths(&filtered);
    assert!(!paths.iter().any(|path| path.ends_with("skip.tmp")));
    assert!(!paths.iter().any(|path| path.ends_with("a.txt")));
    assert!(paths.iter().any(|path| path.ends_with("sub/b.txt")));
}

/// 归档时间选项应改变解压后文件的修改时间。
#[test]
fn zpaq_archive_time_changes_extracted_mtime() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("old.txt"), b"old").unwrap();
    fs::write(source.join("new.txt"), b"new").unwrap();
    let old = 1_577_939_045;
    let new = 1_622_722_506;
    filetime::set_file_mtime(
        source.join("old.txt"),
        filetime::FileTime::from_unix_time(old, 0),
    )
    .unwrap();
    filetime::set_file_mtime(
        source.join("new.txt"),
        filetime::FileTime::from_unix_time(new, 0),
    )
    .unwrap();

    let latest = directory.path().join("latest.zpaq");
    let mut latest_options = create_options(ArchiveFormat::Zpaq);
    latest_options.archive_time = lumark_archive::ArchiveTimeSource::Latest;
    ArchiveService::create(
        &latest,
        std::slice::from_ref(&source),
        &latest_options,
        |_| {},
    )
    .unwrap();
    let output = directory.path().join("latest-out");
    ArchiveService::extract(&latest, &extract_options(output.clone()), |_| {}).unwrap();
    assert_eq!(modified_seconds(&output.join("source/old.txt")), new);
    assert_eq!(modified_seconds(&output.join("source/new.txt")), new);

    let original = directory.path().join("original.zpaq");
    ArchiveService::create(
        &original,
        std::slice::from_ref(&source),
        &create_options(ArchiveFormat::Zpaq),
        |_| {},
    )
    .unwrap();
    let output = directory.path().join("original-out");
    ArchiveService::extract(&original, &extract_options(output.clone()), |_| {}).unwrap();
    assert_eq!(modified_seconds(&output.join("source/old.txt")), old);
    assert_eq!(modified_seconds(&output.join("source/new.txt")), new);
}

/// 空计划、重复路径和以 `-` 开头的顶层名称应被拒绝。
#[test]
fn zpaq_create_rejects_invalid_plans() {
    let directory = TempDir::new().unwrap();
    let empty = directory.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        ArchiveService::create(
            &directory.path().join("empty.zpaq"),
            &[empty],
            &create_options(ArchiveFormat::Zpaq),
            |_| {}
        ),
        Err(ArchiveError::InvalidRequest(_))
    ));

    let source = directory.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("a.txt"), b"a").unwrap();
    assert!(matches!(
        ArchiveService::create(
            &directory.path().join("duplicate.zpaq"),
            &[source.clone(), source.join("a.txt")],
            &create_options(ArchiveFormat::Zpaq),
            |_| {}
        ),
        Err(ArchiveError::InvalidRequest(_))
    ));

    let dash = directory.path().join("dash");
    fs::create_dir_all(&dash).unwrap();
    fs::write(dash.join("-dash.txt"), b"dash").unwrap();
    let mut options = create_options(ArchiveFormat::Zpaq);
    options.path_mode = PathMode::NoRoot;
    assert!(matches!(
        ArchiveService::create(
            &directory.path().join("dash.zpaq"),
            &[dash],
            &options,
            |_| {}
        ),
        Err(ArchiveError::InvalidRequest(_))
    ));
}

/// 外部生成的流式归档应能列表和解压，大小未知时按 0 展示。
#[test]
fn zpaq_streaming_archive_lists_and_extracts() {
    let directory = TempDir::new().unwrap();
    let archive = directory.path().join("streaming.zpaq");
    let payload = b"streaming payload";
    let bytes = zpaq_rs::archive_from_entries(
        &[zpaq_rs::ArchiveEntry {
            path: "stream/one.txt",
            data: payload,
            comment: None,
        }],
        "1",
    )
    .unwrap();
    fs::write(&archive, bytes).unwrap();

    let info = ArchiveService::list(&archive, None).expect("流式归档列表应成功");
    assert_eq!(info.entries.len(), 1);
    assert_eq!(info.entries[0].path, "stream/one.txt");
    assert_eq!(info.entries[0].size, 0);

    let output = directory.path().join("out");
    let report = ArchiveService::extract(&archive, &extract_options(output.clone()), |_| {})
        .expect("流式归档解压应成功");
    assert_eq!(report.files, 1);
    assert_eq!(fs::read(output.join("stream/one.txt")).unwrap(), payload);
}
