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
    }
}

fn manager() -> Arc<TaskManager> {
    Arc::new(TaskManager::with_workers(1))
}

fn config() -> BatchExportConfig {
    BatchExportConfig {
        output_dir: PathBuf::from("out"),
        formats: vec![BatchExportFormat::Glb, BatchExportFormat::UeModel],
        include_materials: true,
        ..BatchExportConfig::default()
    }
}

/// 格式切换与数量限制。
#[test]
fn batch_window_toggles_formats_and_limits() {
    let mut view = BatchExportWindowView::new(target(), manager(), config());
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
    let (_view, cx) =
        cx.add_window_view(|_, _| BatchExportWindowView::new(target(), manager(), config()));
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
        move |_, _| BatchExportWindowView::new(target(), manager, config())
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

/// 游戏保存的导出配置会作为批量窗口初始值。
#[test]
fn batch_window_uses_saved_config() {
    let saved = BatchExportConfig {
        output_dir: PathBuf::from("D:/Game/LumarkExports"),
        formats: vec![BatchExportFormat::Usda, BatchExportFormat::Psk],
        asset_filter: Some("Meshes".to_string()),
        limit: Some(50),
        overwrite: true,
        include_materials: false,
        ..BatchExportConfig::default()
    };
    let view = BatchExportWindowView::new(target(), manager(), saved);
    assert_eq!(
        view.config.output_dir,
        PathBuf::from("D:/Game/LumarkExports")
    );
    assert_eq!(
        view.config.formats,
        vec![BatchExportFormat::Psk, BatchExportFormat::Usda]
    );
    assert_eq!(view.config.asset_filter.as_deref(), Some("Meshes"));
    assert_eq!(view.config.limit, Some(50));
    assert!(view.config.overwrite);
    assert!(!view.config.include_materials);
}
