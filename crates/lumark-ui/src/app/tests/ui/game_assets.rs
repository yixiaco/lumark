//! 游戏资产工作区和详情图片预览的真实窗口测试。

use super::*;

/// 游戏资产入口只在 Windows 编译时展示。
#[test]
fn game_assets_gate_matches_platform() {
    assert_eq!(GAME_ASSETS_SUPPORTED, cfg!(target_os = "windows"));
}

/// 验证扫描并保存未加密游戏后进入归档列表，容器打开动作已排队。
#[cfg(windows)]
#[gpui_kit::test]
fn saved_game_shows_archive_list(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = directory.path().join("TogetherBNB");
    let paks = project.join("Content/Paks");
    std::fs::create_dir_all(&paks).unwrap();
    // 扩展名为 `.pak` 的 ZIP 用于在没有真实 UE 打包器时覆盖 UI 路由。
    let payload = directory.path().join("readme.txt");
    std::fs::write(&payload, b"hello game").unwrap();
    lumark_archive::ArchiveService::create(
        &paks.join("pakchunk0-Windows.pak"),
        &[payload],
        &lumark_archive::CompressionOptions {
            format: lumark_archive::ArchiveFormat::Zip,
            path_mode: lumark_archive::PathMode::NoRoot,
            ..Default::default()
        },
        |_| {},
    )
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
            app.open_game_assets(window, cx);
            assert!(app.game_library, "游戏资产入口应显示游戏库列表");
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(project.clone(), window, cx);
            app.save_game_setup(window, cx);
            assert!(app.game_session.is_some(), "保存后应进入游戏会话");
            assert!(!app.game_library, "进入游戏后应离开游戏库列表");
            assert!(
                matches!(
                    app.browser.location,
                    lumark_browser::BrowserLocation::GameArchives { .. }
                ),
                "应进入归档列表"
            );
            assert_eq!(app.displayed_count(), 1);
            let entry = app
                .displayed_entries()
                .into_iter()
                .next()
                .expect("应有容器行");
            assert!(entry.name.ends_with(".pak"));
            app.open_game_container(entry, window, cx);
            assert!(
                app.status.contains("正在读取归档目录"),
                "应排队打开容器，实际：{}",
                app.status
            );
        });
        window.draw(cx).clear(cx);
        assert!(window.find("game-sidebar-edit").visible());
        assert!(window.find("game-sidebar-list").visible());
        assert!(window.find("game-context-exit").visible());
    });
    let _ = app;
}

/// 验证动作带入口打开游戏库列表，并渲染新增游戏和返回文件浏览按钮。
#[cfg(windows)]
#[gpui_kit::test]
fn game_assets_button_opens_library(cx: &mut gpui_kit::TestAppContext) {
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
        window.draw(cx).clear(cx);
        assert!(
            window.find("game-assets").visible(),
            "动作带应显示游戏资产按钮"
        );
        app.update(cx, |app, cx| {
            app.open_game_assets(window, cx);
            assert!(app.game_library, "应打开游戏库列表页");
            assert!(app.game_setup.is_none(), "游戏库列表不应直接打开设置表单");
        });
        window.draw(cx).clear(cx);
        assert!(window.find("game-library-add").visible());
        assert!(window.find("game-library-back").visible());
    });
    let _ = app;
}

/// 创建用于扫描的最小 UE 项目目录。
#[cfg(windows)]
fn fake_game_project(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let project = root.join(name);
    let paks = project.join("Content/Paks");
    std::fs::create_dir_all(&paks).unwrap();
    std::fs::write(paks.join("pakchunk0-Windows.utoc"), b"not a real utoc").unwrap();
    project
}

