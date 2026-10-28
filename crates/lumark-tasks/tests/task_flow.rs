//! 后台任务提交、取消、密码和归档加载集成测试。

use std::fs;
use std::time::{Duration, Instant};

use lumark_archive::{
    ArchiveFormat, ArchiveService, CompressionOptions, EditOptions, ExtractionOptions,
};
use lumark_tasks::{TaskEvent, TaskManager, TaskRequest, TaskState};
use tempfile::tempdir;
use zeroize::Zeroizing;

#[test]
/// 验证压缩和解压通过任务管理器执行。
fn compress_and_extract_through_task_manager() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("hello.txt"), b"task flow").unwrap();
    let archive = dir.path().join("out.zip");

    let manager = TaskManager::new();
    let id = manager.submit(TaskRequest::Compress {
        output: archive.clone(),
        sources: vec![source],
        options: CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        sfx_stub: None,
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Completed { id: event_id, .. } = event
                && event_id == id
            {
                completed = true;
            }
        }
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(completed);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );

    let output = dir.path().join("output");
    let id = manager.submit(TaskRequest::Extract {
        archive: archive.clone(),
        options: ExtractionOptions {
            output_dir: output.clone(),
            overwrite: true,
            ..Default::default()
        },
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Completed { id: event_id, .. } = event
                && event_id == id
            {
                completed = true;
            }
        }
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(completed);
    assert_eq!(
        fs::read(output.join("source").join("hello.txt")).unwrap(),
        b"task flow"
    );
    let test_id = manager.submit(TaskRequest::Test {
        archive,
        password: None,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut test_completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Completed { id, .. } = event
                && id == test_id
            {
                test_completed = true;
            }
        }
        if test_completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(test_completed);
}

/// 验证任务层会把 ZIP 数据和运行时模块拼成自解压 EXE。
#[test]
fn self_extracting_archive_runs_through_the_task_manager() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("hello.txt");
    fs::write(&source, b"sfx payload").unwrap();
    let stub = dir.path().join("lumark-sfx.exe");
    let stub_bytes = b"MZ-stub";
    fs::write(&stub, stub_bytes).unwrap();
    let output = dir.path().join("bundle.exe");

    let manager = TaskManager::new();
    let id = manager.submit(TaskRequest::Compress {
        output: output.clone(),
        sources: vec![source],
        options: CompressionOptions {
            format: ArchiveFormat::Zip,
            sfx: true,
            ..Default::default()
        },
        sfx_stub: Some(stub),
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Completed { id: event_id, .. } = event
                && event_id == id
            {
                completed = true;
            }
        }
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(completed, "自解压任务未在限期内完成");
    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(stub_bytes));
    assert_eq!(&bytes[bytes.len() - 32..][..8], b"LMSFX002");
    let offset =
        u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into().unwrap()) as usize;
    let length = u64::from_le_bytes(bytes[bytes.len() - 8..].try_into().unwrap()) as usize;
    assert_eq!(offset, stub_bytes.len());
    let payload = dir.path().join("payload.zip");
    fs::write(&payload, &bytes[offset..offset + length]).unwrap();
    let info = ArchiveService::list(&payload, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));
}

