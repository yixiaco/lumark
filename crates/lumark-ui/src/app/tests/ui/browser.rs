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

/// 大目录只渲染可见行；详情/平铺切换后按新模式重建列表。
#[gpui_kit::test]
fn large_directory_renders_only_visible_rows(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    for index in 0..400 {
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
    let row_id = |index: usize| {
        format!(
            "file-row-{}",
            directory
                .path()
                .join(format!("file-{index:03}.txt"))
                .display()
        )
    };
    let tile_id = |index: usize| {
        format!(
            "file-tile-name-{}",
            directory
                .path()
                .join(format!("file-{index:03}.txt"))
                .display()
        )
    };
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find(row_id(0)).visible(), "首行应正常渲染");
        assert!(
            window.try_find(row_id(399)).is_none(),
            "400 项目录中最后一行不应被渲染（虚拟列表只画可见行）"
        );

        window.click("view-toggle", cx);
        window.draw(cx).clear(cx);
        assert!(window.find(tile_id(0)).visible(), "平铺首项应正常渲染");
        assert!(
            window.try_find(tile_id(399)).is_none(),
            "平铺视图同样只应渲染可见卡片"
        );

        window.click("view-toggle", cx);
        window.draw(cx).clear(cx);
        assert!(
            window.find(row_id(0)).visible(),
            "切回详情列表后应重新渲染文件行"
        );
        assert!(
            window.try_find(row_id(399)).is_none(),
            "切回详情后仍只渲染可见行"
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

/// 资产类型下拉菜单选中后立即过滤（无需再点「搜索」）。
#[gpui_kit::test]
fn search_type_dropdown_filters_immediately(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("photo.png"), b"png").unwrap();
    std::fs::write(directory.path().join("readme.txt"), b"text").unwrap();
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
        // 没有输入文本也没有点「搜索」；类型切换本身应产生过滤结果。
        let names: Vec<String> = app
            .displayed_entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, vec!["photo.png".to_string()], "类型筛选应立即生效");
        assert!(app.search_results.is_some(), "应立即进入搜索结果视图");
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

/// 方向键移动文件选择：上下/左右/Home/End 与 Shift 范围扩展。
#[gpui_kit::test]
fn arrow_keys_move_file_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"] {
        std::fs::write(directory.path().join(name), b"x").unwrap();
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
    /// 返回当前选择焦点在显示顺序中的下标。
    fn focus_index(state: &LumarkApp) -> Option<usize> {
        state.browser.selection.focus().and_then(|id| {
            state
                .displayed_ids()
                .iter()
                .position(|candidate| candidate == id)
        })
    }
    let ids: Vec<String> = app.update(cx, |app, _| app.displayed_ids());
    assert_eq!(ids.len(), 6);
    let focus = |cx: &mut gpui_kit::VisualTestContext| app.update(cx, |app, _| focus_index(app));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        app.update(cx, |app, cx| app.browser_focus.focus(window, cx));
        window.press("down", cx);
        window.draw(cx).clear(cx);
    });
    assert_eq!(focus(cx), Some(0), "Down 应选中第一个文件");
    cx.update(|window, cx| {
        window.press("down", cx);
        window.press("right", cx);
        window.draw(cx).clear(cx);
    });
    assert_eq!(focus(cx), Some(2), "Down/Right 应逐个向后移动");
    cx.update(|window, cx| {
        window.press("up", cx);
        window.press("left", cx);
        window.draw(cx).clear(cx);
    });
    assert_eq!(focus(cx), Some(0), "Up/Left 应逐个向前移动");
    cx.update(|window, cx| {
        window.press("end", cx);
        window.draw(cx).clear(cx);
    });
    assert_eq!(focus(cx), Some(5), "End 应选中最后一个");
    cx.update(|window, cx| {
        window.press("home", cx);
        window.press("shift-down", cx);
        window.press("shift-down", cx);
        window.draw(cx).clear(cx);
    });
    app.update(cx, |app, _| {
        assert_eq!(
            app.browser.selection.selected_ids().len(),
            3,
            "Shift+Down 应从锚点扩展选择"
        );
        assert_eq!(app.browser.selection.focus(), Some(ids[2].as_str()));
    });
    // 平铺视图上下移动一整行（列数）。
    cx.update(|window, cx| {
        window.click("view-toggle", cx);
        window.draw(cx).clear(cx);
        app.update(cx, |app, cx| {
            app.browser.selection.clear();
            app.browser_focus.focus(window, cx);
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    let columns = cx.update(|window, cx| {
        app.update(cx, |app, _| {
            app.tile_columns_at(app.browser_list_width(window))
        })
    });
    assert!(columns >= 2, "平铺视图应至少两列：{columns}");
    cx.update(|window, cx| {
        window.press("down", cx);
        window.press("down", cx);
        window.draw(cx).clear(cx);
    });
    assert_eq!(
        focus(cx),
        Some(columns.min(5)),
        "平铺视图 Down 应跨一整行（条目不足时夹到最后一个）"
    );
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