/// 两个游戏可以同时保持打开，切换时各自浏览位置独立。
#[cfg(windows)]
#[gpui_kit::test]
fn multiple_games_stay_open(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let first = fake_game_project(directory.path(), "GameOne");
    let second = fake_game_project(directory.path(), "GameTwo");
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            directory.path().join("vault.dat"),
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    let mut tab_ids = Vec::new();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_game_assets(window, cx);
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(first.clone(), window, cx);
            app.save_game_setup(window, cx);
            assert_eq!(app.open_game_entries().len(), 1);

            app.start_game_setup(None, window, cx);
            app.scan_game_directory(second.clone(), window, cx);
            app.save_game_setup(window, cx);
            assert_eq!(app.open_game_entries().len(), 2);
            assert_eq!(
                app.settings.get().open_games.len(),
                2,
                "打开的游戏列表应持久化到设置"
            );
            tab_ids = app
                .open_game_entries()
                .into_iter()
                .map(|(id, _, _)| id)
                .collect();
            let entries = app.open_game_entries();
            assert!(entries[0].2, "第二个游戏应处于前台");
            assert!(!entries[1].2, "第一个游戏应保留在后台");

            let first_id = entries[1].0.clone();
            app.switch_game_session(&first_id, window, cx);
            assert_eq!(
                app.game_session
                    .as_ref()
                    .map(|session| session.display_name.as_str()),
                Some("GameOne")
            );
            assert!(
                app.open_game_entries()
                    .iter()
                    .any(|(_, name, active)| name == "GameTwo" && !active)
            );

            let second_id = app
                .open_game_entries()
                .into_iter()
                .find(|(_, name, _)| name == "GameTwo")
                .map(|(id, _, _)| id)
                .expect("第二个游戏");
            app.switch_game_session(&second_id, window, cx);
            assert_eq!(
                app.game_session
                    .as_ref()
                    .map(|session| session.display_name.as_str()),
                Some("GameTwo")
            );
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("game-sidebar-edit").visible(),
            "侧栏应显示当前游戏卡片"
        );
        for game_id in &tab_ids {
            let _ = game_id;
        }
        app.update(cx, |app, cx| {
            // 关闭当前游戏后应自动切回仍打开的另一个游戏。
            app.close_current_game(window, cx);
            assert_eq!(app.open_game_entries().len(), 1);
            assert_eq!(
                app.game_session
                    .as_ref()
                    .map(|session| session.display_name.as_str()),
                Some("GameOne")
            );
        });
    });
    let _ = app;
}

/// 验证扫描游戏目录后设置页显示识别结果，且保存按钮可用。
#[cfg(windows)]
#[gpui_kit::test]
fn game_setup_scan_fills_detection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = directory.path().join("TogetherBNB");
    let paks = project.join("Content/Paks");
    std::fs::create_dir_all(&paks).unwrap();
    std::fs::write(paks.join("pakchunk0-Windows.utoc"), b"not a real utoc").unwrap();
    std::fs::create_dir_all(project.join("Mappings")).unwrap();
    std::fs::write(project.join("Mappings/Game.usmap"), b"not a real mapping").unwrap();
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
            app.open_game_assets(window, cx);
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(project.clone(), window, cx);
            let setup = app.game_setup.as_ref().expect("设置页");
            assert!(setup.scanned.is_some(), "应完成目录扫描");
            assert_eq!(setup.engine(), lumark_unreal::UnrealEngine::Unknown);
            assert!(setup.mappings.is_some(), "应自动发现映射文件");
            assert!(setup.ready(), "扫描完成后应可保存");
            // 设置表单只由独立编辑窗口渲染；测试窗口模拟编辑窗口的渲染分支。
            app.game_editor_only = true;
            app.game_library = false;
        });
        window.draw(cx).clear(cx);
        assert!(window.find("game-setup-save").visible());
    });
    let _ = app;
}

