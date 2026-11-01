//! 文件表、父行和密码列表滚动测试。

use super::*;

/// 验证虚拟文件行和平铺卡片都暴露预期的测试目标。
#[gpui_kit::test]
fn browser_renders_virtual_rows_and_tile_view(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    for index in 0..80 {
        std::fs::write(
            directory.path().join(format!("file-{index:03}.txt")),
            b"lumark",
        )
        .unwrap();
    }
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let settings_path = directory.path().join("settings.json");
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            settings_path,
            Some(directory.path().to_path_buf()),
        )
    });
    let first_row = format!(
        "file-row-{}",
        directory.path().join("file-000.txt").display()
    );
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find(first_row.clone()).visible());
        window.click("view-toggle", cx);
        window.draw(cx).clear(cx);
        let first_tile = format!(
            "file-tile-name-{}",
            directory.path().join("file-000.txt").display()
        );
        assert!(window.find(first_tile).visible());
    });
    let _ = app;
}

/// 验证文件表最上方的“..”行能返回上一级，并在盘符根目录隐藏。
#[gpui_kit::test]
fn parent_row_navigates_up_and_hides_at_root(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("readme.txt"), b"lumark").unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    let parent = directory.path().parent().unwrap().to_path_buf();
    let root = directory.path().ancestors().last().unwrap().to_path_buf();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("parent-entry-row").visible());
        window.click("parent-entry-row", cx);
        assert_eq!(
            app.read(cx).browser.location,
            BrowserLocation::FileSystem(parent.clone())
        );

        window.click("view-toggle", cx);
        window.draw(cx).clear(cx);
        assert!(window.find("parent-entry-tile").visible());
        window.click("parent-entry-tile", cx);
        assert_eq!(
            app.read(cx).browser.location,
            BrowserLocation::FileSystem(parent.parent().unwrap().to_path_buf())
        );

        window.click("view-toggle", cx);
        app.update(cx, |app, cx| {
            app.browser.navigate(&root).unwrap();
            app.search_results = None;
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(window.try_find("parent-entry-row").is_none());
        assert!(window.try_find("parent-entry-tile").is_none());
    });
    let _ = app;
}

/// 验证密码列表先消费滚轮，外层页面不会抢先滚动。
#[gpui_kit::test]
fn password_list_scrolls_before_page(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.vault.unlock("master-pass").unwrap();
            for index in 0..20 {
                app.vault
                    .add_entry(PasswordEntry::new(
                        format!("记录 {index}"),
                        format!("pass-{index}"),
                    ))
                    .unwrap();
            }
            app.screen = Screen::Passwords;
            cx.notify();
        });
        window.draw(cx).clear(cx);
        window.scroll(
            "password-list",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-200.))),
            cx,
        );
        window.draw(cx).clear(cx);
        let (list_offset, page_offset, page_max) = {
            let app = app.read(cx);
            (
                app.password_list_scroll.offset().y,
                app.password_page_scroll.offset().y,
                app.password_page_scroll.max_offset().y,
            )
        };
        assert!(list_offset < px(0.));
        assert_eq!(page_offset, px(0.));
        if page_max < px(0.) {
            window.scroll(
                "lock-vault",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-60.))),
                cx,
            );
            window.draw(cx).clear(cx);
            assert!(app.read(cx).password_page_scroll.offset().y < px(0.));
        }
    });
    let _ = app;
}