#[test]
/// 验证密码候选在后台验证并上报尝试次数。
fn password_resolution_runs_in_background_and_reports_attempts() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("secret.txt");
    fs::write(&source, b"resolved in the task worker").unwrap();
    let archive = dir.path().join("secret.zip");
    ArchiveService::create(
        &archive,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            password: Some(Zeroizing::new("correct-password".to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();

    let manager = TaskManager::new();
    let id = manager.submit(TaskRequest::ResolvePassword {
        archive,
        candidates: vec![
            Zeroizing::new("wrong-password".to_string()),
            Zeroizing::new("correct-password".to_string()),
        ],
        load_info: true,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut attempts = Vec::new();
    let mut completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            match event {
                TaskEvent::PasswordAttempt {
                    id: event_id,
                    index,
                    total,
                } if event_id == id => attempts.push((index, total)),
                TaskEvent::Completed { id: event_id, .. } if event_id == id => {
                    completed = true;
                }
                _ => {}
            }
        }
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(completed);
    assert_eq!(attempts, vec![(1, 2), (2, 2)]);
    let resolved = manager.take_password_result(id).unwrap();
    assert_eq!(resolved.candidate_index, 1);
    assert_eq!(resolved.password.as_str(), "correct-password");
    assert!(manager.take_password_result(id).is_none());
    let loaded = manager.take_archive_load_result(id).unwrap();
    assert!(loaded.info.encrypted);
    assert!(!loaded.info.entries.is_empty());
}

#[test]
/// 验证归档编辑通过任务管理器执行。
fn archive_edit_runs_through_the_task_manager() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("payload.txt");
    fs::write(&source, b"edit task").unwrap();
    let archive = dir.path().join("edit-task.zip");
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

    let manager = TaskManager::new();
    let id = manager.submit(TaskRequest::Edit {
        archive: archive.clone(),
        options: EditOptions {
            rename: vec![("payload.txt".to_string(), "renamed.txt".to_string())],
            ..Default::default()
        },
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Completed { id: event_id, .. } = event
                && event_id == id
            {
                completed = true;
            }
        }
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(completed);
    let info = ArchiveService::list(&archive, None).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "renamed.txt"));
}

#[test]
/// 验证归档加载结果可直接消费而无需重新列表。
fn archive_load_result_can_be_consumed_without_relisting() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("listed.txt");
    fs::write(&source, b"listed payload").unwrap();
    let archive = dir.path().join("listed.zip");
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

    let manager = TaskManager::new();
    let id = manager.submit(TaskRequest::LoadArchive {
        archive,
        password: None,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut completed = false;
    while Instant::now() < deadline {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Completed { id: event_id, .. } = event
                && event_id == id
            {
                completed = true;
            }
        }
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(completed);
    let loaded = manager.take_archive_load_result(id).unwrap();
    assert!(
        loaded
            .info
            .entries
            .iter()
            .any(|entry| entry.path == "listed.txt")
    );
    assert!(loaded.password.is_none());
}

/// 验证单工作线程下任务严格按提交顺序串行执行。
#[test]
fn single_worker_runs_tasks_in_submission_order() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("payload.txt");
    fs::write(&source, b"queue ordering").unwrap();
    let manager = TaskManager::with_workers(1);
    let first = manager.submit(TaskRequest::Compress {
        output: dir.path().join("first.zip"),
        sources: vec![source.clone()],
        options: CompressionOptions::default(),
        sfx_stub: None,
    });
    let second = manager.submit(TaskRequest::Compress {
        output: dir.path().join("second.zip"),
        sources: vec![source],
        options: CompressionOptions::default(),
        sfx_stub: None,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    let mut done = 0;
    while Instant::now() < deadline && done < 2 {
        while let Some(event) = manager.try_recv() {
            match event {
                TaskEvent::StateChanged {
                    id,
                    state: TaskState::Running,
                } => events.push(("running", id)),
                TaskEvent::Completed { id, .. } => {
                    events.push(("completed", id));
                    done += 1;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(done, 2, "两个任务都应在限期内完成：{events:?}");
    let first_completed = events
        .iter()
        .position(|(kind, id)| *kind == "completed" && *id == first)
        .expect("第一个任务完成事件");
    let second_running = events
        .iter()
        .position(|(kind, id)| *kind == "running" && *id == second)
        .expect("第二个任务开始事件");
    assert!(
        second_running > first_completed,
        "单工作线程必须串行执行：{events:?}"
    );
}

/// 验证排队中的任务可以在开始前取消，且不会产生输出文件。
#[test]
fn queued_task_can_be_cancelled_before_starting() {
    let dir = tempdir().unwrap();
    // 用不可压缩数据让第一个任务保持运行，确保第二个任务一定在排队。
    let source = dir.path().join("slow.bin");
    let mut data = Vec::with_capacity(3 * 1024 * 1024);
    let mut state = 0x1234_5678u32;
    for _ in 0..3 * 1024 * 1024u32 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        data.push((state >> 24) as u8);
    }
    fs::write(&source, &data).unwrap();

    let manager = TaskManager::with_workers(1);
    let first = manager.submit(TaskRequest::Compress {
        output: dir.path().join("slow.7z"),
        sources: vec![source.clone()],
        options: CompressionOptions {
            format: ArchiveFormat::SevenZip,
            level: Some(9),
            ..Default::default()
        },
        sfx_stub: None,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut first_running = false;
    while Instant::now() < deadline && !first_running {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::StateChanged {
                id,
                state: TaskState::Running,
            } = event
                && id == first
            {
                first_running = true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(first_running, "第一个任务应已开始");

    let queued_output = dir.path().join("queued.zip");
    let queued = manager.submit(TaskRequest::Compress {
        output: queued_output.clone(),
        sources: vec![source],
        options: CompressionOptions::default(),
        sfx_stub: None,
    });
    manager.control(queued).expect("排队任务的控制器").cancel();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut queued_cancelled = false;
    while Instant::now() < deadline && !queued_cancelled {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Cancelled { id } = event
                && id == queued
            {
                queued_cancelled = true;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(queued_cancelled, "排队中的任务应能取消");
    assert!(!queued_output.exists(), "取消的排队任务不应产生输出");
}