/// 验证文件系统图片在详情面板显示预览。
#[cfg(windows)]
#[gpui_kit::test]
fn details_preview_shows_filesystem_image(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 120, 220, 255]))
        .save(directory.path().join("photo.png"))
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
    // 等待初次目录列表加载完成，避免绘制时刷新目录清掉选择。
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let entry = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == "photo.png")
                .expect("应包含 photo.png");
            app.browser.selection.select_only(&entry.id);
            app.sync_details_preview(cx);
            assert!(matches!(
                app.details_preview,
                Some(crate::app::view::DetailsPreview::Ready { .. })
            ));
        });
        window.draw(cx).clear(cx);
        app.update(cx, |app, _| {
            assert!(app.selected_entry().is_some(), "选择应保持");
            assert!(matches!(
                app.details_preview,
                Some(crate::app::view::DetailsPreview::Ready { .. })
            ));
        });
    });
    let _ = app;
}

/// 验证普通 ZIP 内图片可以解压到预览缓存并显示。
#[cfg(windows)]
#[gpui_kit::test]
fn details_preview_extracts_archive_image(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let image_path = directory.path().join("inside.png");
    image::RgbaImage::from_pixel(3, 3, image::Rgba([10, 20, 30, 255]))
        .save(&image_path)
        .unwrap();
    let archive = directory.path().join("images.zip");
    lumark_archive::ArchiveService::create(
        &archive,
        &[image_path],
        &lumark_archive::CompressionOptions {
            format: lumark_archive::ArchiveFormat::Zip,
            path_mode: lumark_archive::PathMode::NoRoot,
            ..Default::default()
        },
        |_| {},
    )
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
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.browser.open_archive(&archive, None).unwrap();
            let entry = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == "inside.png")
                .expect("归档应包含 inside.png");
            app.browser.selection.select_only(&entry.id);
            app.sync_details_preview(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.update(cx, |app, _| {
            assert!(
                matches!(
                    app.details_preview,
                    Some(crate::app::view::DetailsPreview::Ready { .. })
                ),
                "归档图片应完成解压预览"
            );
        });
        window.draw(cx).clear(cx);
        app.update(cx, |app, _| {
            assert!(app.selected_entry().is_some(), "选择应保持");
            assert!(matches!(
                app.details_preview,
                Some(crate::app::view::DetailsPreview::Ready { .. })
            ));
        });
    });
    let _ = app;
}

/// 游戏库列出已保存游戏，打开行可重新进入，返回按钮恢复文件浏览。
#[cfg(windows)]
#[gpui_kit::test]
fn game_library_lists_saved_games_and_reopens(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = directory.path().join("TogetherBNB");
    let paks = project.join("Content/Paks");
    std::fs::create_dir_all(&paks).unwrap();
    let payload = directory.path().join("readme.txt");
    std::fs::write(&payload, b"hello game").unwrap();
    lumark_archive::ArchiveService::create(
        &paks.join("pakchunk0-Windows.pak"),
        &[payload],
        &lumark_archive::CompressionOptions {
            format: lumark_archive::ArchiveFormat::Zip,
            path_mode: lumark_archive::PathMode::NoRoot,
            ..Default::default()
        },
        |_| {},
    )
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
            app.open_game_assets(window, cx);
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(project.clone(), window, cx);
            app.save_game_setup(window, cx);
            assert!(app.game_session.is_some(), "保存后应进入游戏会话");
            app.back_to_game_library(window, cx);
            assert!(app.game_library, "应返回游戏库列表");
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("game-library-add").visible(),
            "游戏库列表页应渲染新增游戏按钮"
        );
        assert!(
            window.find(("game-open", 0usize)).visible(),
            "游戏库应列出已保存游戏的打开按钮"
        );
        assert!(window.find(("game-edit", 0usize)).visible());
        assert!(window.find(("game-remove", 0usize)).visible());

        window.click(("game-open", 0usize), cx);
        app.update(cx, |app, _| {
            assert!(app.game_session.is_some(), "打开已保存游戏应恢复会话");
            assert!(!app.game_library, "打开游戏后应离开游戏库列表");
            assert!(
                matches!(
                    app.browser.location,
                    lumark_browser::BrowserLocation::GameArchives { .. }
                ),
                "应回到游戏归档列表：{:?}",
                app.browser.location
            );
        });

        app.update(cx, |app, cx| app.back_to_game_library(window, cx));
        window.draw(cx).clear(cx);
        window.click("game-library-back", cx);
        app.update(cx, |app, _| {
            assert!(app.game_session.is_none(), "返回文件浏览应关闭游戏会话");
            assert!(!app.game_library);
            assert!(
                matches!(
                    app.browser.location,
                    lumark_browser::BrowserLocation::FileSystem(_)
                ),
                "应恢复进入游戏前的文件系统位置：{:?}",
                app.browser.location
            );
        });
    });
    let _ = app;
}

