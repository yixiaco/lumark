use super::*;

#[gpui_kit::test]
/// 验证虚拟文件行和平铺卡片都暴露预期的测试目标。
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

#[gpui_kit::test]
/// 验证文件表最上方的“..”行能返回上一级，并在盘符根目录隐藏。
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

#[gpui_kit::test]
/// 验证密码列表先消费滚轮，外层页面不会抢先滚动。
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
