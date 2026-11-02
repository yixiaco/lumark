use super::*;
use gpui_kit::test::TestWindowExt;
use lumark_tasks::TaskEvent;

fn target() -> BatchExportTarget {
    BatchExportTarget {
        archive: PathBuf::from("Game/pakchunk0-Windows.utoc"),
        global: PathBuf::from("Game/global.utoc"),
        mappings: Some(PathBuf::from("Game/mapping.usmap")),
        oodle: None,
        key: None,
        output_dir: PathBuf::from("out"),
    }
}

fn manager() -> Arc<TaskManager> {
    Arc::new(TaskManager::with_workers(1))
}

/// 格式切换与数量限制。
#[test]
fn batch_window_toggles_formats_and_limits() {
    let mut view = BatchExportWindowView::new(target(), manager());
    assert_eq!(view.config.formats.len(), 2);
    view.toggle_format(BatchExportFormat::Glb);
    assert_eq!(view.config.formats, vec![BatchExportFormat::UeModel]);
    view.toggle_format(BatchExportFormat::Usda);
    assert!(view.config.formats.contains(&BatchExportFormat::Usda));
    view.config.limit = Some(100);
    assert_eq!(view.config.limit, Some(100));
    assert!(view.config.include_materials);
}

/// 批量窗口渲染关键控件。
#[gpui_kit::test]
fn batch_window_renders_controls(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_view, cx) = cx.add_window_view(|_, _| BatchExportWindowView::new(target(), manager()));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("batch-start").visible());
        assert!(window.find("batch-format-glb").visible());
        assert!(window.find("batch-limit-100").visible());
        assert!(window.find("batch-materials").visible());
        assert!(window.find("batch-overwrite").visible());
    });
}

/// 开始导出应把任务提交给 TaskManager，类型为 Export。
#[gpui_kit::test]
fn batch_window_submits_task_to_manager(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let manager = manager();
    let (view, cx) = cx.add_window_view({
        let manager = manager.clone();
        move |_, _| BatchExportWindowView::new(target(), manager)
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| view.start(cx));
    });
    let mut queued = false;
    for _ in 0..100 {
        while let Some(event) = manager.try_recv() {
            if let TaskEvent::Queued {
                kind: TaskKind::Export,
                ..
            } = event
            {
                queued = true;
            }
        }
        if queued {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(queued, "批量导出应提交 Export 类型任务");
}