/// 新增游戏会打开独立编辑窗口，重复点击复用已有窗口。
#[cfg(windows)]
#[gpui_kit::test]
fn new_game_editor_opens_separate_window(cx: &mut gpui_kit::TestAppContext) {
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
            app.open_game_assets(window, cx);
            app.open_game_setup_new(window, cx);
        });
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "新增游戏应打开独立编辑窗口");
    let editor = *windows.last().unwrap();
    cx.update_window(editor, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.find("game-editor-close").visible(),
            "编辑窗口应渲染关闭按钮"
        );
        assert!(
            window.find("game-setup-save").visible(),
            "编辑窗口应渲染保存按钮"
        );
    })
    .unwrap();
    // 再次点击新增应复用已有编辑窗口，而不是继续创建。
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_game_setup_new(window, cx);
        });
    });
    assert_eq!(cx.windows().len(), 2, "重复点击新增不应再开窗口");
    let _ = app;
}

/// 编辑窗口扫描游戏目录后可保存并自行关闭。
#[cfg(windows)]
#[gpui_kit::test]
fn game_editor_window_saves_game_and_closes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = directory.path().join("TogetherBNB");
    let paks = project.join("Content/Paks");
    std::fs::create_dir_all(&paks).unwrap();
    std::fs::write(paks.join("pakchunk0-Windows.utoc"), b"stub").unwrap();
    std::fs::create_dir_all(project.join("Mappings")).unwrap();
    std::fs::write(project.join("Mappings/Game.usmap"), b"stub").unwrap();
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
            app.open_game_assets(window, cx);
            app.open_game_setup_new(window, cx);
        });
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "新增游戏应打开独立编辑窗口");
    let editor = *windows.last().unwrap();
    cx.update_window(editor, |view, window, cx| {
        let root = view
            .downcast::<gpui_kit::component::Root>()
            .expect("编辑窗口 Root");
        let editor_view = root
            .read(cx)
            .view()
            .clone()
            .downcast::<crate::app::LumarkApp>()
            .expect("编辑窗口视图");
        editor_view.update(cx, |editor, cx| {
            editor.scan_game_directory(project.clone(), window, cx);
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("game-setup-save").visible(),
            "扫描完成后编辑窗口应可保存"
        );
        window.click("game-setup-save", cx);
    })
    .unwrap();
    assert_eq!(cx.windows().len(), 1, "保存后编辑窗口应关闭");
    app.update(cx, |app, _| {
        assert_eq!(app.settings.get().games.len(), 1, "保存后游戏库应新增记录");
        assert_eq!(app.settings.get().games[0].name, "TogetherBNB");
    });
    let _ = app;
}

