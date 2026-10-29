//! 文本查看器和归档添加弹窗测试。

use super::*;

/// 验证内置文本查看器可以渲染只读编辑器和状态行。
#[gpui_kit::test]
fn text_viewer_renders_readonly_editor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("note.txt");
    std::fs::write(&file, "第一行\n第二行\n").unwrap();
    let (_view, cx) = cx.add_window_view(|window, cx| {
        crate::app::view::text_viewer_window::TextViewerWindowView::new(
            file.clone(),
            "note.txt".to_string(),
            false,
            None,
            window,
            cx,
        )
    });
    let _ = _view;
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
            app.open_text_viewer(file.clone(), "note.txt".to_string(), true, None, window, cx);
            assert!(
                app.status.contains("已在内置查看器中打开"),
                "{}",
                app.status
            );
        });
    });
    let _ = app;
}

/// 构造带一个 `.text` 节的最小 PE32，供查看器测试使用。
fn minimal_pe() -> Vec<u8> {
    let mut bytes = vec![0u8; 0x400];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    let coff = 0x84;
    bytes[coff..coff + 2].copy_from_slice(&0x014cu16.to_le_bytes());
    bytes[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
    bytes[coff + 16..coff + 18].copy_from_slice(&0xe0u16.to_le_bytes());
    let optional = coff + 20;
    bytes[optional..optional + 2].copy_from_slice(&0x10bu16.to_le_bytes());
    bytes[optional + 16..optional + 20].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[optional + 28..optional + 32].copy_from_slice(&0x400000u32.to_le_bytes());
    bytes[optional + 68..optional + 70].copy_from_slice(&3u16.to_le_bytes());
    let section = optional + 0xe0;
    bytes[section..section + 5].copy_from_slice(b".text");
    bytes[section + 8..section + 12].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[section + 16..section + 20].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
    bytes
}

/// 验证 exe 内容会被识别为 PE 并显示 PE 结构入口。
#[gpui_kit::test]
fn text_viewer_opens_pe_file_in_pe_mode(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, minimal_pe()).unwrap();
    let (_view, cx) = cx.add_window_view(|window, cx| {
        crate::app::view::text_viewer_window::TextViewerWindowView::new(
            file.clone(),
            "sample.exe".to_string(),
            false,
            None,
            window,
            cx,
        )
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.find("viewer-mode-pe").visible(),
            "PE 文件应显示 PE 结构入口"
        );
        assert!(
            window.find("viewer-pe-sections").visible(),
            "PE 文件初始应显示节表，而不是十六进制"
        );
    });
    let _ = _view;
}

/// 验证文件表中双击 exe 会进入 PE 只读归档，而不是交给系统运行。
#[gpui_kit::test]
fn browser_opens_pe_as_archive(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, minimal_pe()).unwrap();
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
            let entry = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == "sample.exe")
                .expect("文件表应包含 sample.exe");
            app.browser.selection.select_only(&entry.id);
            app.open_selected(window, cx);
            assert!(
                app.status.contains("正在读取归档目录"),
                "selection should start opening PE archive, status: {}",
                app.status
            );
        });
    });
    let _ = app;
}

/// 验证 PE 归档的节和 `.rsrc/ICON` 图标能在文件表中渲染和导航。
#[gpui_kit::test]
fn pe_archive_renders_sections_and_icons(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, minimal_pe()).unwrap();
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
        app.update(cx, |app, _| {
            app.browser.open_archive(&file, None).unwrap();
            let section = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == ".text")
                .expect("PE 根目录应包含 .text");
            app.browser.enter_entry(&section.id).unwrap();
            assert!(
                app.displayed_entries()
                    .iter()
                    .any(|entry| entry.name == "[0]"),
                "节目录应包含原始数据条目"
            );
        });
        window.draw(cx).clear(cx);
    });
    let _ = app;
}

/// 验证资源管理器转发 `--open exe` 时打开 PE 只读归档。
#[gpui_kit::test]
fn startup_open_pe_opens_archive(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, minimal_pe()).unwrap();
    let request = StartupRequest::parse(vec![
        "--open".to_string(),
        file.to_string_lossy().into_owned(),
    ])
    .expect("PE open request");
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
            app.apply_startup_request(request, window, cx);
            assert!(
                app.status.contains("正在读取归档目录"),
                "startup should start opening PE archive, status: {}",
                app.status
            );
        });
    });
    let _ = app;
}

/// 验证图片文件走独立图片查看器，而不是文本十六进制视图。
#[gpui_kit::test]
fn image_viewer_opens_for_png(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("photo.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 120, 220, 255]))
        .save(&file)
        .unwrap();
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
            app.open_image_viewer(file.clone(), "photo.png".to_string(), window, cx);
            assert!(
                app.status.contains("已在内置图片查看器中打开"),
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
            app.add_files_to_current_archive(&[dropped, dropped_folder], window, cx);
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
        window.draw(cx).clear(cx);
        app.update(cx, |app, _| {
            assert!(app.pending_archive_additions.is_empty());
            assert!(app.pending_archive_roots.is_empty());
        });
        assert!(
            window.try_find("archive-add-confirm").is_none(),
            "确认后官方添加对话框应关闭"
        );
    });
    let _ = app;
}