/// 平铺视图的列数随窗口宽度变化：放大/缩小窗口后重新排版，而不是停在旧列数。
#[gpui_kit::test]
fn tile_view_reflows_after_window_resize(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    for index in 0..24 {
        std::fs::write(
            directory.path().join(format!("file-{index:03}.txt")),
            b"lumark",
        )
        .unwrap();
    }
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    let tile_id = |index: usize| {
        format!(
            "file-tile-{}",
            directory
                .path()
                .join(format!("file-{index:03}.txt"))
                .display()
        )
    };
    cx.update(|window, cx| {
        app.update(cx, |app, _| app.tile_view = true);
        window.resize(gpui_kit::size(px(1200.), px(900.)));
        window.bounds_changed(cx);
        window.draw(cx).clear(cx);
        let first_tile = window.find(tile_id(0)).bounds().origin.y;
        let eighth = window.find(tile_id(7)).bounds().origin.y;
        assert!(
            eighth > first_tile,
            "窄窗口下第 8 个项目应换到第二行（first={first_tile:?}, eighth={eighth:?}）"
        );

        window.resize(gpui_kit::size(px(1800.), px(900.)));
        window.bounds_changed(cx);
        window.draw(cx).clear(cx);
        let first_tile = window.find(tile_id(0)).bounds().origin.y;
        let eighth = window.find(tile_id(7)).bounds().origin.y;
        assert_eq!(
            eighth, first_tile,
            "加宽窗口后第 8 个项目应回到第一行，列数需随窗口变化"
        );
    });
    let _ = app;
}

/// 平铺视图为图片文件生成真实缩略图，而不是一直显示类型图标。
#[gpui_kit::test]
fn tile_view_generates_image_thumbnails(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    for name in ["photo-a.png", "photo-b.png"] {
        image::RgbaImage::from_pixel(64, 64, image::Rgba([30, 140, 220, 255]))
            .save(directory.path().join(name))
            .unwrap();
    }
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        app.update(cx, |app, _| app.tile_view = true);
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.update(cx, |app, _| {
            let results = app.tile_thumbnails.borrow();
            let thumbnail = results
                .values()
                .flatten()
                .next()
                .cloned()
                .expect("平铺视图应为图片生成缩略图");
            assert!(
                thumbnail.is_file(),
                "缩略图文件应存在：{}",
                thumbnail.display()
            );
        });
        window.draw(cx).clear(cx);
        let photo = format!(
            "file-tile-{}",
            directory.path().join("photo-a.png").display()
        );
        assert!(window.find(photo).visible());
    });
    let _ = app;
}

/// 按资产类型筛选搜索结果，并在清除搜索时重置类型条件。
#[gpui_kit::test]
fn search_type_filter_limits_results_and_clear_resets(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("photo.png"), b"png").unwrap();
    std::fs::write(directory.path().join("notes.txt"), b"text").unwrap();
    std::fs::write(directory.path().join("bundle.zip"), b"zip").unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.find("search-type").visible(),
            "搜索栏应显示资产类型筛选"
        );
        app.update(cx, |app, cx| {
            app.search_category = Some(lumark_browser::AssetCategory::Image);
            app.run_search(cx);
            let names: Vec<String> = app
                .displayed_entries()
                .into_iter()
                .map(|entry| entry.name)
                .collect();
            assert_eq!(names, vec!["photo.png".to_string()], "图片筛选结果不正确");
            assert_eq!(app.displayed_count(), 1);

            app.search_category = Some(lumark_browser::AssetCategory::Text);
            app.run_search(cx);
            let names: Vec<String> = app
                .displayed_entries()
                .into_iter()
                .map(|entry| entry.name)
                .collect();
            assert!(
                names.iter().any(|name| name == "notes.txt"),
                "文本筛选应包含 notes.txt：{names:?}"
            );
            assert!(
                !names
                    .iter()
                    .any(|name| name == "photo.png" || name == "bundle.zip"),
                "文本筛选不应包含图片/压缩包：{names:?}"
            );
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("search-clear").visible(),
            "有搜索结果时应显示清除按钮"
        );
        window.click("search-clear", cx);
        app.update(cx, |app, _| {
            assert!(app.search_results.is_none(), "清除后应恢复文件列表");
            assert!(
                app.search_category.is_none(),
                "清除后应重置资产类型筛选，避免下次搜索被旧条件限制"
            );
        });
    });
    let _ = app;
}