/// 侧栏“编辑”按钮会打开当前游戏的独立编辑窗口。
#[cfg(windows)]
#[gpui_kit::test]
fn game_sidebar_edit_opens_editor_window(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = fake_game_project(directory.path(), "TogetherBNB");
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
            app.open_game_assets(window, cx);
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(project.clone(), window, cx);
            app.save_game_setup(window, cx);
        });
        window.draw(cx).clear(cx);
        window.click("game-sidebar-edit", cx);
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "侧栏编辑应打开独立编辑窗口");
    let editor = *windows.last().unwrap();
    cx.update_window(editor, |view, window, cx| {
        let root = view
            .downcast::<gpui_kit::component::Root>()
            .expect("编辑窗口 Root");
        let editor_view = root
            .read(cx)
            .view()
            .clone()
            .downcast::<crate::app::LumarkApp>()
            .expect("编辑窗口视图");
        editor_view.update(cx, |editor, _| {
            assert!(editor.game_editor_only, "窗口应使用仅编辑模式");
            assert!(editor.game_setup.is_some(), "编辑窗口应打开设置表单");
        });
        window.draw(cx).clear(cx);
        assert!(window.find("game-setup-save").visible());
    })
    .unwrap();
    let _ = app;
}

/// 编辑窗口关闭按钮会关闭窗口且不保存游戏。
#[cfg(windows)]
#[gpui_kit::test]
fn game_editor_close_button_discards_game(cx: &mut gpui_kit::TestAppContext) {
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
            app.open_game_assets(window, cx);
            app.open_game_setup_new(window, cx);
        });
    });
    let windows = cx.windows();
    assert_eq!(windows.len(), 2, "新增游戏应打开独立编辑窗口");
    let editor = *windows.last().unwrap();
    cx.update_window(editor, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("game-editor-close").visible());
        window.click("game-editor-close", cx);
    })
    .unwrap();
    assert_eq!(cx.windows().len(), 1, "关闭编辑窗口后应只剩主窗口");
    app.update(cx, |app, _| {
        assert!(
            app.settings.get().games.is_empty(),
            "取消编辑不应写入游戏记录"
        );
    });
    let _ = app;
}

/// 移除游戏先弹出确认框：取消保留记录，确认后删除并关闭当前会话。
#[cfg(windows)]
#[gpui_kit::test]
fn game_library_remove_requires_confirmation(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = fake_game_project(directory.path(), "TogetherBNB");
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
            app.open_game_assets(window, cx);
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(project.clone(), window, cx);
            app.save_game_setup(window, cx);
            app.back_to_game_library(window, cx);
        });
        window.draw(cx).clear(cx);
        window.click(("game-remove", 0usize), cx);
        window.draw(cx).clear(cx);
    });
    cx.update(|window, cx| {
        assert!(window.find("cancel").visible(), "移除游戏应弹出确认框");
        window.click("cancel", cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert_eq!(app.settings.get().games.len(), 1, "取消确认不应移除游戏");
    });
    cx.update(|window, cx| {
        window.click(("game-remove", 0usize), cx);
        window.draw(cx).clear(cx);
        assert!(window.find("ok").visible());
        window.click("ok", cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert!(app.settings.get().games.is_empty(), "确认后应移除游戏记录");
        assert!(app.game_session.is_none(), "移除当前游戏应关闭会话");
        assert!(app.game_library, "移除后仍停留在游戏库列表");
    });
    let _ = app;
}

/// 预览已缓存时再次同步预览不应丢失查看器场景数据（否则双击会退回图片查看器）。
#[gpui_kit::test]
fn cached_preview_keeps_viewer_scene_data(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
        .save(directory.path().join("photo.png"))
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
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            let entry = app
                .displayed_entries()
                .into_iter()
                .find(|entry| entry.name == "photo.png")
                .expect("应包含 photo.png");
            app.browser.selection.select_only(&entry.id);
            app.sync_details_preview(cx);
            assert!(matches!(
                app.details_preview,
                Some(crate::app::view::DetailsPreview::Ready { .. })
            ));
            app.details_material = Some(lumark_unreal::MaterialViewerData {
                appearance: lumark_unreal::MaterialAppearance::default(),
                base_color: None,
                normal: None,
                metallic_roughness: None,
                occlusion: None,
                ..Default::default()
            });
            // 模拟渲染期间的再次同步：预览 key 未变，查看器数据应保留。
            app.sync_details_preview(cx);
            assert!(
                app.details_material.is_some(),
                "缓存预览不应丢失材质查看器数据"
            );
        });
    });
    let _ = app;
}

