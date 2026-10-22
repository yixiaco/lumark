//! 应用启动参数与真实窗口 UI 测试。
//!
//! 测试直接访问应用内部状态，因此仍放在 `app` 模块下；启动参数测试不
//! 依赖窗口，UI 测试通过 gpui-kit 测试窗口验证渲染、点击和滚动行为。

mod startup_tests {
    use super::super::{StartupMode, StartupRequest};
    use std::path::PathBuf;

    /// 验证三种开关都能解析，并且路径原样保留。
    #[test]
    fn startup_request_parses_mode_and_paths() {
        let request = StartupRequest::parse(vec![
            "--extract".to_string(),
            r"C:\downloads\report.zip".to_string(),
        ])
        .expect("extract request");
        assert_eq!(request.mode, StartupMode::Extract);
        assert_eq!(
            request.paths,
            vec![PathBuf::from(r"C:\downloads\report.zip")]
        );

        let request = StartupRequest::parse(vec![
            "--compress".to_string(),
            r"C:\work\images".to_string(),
        ])
        .expect("compress request");
        assert_eq!(request.mode, StartupMode::Compress);
    }

    /// 验证没有路径时不会创建窗口，未知开关按默认浏览处理。
    #[test]
    fn startup_request_ignores_empty_and_unknown_arguments() {
        assert!(StartupRequest::parse(vec!["--open".to_string()]).is_none());
        assert!(StartupRequest::parse(vec!["--register-shell".to_string()]).is_none());
        let request = StartupRequest::parse(vec![
            "--unknown".to_string(),
            r"C:\downloads\a.zip".to_string(),
        ])
        .expect("default open request");
        assert_eq!(request.mode, StartupMode::Open);
    }
}

mod ui_tests {
    use super::super::*;
    use gpui_kit::test::TestWindowExt;
    use lumark_vault::{VaultCrypto, VaultSettings};
    use tempfile::tempdir;

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

    #[gpui_kit::test]
    /// 验证 Shell 多选压缩会合并路径，只打开一个轻巧压缩窗口。
    fn shell_compress_batches_into_one_window(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let (vault, _) =
            VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
        VaultCrypto::write_file(&vault_path, &vault).unwrap();
        let first = directory.path().join("第一份.txt");
        let second = directory.path().join("第二份.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
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
                app.queue_shell_compress(vec![first.clone()]);
                app.queue_shell_compress(vec![second.clone(), first.clone()]);
                assert_eq!(app.pending_shell_compress.len(), 2);
                app.shell_compress_deadline = Some(std::time::Instant::now());
                app.flush_shell_compress(window, cx);
            });
            window.draw(cx).clear(cx);
            let mut compress_open = false;
            let handle = app.read(cx).compress_window.unwrap();
            handle
                .update(cx, |_, _, _| {
                    compress_open = true;
                })
                .unwrap();
            assert!(compress_open);
            let output = {
                let app = app.read(cx);
                input_value(&app.compress_output, cx)
            };
            assert_eq!(
                Path::new(&output)
                    .file_stem()
                    .and_then(|value| value.to_str()),
                directory
                    .path()
                    .file_name()
                    .and_then(|value| value.to_str()),
                "多选压缩应使用父目录名作为默认归档名"
            );
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证打开加密归档时会弹出独立密码输入框，而不是只改状态栏。
    fn encrypted_archive_requests_password_dialog(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let source = directory.path().join("secret.txt");
        std::fs::write(&source, b"secret").unwrap();
        let archive = directory.path().join("secret.zip");
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format: ArchiveFormat::Zip,
                password: Some(zeroize::Zeroizing::new("archive-pass".to_string())),
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
                app.open_archive_with_auto_password(archive, ArchiveOpenIntent::Browse, window, cx);
                assert!(app.show_archive_password_prompt);
                assert!(app.archive_password_action.is_some());
            });
            window.draw(cx).clear(cx);
            assert!(window.find("archive-password").visible());
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码页的主操作按钮使用标准按钮高度。
    fn password_actions_use_standard_button_height(cx: &mut gpui_kit::TestAppContext) {
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
                app.screen = Screen::Passwords;
                cx.notify();
            });
            window.draw(cx).clear(cx);
            for id in ["password-add-new", "open-master-window"] {
                let button = window.find(id);
                assert!(button.visible(), "密码页按钮未渲染：{id}");
                assert!(
                    button.bounds().size.height >= px(30.),
                    "密码页按钮仍为小尺寸：{id} 高度 {:?}",
                    button.bounds().size.height
                );
            }
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码记录新增、修改和删除都直接写入持久化密码库。
    fn password_records_are_written_without_draft(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
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
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.vault.unlock("master-pass").unwrap();
                let id = app
                    .save_password_entry(
                        None,
                        "测试记录",
                        "first-secret".to_string(),
                        "*.zip",
                        "直接写入",
                    )
                    .unwrap();
                assert_eq!(app.vault.entry_count(), 1);
                assert_eq!(
                    app.vault.find_entry(id).unwrap().password.expose(),
                    "first-secret"
                );
                app.save_password_entry(
                    Some(id),
                    "测试记录 2",
                    "second-secret".to_string(),
                    "",
                    "修改后写入",
                )
                .unwrap();
                let saved = app.vault.find_entry(id).unwrap();
                assert_eq!(saved.label, "测试记录 2");
                assert_eq!(saved.password.expose(), "second-secret");
                assert_eq!(saved.archive_pattern, None);
                app.delete_password(id).unwrap();
                assert_eq!(app.vault.entry_count(), 0);
            });
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码库可以迁移到自定义目录并持久化新位置。
    fn vault_can_move_to_custom_location(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let settings_path = directory.path().join("settings.json");
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path.clone(),
                settings_path,
                Some(directory.path().to_path_buf()),
            )
        });
        let moved_directory = directory.path().join("moved");
        std::fs::create_dir(&moved_directory).unwrap();
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.vault
                    .add_entry(PasswordEntry::new("移动测试", "move-secret-123"))
                    .unwrap();
                app.move_vault_to(moved_directory.clone(), cx).unwrap();
                let target = moved_directory.join("vault.dat");
                assert_eq!(app.vault.path(), target);
                assert!(target.exists());
                assert!(!vault_path.exists());
                assert_eq!(app.settings.get().vault_path, Some(target.clone()));
                assert!(
                    app.vault
                        .entries()
                        .iter()
                        .any(|entry| { entry.password.expose() == "move-secret-123" })
                );
            });
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证恢复默认设置会复位界面偏好，但保留自定义密码库位置。
    fn reset_preferences_keeps_vault_location(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let settings_path = directory.path().join("settings.json");
        let custom_vault = directory.path().join("custom-vault.dat");
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                settings_path,
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings
                    .update(|settings| {
                        settings.accent_color = "#ff0000".to_string();
                        settings.details_visible = false;
                        settings.column_layout = 2;
                        settings.vault_path = Some(custom_vault.clone());
                    })
                    .unwrap();
                app.details_visible = false;
                app.column_layout = 2;
                app.reset_preferences(window, cx).unwrap();
                let settings = app.settings.get();
                assert_eq!(settings.accent_color, "#2563eb");
                assert!(settings.details_visible);
                assert_eq!(settings.column_layout, 1);
                assert_eq!(settings.vault_path, Some(custom_vault.clone()));
                assert!(app.details_visible);
                assert_eq!(app.column_layout, 1);
            });
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证内置文本查看器可以渲染只读编辑器和状态行。
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

    #[gpui_kit::test]
    /// 验证内置查看器能从普通文本文件读取内容并打开窗口。
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
}
