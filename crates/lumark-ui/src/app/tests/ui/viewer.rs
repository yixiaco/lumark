//! 文本查看器和归档添加弹窗测试。

use super::*;

/// 验证内置文本查看器可以渲染只读编辑器和状态行。
#[gpui_kit::test]
fn text_viewer_renders_readonly_editor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_view, cx) = cx.add_window_view(|window, cx| {
        crate::app::view::text_viewer_window::TextViewerWindowView::new(
            "第一行\n第二行\n".to_string(),
            "note.txt".to_string(),
            r"C:\downloads\note.txt".to_string(),
            18,
            window,
            cx,
        )
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}

/// 验证内置查看器能从普通文本文件读取内容并打开窗口。
#[gpui_kit::test]
fn text_viewer_opens_for_text_file(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("note.txt");
    std::fs::write(&file, "hello lumark\n").unwrap();
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
            app.open_text_viewer(file.clone(), "note.txt".to_string(), window, cx);
            assert!(
                app.status.contains("已在内置查看器中打开"),
                "{}",
                app.status
            );
        });
    });
    let _ = app;
}

/// 验证拖入归档的文件先打开更新模式确认框，确认后才提交编辑任务。
#[gpui_kit::test]
fn archive_add_dialog_selects_update_mode(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let archive = directory.path().join("target.zip");
    let archived = directory.path().join("hello.txt");
    std::fs::write(&archived, b"original").unwrap();
    ArchiveService::create(
        &archive,
        &[archived],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let dropped = directory.path().join("dropped.txt");
    std::fs::write(&dropped, b"dropped").unwrap();
    let dropped_folder = directory.path().join("dropped-folder");
    std::fs::create_dir_all(&dropped_folder).unwrap();
    std::fs::write(dropped_folder.join("inside.txt"), b"inside").unwrap();
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
            app.browser.open_archive(&archive, None).unwrap();
            app.add_files_to_current_archive(&[dropped, dropped_folder]);
            assert!(app.show_archive_add_dialog);
            assert_eq!(app.pending_archive_additions.len(), 1);
            assert_eq!(app.pending_archive_roots.len(), 1);
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(window.find("archive-add-confirm").visible());
        assert!(window.find("archive-add-freshen").visible());
        assert!(
            window.find("archive-add-sync").visible(),
            "拖入文件夹后应提供同步模式"
        );
        window.click("archive-add-sync", cx);
        window.draw(cx).clear(cx);
        app.update(cx, |app, _| {
            assert_eq!(app.archive_add_mode, lumark_archive::UpdateMode::Sync);
        });
        window.click("archive-add-confirm", cx);
        app.update(cx, |app, _| {
            assert!(!app.show_archive_add_dialog);
            assert!(app.pending_archive_additions.is_empty());
            assert!(app.pending_archive_roots.is_empty());
        });
    });
    let _ = app;
}
