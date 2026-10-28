//! 任务中心和进度窗口测试。

use super::*;

/// 验证跨任务聚合只统计非终态任务，并按字节累加运行任务的进度。
#[test]
fn task_aggregate_sums_running_progress() {
    let progress = |bytes_done: u64, bytes_total: u64| TaskProgress {
        bytes_done,
        bytes_total,
        ..Default::default()
    };
    let record = |id: u64, state: TaskState, progress: TaskProgress| TaskRecord {
        id,
        kind: TaskKind::Compress,
        label: String::new(),
        state,
        progress,
        message: None,
    };
    let records = vec![
        record(1, TaskState::Running, progress(50, 100)),
        record(2, TaskState::Paused, progress(10, 100)),
        record(3, TaskState::Queued, progress(0, 0)),
        record(4, TaskState::Completed, progress(100, 100)),
        record(5, TaskState::Failed, progress(0, 0)),
    ];
    let aggregate = aggregate_task_progress(&records);
    assert_eq!(aggregate.running, 2);
    assert_eq!(aggregate.queued, 1);
    assert_eq!(aggregate.finished, 2);
    assert_eq!(aggregate.bytes_done, 60);
    assert_eq!(aggregate.bytes_total, 200);
    assert!((aggregate.ratio() - 0.3).abs() < f32::EPSILON);
}

/// 验证自动进度窗口不会为已经结束的任务补开窗口。
#[gpui_kit::test]
fn auto_progress_window_skips_finished_task(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            directory.path().join("vault.dat"),
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.task_records.push(TaskRecord {
                id: 7,
                kind: TaskKind::Extract,
                label: "解压 note.txt".to_string(),
                state: TaskState::Completed,
                progress: TaskProgress::default(),
                message: Some("已解压 1 个文件".to_string()),
            });
            app.open_auto_progress_window_for(7, window, cx);
            assert!(
                app.progress_windows.is_empty(),
                "已结束任务不应自动补开进度窗口"
            );
        });
    });
    let _ = app;
}

/// 验证任务中心显示排队数量和跨任务总进度。
#[gpui_kit::test]
fn task_center_shows_aggregate_progress(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            directory.path().join("vault.dat"),
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.task_records.push(TaskRecord {
                id: 1,
                kind: TaskKind::Compress,
                label: "压缩 归档".to_string(),
                state: TaskState::Running,
                progress: TaskProgress {
                    bytes_done: 50,
                    bytes_total: 100,
                    ..Default::default()
                },
                message: None,
            });
            app.task_records.push(TaskRecord {
                id: 2,
                kind: TaskKind::Extract,
                label: "解压 归档".to_string(),
                state: TaskState::Queued,
                progress: TaskProgress::default(),
                message: None,
            });
            app.screen = Screen::Tasks;
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("task-aggregate-progress").visible(),
            "任务中心未渲染跨任务总进度"
        );
    });
    let _ = app;
}

/// 验证任务中心为运行中任务渲染暂停按钮，点击缺少控制器时不 panic。
#[gpui_kit::test]
fn task_center_renders_pause_control(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            directory.path().join("vault.dat"),
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.screen = Screen::Tasks;
            app.task_records.push(TaskRecord {
                id: 42,
                kind: TaskKind::Compress,
                label: "测试压缩".to_string(),
                state: TaskState::Running,
                progress: TaskProgress::default(),
                message: None,
            });
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(window.find("task-pause-42").visible());
        window.click("task-pause-42", cx);
        window.draw(cx).clear(cx);
    });
    let _ = app;
}
