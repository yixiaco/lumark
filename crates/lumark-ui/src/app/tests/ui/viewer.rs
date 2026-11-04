//! 文本查看器和归档添加弹窗测试。

use super::*;

/// 验证模型查看器的画布、工具栏和按钮尺寸在窗口放大后仍然正确：
/// 画布必须位于工具栏下方（最大化时也不能盖住按钮），且工具栏高度符合
/// 标准按钮尺寸（32px + 上下内边距）。
#[gpui_kit::test]
fn model_viewer_layout_stays_below_toolbar_when_resized(cx: &mut gpui_kit::TestAppContext) {
    use crate::app::view::asset_viewer_window::{AssetViewerWindowView, ViewerScene};
    cx.update(gpui_kit::init);
    let mesh = lumark_unreal::MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![lumark_unreal::MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let (view, cx) = cx.add_window_view(|_window, _cx| {
        AssetViewerWindowView::new(
            "Mesh".to_string(),
            ViewerScene::Mesh {
                lods: vec![mesh],
                materials: std::sync::Arc::new(Vec::new()),
            },
        )
    });
    let _ = view;
    cx.simulate_resize(gpui_kit::size(px(1920.), px(1080.)));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let toolbar = cx
        .debug_bounds("model-viewer-toolbar")
        .expect("工具栏应记录布局 bounds");
    let canvas = cx
        .debug_bounds("model-viewer-canvas")
        .expect("画布应记录布局 bounds");
    assert!(
        canvas.origin.y >= toolbar.origin.y + toolbar.size.height - px(1.0),
        "画布（top={:?}）不应覆盖工具栏（bottom={:?}），窗口放大后尤其不能遮挡按钮",
        canvas.origin.y,
        toolbar.origin.y + toolbar.size.height
    );
    assert!(
        toolbar.size.height >= px(44.),
        "工具栏高度 {:?} 过小：线框/sRGB/S0/S1/LOD 按钮应使用标准尺寸",
        toolbar.size.height
    );
}

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

/// 复制按钮分别把文本正文和十六进制转储写入剪贴板。
#[gpui_kit::test]
fn text_viewer_copy_button_writes_clipboard(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("note.txt");
    std::fs::write(&file, "hello clipboard").unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        crate::app::view::text_viewer_window::TextViewerWindowView::new(
            file.clone(),
            "note.txt".to_string(),
            false,
            None,
            window,
            cx,
        )
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("viewer-copy").visible());
        window.click("viewer-copy", cx);
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        assert_eq!(text, "hello clipboard", "文本复制内容不正确：{text:?}");

        window.click("viewer-mode-hex", cx);
        window.draw(cx).clear(cx);
        window.click("viewer-copy", cx);
        let hex = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        assert!(hex.contains("00000000"), "十六进制复制应带偏移：{hex:?}");
        assert!(
            hex.contains("68 65 6c 6c 6f"),
            "十六进制复制应带字节：{hex:?}"
        );
        assert!(
            hex.contains("hello clipboard"),
            "十六进制复制应带 ASCII：{hex:?}"
        );
    });
    let _ = view;
}

/// 生成带 EXIF 方向的最小 JPEG。
fn exif_rotated_jpeg(width: u32, height: u32, orientation: u16) -> Vec<u8> {
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 40) as u8, (y * 80) as u8, 100]);
    }
    let mut jpeg = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 90)
        .encode_image(&image)
        .expect("编码 JPEG");
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    let mut output = vec![0xFF, 0xD8];
    output.extend_from_slice(&[0xFF, 0xE1]);
    output.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    output.extend_from_slice(&payload);
    output.extend_from_slice(&jpeg[2..]);
    output
}

/// 带 EXIF 方向的图片仍能在查看器中打开并显示工具栏。
#[gpui_kit::test]
fn image_viewer_opens_exif_rotated_image(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("rotated.jpg");
    std::fs::write(&file, exif_rotated_jpeg(2, 3, 6)).unwrap();
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
            app.open_image_viewer(file.clone(), "rotated.jpg".to_string(), window, cx);
        });
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "应打开图片查看器窗口");
    let viewer = *windows.last().unwrap();
    cx.run_until_parked();
    cx.update_window(viewer, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("image-fit").visible(), "图片查看器应渲染工具栏");
    })
    .unwrap();
    let _ = app;
}

/// 图片查看器可以手动旋转并在重置后恢复。
#[gpui_kit::test]
fn image_viewer_rotates_and_resets(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let file = directory.path().join("rotated.png");
    let mut image = image::RgbaImage::new(2, 3);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x * 100) as u8, (y * 60) as u8, 40, 255]);
    }
    image.save(&file).unwrap();
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
            app.open_image_viewer(file.clone(), "rotated.png".to_string(), window, cx);
        });
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "应打开图片查看器窗口");
    let viewer = *windows.last().unwrap();
    cx.run_until_parked();
    cx.update_window(viewer, |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("image-rotate-right", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(viewer, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.find("image-rotation").visible(),
            "旋转后状态栏应显示旋转角度"
        );
        window.click("image-reset", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(viewer, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.try_find("image-rotation").is_none(),
            "重置后应恢复未旋转状态"
        );
    })
    .unwrap();
    let _ = app;
}

/// 材质预览应打开独立的材质查看器（可拖拽旋转），而不是静态图片查看器。
#[gpui_kit::test]
fn material_viewer_opens_dedicated_window(cx: &mut gpui_kit::TestAppContext) {
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
    let viewer = lumark_unreal::MaterialViewerData {
        appearance: lumark_unreal::MaterialAppearance::default(),
        base_color: None,
        normal: None,
        metallic_roughness: None,
        occlusion: None,
        ..Default::default()
    };
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_asset_viewer(
                crate::app::view::ViewerScene::Material { viewer },
                "MAT_Teapot".to_string(),
                window,
                cx,
            );
        });
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "应打开独立的材质查看器窗口");
    let handle = *windows.last().unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.find("model-reset").visible() && window.find("model-zoom-in").visible(),
            "材质查看器应渲染交互式查看器工具栏"
        );
        assert!(
            window.try_find("image-fit").is_none(),
            "材质不应打开静态图片查看器"
        );
    })
    .unwrap();
    let _ = app;
}
