//! 文件动作与拖入归档场景测试。

use super::*;

/// 验证拖入归档的文件会进入添加确认弹窗，而不是直接覆盖。
#[gpui_kit::test]
fn dropped_files_open_archive_add_confirmation(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("inside.txt");
    std::fs::write(&source, b"inside").unwrap();
    let archive = directory.path().join("target.zip");
    lumark_archive::ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &lumark_archive::CompressionOptions {
            format: lumark_archive::ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let dropped = directory.path().join("dropped.txt");
    std::fs::write(&dropped, b"dropped").unwrap();
    let mut app_slot = None;
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let app = cx.new(|cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        app_slot = Some(app.clone());
        gpui_kit::base::Root::new(app, window, cx)
    });
    let app = app_slot.expect("测试窗口应创建应用视图");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.browser.open_archive(&archive, None).unwrap();
            app.handle_external_drop(std::slice::from_ref(&dropped), window, cx);
            assert_eq!(
                app.pending_archive_additions,
                vec![("dropped.txt".to_string(), dropped)]
            );
        });
        window.draw(cx).clear(cx);
        assert!(window.find("archive-add-confirm").visible());
    });
    let _ = app;
}

/// 新建文件夹对话框可创建文件夹，取消时不写入磁盘。
#[gpui_kit::test]
fn new_folder_dialog_creates_and_cancels(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let mut app_slot = None;
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let app = cx.new(|cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        app_slot = Some(app.clone());
        gpui_kit::base::Root::new(app, window, cx)
    });
    let app = app_slot.expect("测试窗口应创建应用视图");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.click("new-folder", cx);
        window.draw(cx).clear(cx);
        assert!(
            window.find("new-folder-name").visible(),
            "应弹出新建文件夹对话框"
        );
        app.update(cx, |app, cx| {
            crate::app::util::set_input(&app.new_folder_name, "notes", window, cx);
        });
        window.click("new-folder-create", cx);
        window.draw(cx).clear(cx);
    });
    assert!(
        directory.path().join("notes").is_dir(),
        "应创建 notes 文件夹"
    );
    assert!(
        cx.update(|window, _| window.try_find("new-folder-create").is_none()),
        "创建后对话框应关闭"
    );
    cx.update(|window, cx| {
        window.click("new-folder", cx);
        window.draw(cx).clear(cx);
        app.update(cx, |app, cx| {
            crate::app::util::set_input(&app.new_folder_name, "canceled", window, cx);
        });
        window.click("new-folder-cancel", cx);
        window.draw(cx).clear(cx);
    });
    assert!(
        !directory.path().join("canceled").is_dir(),
        "取消不应创建文件夹"
    );
    let _ = app;
}

/// 重命名对话框会重命名所选文件。
#[gpui_kit::test]
fn rename_dialog_renames_selected_file(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("old.txt"), b"old").unwrap();
    let mut app_slot = None;
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let app = cx.new(|cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        app_slot = Some(app.clone());
        gpui_kit::base::Root::new(app, window, cx)
    });
    let app = app_slot.expect("测试窗口应创建应用视图");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let entry = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == "old.txt")
                .expect("应包含 old.txt");
            app.browser.selection.select_only(&entry.id);
            app.start_rename(window, cx);
        });
        window.draw(cx).clear(cx);
        assert!(window.find("rename-name").visible(), "应弹出重命名对话框");
        app.update(cx, |app, cx| {
            crate::app::util::set_input(&app.rename_name, "new.txt", window, cx);
        });
        window.click("rename-confirm", cx);
        window.draw(cx).clear(cx);
    });
    assert!(
        directory.path().join("new.txt").is_file(),
        "应重命名为 new.txt"
    );
    assert!(!directory.path().join("old.txt").exists(), "旧文件应消失");
    let _ = app;
}

/// 删除确认：取消保留文件，确认后删除所选文件。
#[gpui_kit::test]
fn delete_confirmation_cancels_and_removes_file(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("trash.txt");
    std::fs::write(&file, b"trash").unwrap();
    let mut app_slot = None;
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let app = cx.new(|cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        app_slot = Some(app.clone());
        gpui_kit::base::Root::new(app, window, cx)
    });
    let app = app_slot.expect("测试窗口应创建应用视图");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let entry = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == "trash.txt")
                .expect("应包含 trash.txt");
            app.browser.selection.select_only(&entry.id);
            app.request_delete(window, cx);
        });
        window.draw(cx).clear(cx);
        assert!(window.find("cancel").visible(), "应弹出删除确认框");
        window.click("cancel", cx);
        window.draw(cx).clear(cx);
    });
    assert!(file.is_file(), "取消确认不应删除文件");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.request_delete(window, cx);
        });
        window.draw(cx).clear(cx);
        assert!(window.find("ok").visible());
        window.click("ok", cx);
        window.draw(cx).clear(cx);
    });
    assert!(!file.exists(), "确认后应删除文件");
    let _ = app;
}