/// 侧栏归档点击应切换容器，而不是只在当前目录条目里查找。
#[cfg(windows)]
#[gpui_kit::test]
fn sidebar_archive_click_switches_container(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let project = directory.path().join("TogetherBNB");
    let paks = project.join("Content/Paks");
    std::fs::create_dir_all(&paks).unwrap();
    let payload = directory.path().join("readme.txt");
    std::fs::write(&payload, b"hello game").unwrap();
    for name in ["pakchunk0-Windows.pak", "pakchunk1-Windows.pak"] {
        lumark_archive::ArchiveService::create(
            &paks.join(name),
            std::slice::from_ref(&payload),
            &lumark_archive::CompressionOptions {
                format: lumark_archive::ArchiveFormat::Zip,
                path_mode: lumark_archive::PathMode::NoRoot,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    }
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
            app.open_game_assets(window, cx);
            app.start_game_setup(None, window, cx);
            app.scan_game_directory(project.clone(), window, cx);
            app.save_game_setup(window, cx);
            assert!(app.game_session.is_some());
        });
        let containers: Vec<std::path::PathBuf> = app
            .read(cx)
            .game_session
            .as_ref()
            .expect("游戏会话")
            .game
            .containers
            .iter()
            .map(|container| container.path.clone())
            .collect();
        assert_eq!(containers.len(), 2, "应扫描出两个容器");
        // 模拟已进入第一个容器的子目录；此时 browser.entries 不含归档行。
        app.update(cx, |app, cx| {
            app.browser.location = lumark_browser::BrowserLocation::Archive {
                archive: containers[0].clone(),
                inner: "Sub".to_string(),
            };
            app.open_game_container_path(containers[1].clone(), window, cx);
            assert!(
                app.status.contains("正在读取归档目录"),
                "点击另一个归档应切换加载：{}",
                app.status
            );
            assert_eq!(
                app.game_session
                    .as_ref()
                    .and_then(|session| session.last_container.clone()),
                Some(containers[1].clone()),
                "应记录切换后的容器"
            );
        });
        // 已在目标容器根目录时保持现状，不重复加载。
        app.update(cx, |app, cx| {
            app.browser.location = lumark_browser::BrowserLocation::Archive {
                archive: containers[1].clone(),
                inner: String::new(),
            };
            app.open_game_container_path(containers[1].clone(), window, cx);
            assert!(
                app.status.contains("当前归档"),
                "同容器根目录应提示当前归档：{}",
                app.status
            );
        });
    });
    let _ = app;
}

/// 拖动侧栏分栏后宽度写入设置，主界面使用新宽度。
#[gpui_kit::test]
fn sidebar_width_resize_persists(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("readme.txt"), b"lumark").unwrap();
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
        window.draw(cx).clear(cx);
        app.update(cx, |app, cx| {
            let before = app.sidebar_width;
            let state = app.sidebar_state.clone();
            state.update(cx, |state, cx| {
                state.resize_panel(0, gpui_kit::px((before + 40.0).min(460.0)), window, cx);
            });
        });
    });
    // ResizableState 的 Resized 事件在本次更新结束后派发，因此断言放在外层，
    // 与真实界面“拖动结束 → 写设置”的时序一致。
    app.update(cx, |app, _| {
        assert!(
            app.sidebar_width > 250.5,
            "拖动分栏应更新侧栏宽度：{}",
            app.sidebar_width
        );
        assert!(
            (app.settings.get().sidebar_width - app.sidebar_width).abs() < 0.5,
            "侧栏宽度应写入设置：{} vs {}",
            app.settings.get().sidebar_width,
            app.sidebar_width
        );
    });
    let _ = app;
}