/// 资产类型下拉菜单可以直接选中类型（菜单项按索引定位）。
#[gpui_kit::test]
fn search_type_dropdown_selects_category(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("photo.png"), b"png").unwrap();
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
        window.click("search-type", cx);
        window.draw(cx).clear(cx);
        // 菜单第一项是“全部”，第二项是“图片/纹理”。
        assert!(window.find(0usize).visible(), "菜单“全部”项应渲染");
        window.click(1usize, cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert_eq!(
            app.search_category,
            Some(lumark_browser::AssetCategory::Image),
            "点击“图片/纹理”应设置筛选类型"
        );
        assert!(
            app.status.contains("图片"),
            "状态应显示类型：{}",
            app.status
        );
        // 有搜索结果时切换类型应自动重新搜索。
        app.search_results = Some(Vec::new());
    });
    cx.update(|window, cx| {
        window.click("search-type", cx);
        window.draw(cx).clear(cx);
        window.click(1usize, cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        let names: Vec<String> = app
            .displayed_entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, vec!["photo.png".to_string()], "类型筛选应只剩图片");
        // 选回“全部”并再次搜索应清空结果（空文本 + 无类型）。
        app.search_results = Some(Vec::new());
    });
    cx.update(|window, cx| {
        window.click("search-type", cx);
        window.draw(cx).clear(cx);
        window.click(0usize, cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert!(app.search_category.is_none(), "“全部”应清空类型筛选");
        assert!(
            app.search_results.is_none(),
            "空文本 + 无类型应清空搜索结果"
        );
    });
    let _ = app;
}

/// 搜索模式下拉菜单可以切到正则表达式。
#[gpui_kit::test]
fn search_mode_dropdown_switches_to_regex(cx: &mut gpui_kit::TestAppContext) {
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
        window.click("search-mode", cx);
        window.draw(cx).clear(cx);
        // 菜单顺序：包含(0)、通配符(1)、正则表达式(2)。
        window.click(2usize, cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert_eq!(
            app.search_mode,
            lumark_browser::SearchMode::Regex,
            "点击“正则表达式”应切换搜索模式"
        );
        assert!(
            app.status.contains("正则"),
            "状态栏应提示正则模式：{}",
            app.status
        );
    });
    let _ = app;
}

/// 详情面板开关会切换可见状态并持久化到设置。
#[gpui_kit::test]
fn details_toggle_button_updates_setting(cx: &mut gpui_kit::TestAppContext) {
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
        let initial = app.read(cx).details_visible;
        window.click("details-toggle-panel", cx);
        window.draw(cx).clear(cx);
        app.update(cx, |app, _| {
            assert_ne!(app.details_visible, initial, "点击后详情面板可见性应反转");
            assert_eq!(
                app.settings.get().details_visible,
                app.details_visible,
                "详情面板可见性应写入设置"
            );
        });
        window.click("details-toggle-panel", cx);
        window.draw(cx).clear(cx);
        app.update(cx, |app, _| {
            assert_eq!(app.details_visible, initial, "再次点击应恢复原状态");
        });
    });
    let _ = app;
}

/// 正则搜索：在搜索框输入正则并运行，结果按正则匹配。
#[gpui_kit::test]
fn regex_search_filters_files(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("alpha.txt"), b"a").unwrap();
    std::fs::write(directory.path().join("beta.txt"), b"b").unwrap();
    std::fs::write(directory.path().join("gamma.md"), b"c").unwrap();
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
        window.click("search-input", cx);
        window.input("^a.*\\.txt$", cx);
        window.click("search-mode", cx);
        window.draw(cx).clear(cx);
        window.click(2usize, cx);
        window.click("search-run", cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert_eq!(app.search_mode, lumark_browser::SearchMode::Regex);
        let names: Vec<String> = app
            .displayed_entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            names,
            vec!["alpha.txt".to_string()],
            "正则搜索应只匹配 alpha.txt"
        );
    });
    let _ = app;
}
