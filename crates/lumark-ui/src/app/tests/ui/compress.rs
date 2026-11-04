//! 压缩窗口、伪装窗口和 Shell 压缩批次测试。

use super::*;

/// 验证 Shell 多选压缩会合并路径，只打开一个轻巧压缩窗口。
#[gpui_kit::test]
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

/// 验证压缩窗口的文本框点击后都能获得焦点，避免只有输出文件名可输入。
#[gpui_kit::test]
fn compress_window_inputs_take_focus_on_click(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("sample.txt");
    std::fs::write(&source, b"lumark").unwrap();
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
            app.open_compress_window_for_sources(vec![source.clone()], window, cx);
        });
        window.draw(cx).clear(cx);
        let handle = app.read(cx).compress_window.expect("压缩窗口应已打开");
        handle
            .update(cx, |_, window, cx| {
                window.draw(cx).clear(cx);
                window.click("compress-window-output", cx);
                window.draw(cx).clear(cx);
                assert_eq!(
                    window.find("compress-window-output").focused(),
                    Some(true),
                    "轻巧模式输出路径输入框未获得焦点"
                );

                window.click("compress-window-password-toggle", cx);
                window.draw(cx).clear(cx);
                window.click("compress-window-password", cx);
                window.draw(cx).clear(cx);
                assert_eq!(
                    window.find("compress-window-password").focused(),
                    Some(true),
                    "轻巧模式密码输入框未获得焦点"
                );

                window.click("compress-window-switch", cx);
                window.draw(cx).clear(cx);
                window.click("compress-window-output-classic", cx);
                window.draw(cx).clear(cx);
                assert_eq!(
                    window.find("compress-window-output-classic").focused(),
                    Some(true),
                    "经典模式输出路径输入框未获得焦点"
                );

                window.click(("compress-window-tab", 2usize), cx);
                window.draw(cx).clear(cx);
                window.click("classic-exclude", cx);
                window.draw(cx).clear(cx);
                assert_eq!(
                    window.find("classic-exclude").focused(),
                    Some(true),
                    "经典模式排除规则输入框未获得焦点"
                );

                window.click(("compress-window-tab", 1usize), cx);
                window.draw(cx).clear(cx);
                window.click("compress-window-password-classic", cx);
                window.draw(cx).clear(cx);
                assert_eq!(
                    window.find("compress-window-password-classic").focused(),
                    Some(true),
                    "经典模式密码输入框未获得焦点"
                );

                // 测试窗口固定为轻巧模式高度，常规页底部会被页脚遮住；
                // 在可见的输出路径输入框上滚轮滚动，再点击分卷输入框。
                window.click(("compress-window-tab", 0usize), cx);
                window.draw(cx).clear(cx);
                window.scroll(
                    "compress-window-output-classic",
                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-160.))),
                    cx,
                );
                window.draw(cx).clear(cx);
                window.click("classic-split", cx);
                window.draw(cx).clear(cx);
                assert_eq!(
                    window.find("classic-split").focused(),
                    Some(true),
                    "分卷大小输入框未获得焦点"
                );
            })
            .unwrap();
    });
    let _ = app;
}

/// 验证独立伪装窗口渲染源文件、输出目录、面具和开始按钮。
#[gpui_kit::test]
fn disguise_window_renders_controls(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("sample.txt");
    std::fs::write(&source, b"lumark").unwrap();
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
            app.open_disguise_dialog_with_sources(vec![source.clone()], window, cx);
        });
        window.draw(cx).clear(cx);
        let handle = app.read(cx).disguise_window.expect("伪装窗口应已打开");
        handle
            .update(cx, |_, window, cx| {
                window.draw(cx).clear(cx);
                assert!(
                    window.find("disguise-window-output").visible(),
                    "伪装窗口未渲染输出目录"
                );
                assert!(
                    window.find("disguise-window-mask-select").visible(),
                    "伪装窗口未渲染面具下拉"
                );
                assert!(
                    window.find("disguise-window-choose-mask").visible(),
                    "伪装窗口未渲染选择面具按钮"
                );
                assert!(
                    window.find("disguise-window-start").visible(),
                    "伪装窗口未渲染开始按钮"
                );
            })
            .unwrap();
    });
    let _ = app;
}

/// 验证选择 ZPAQ 后算法页渲染 1-5 级压缩级别下拉。
#[gpui_kit::test]
fn compress_window_shows_zpaq_level_selector(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("sample.txt");
    std::fs::write(&source, b"lumark").unwrap();
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
            app.compress_format = ArchiveFormat::Zpaq;
            app.open_compress_window_for_sources(vec![source.clone()], window, cx);
        });
        window.draw(cx).clear(cx);
        let handle = app.read(cx).compress_window.expect("压缩窗口应已打开");
        handle
            .update(cx, |_, window, cx| {
                window.draw(cx).clear(cx);
                window.click("compress-window-switch", cx);
                window.click(("compress-window-tab", 3usize), cx);
                window.draw(cx).clear(cx);
                assert!(
                    window.find("compress-zpaq-method-select").visible(),
                    "ZPAQ 算法页未渲染压缩级别下拉"
                );
            })
            .unwrap();
    });
    let _ = app;
}

/// 验证已存在的同名压缩包复用原路径，由创建流程原子替换。
#[gpui_kit::test]
fn compress_dialog_reuses_existing_output_path(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("sample.txt");
    std::fs::write(&source, b"lumark").unwrap();
    let existing = directory.path().join("sample.zip");
    std::fs::write(&existing, b"old archive").unwrap();
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
            app.compress_format = ArchiveFormat::Zip;
            app.open_compress_dialog_with_sources(vec![source.clone()], window, cx);
        });
        let value = input_value(&app.read(cx).compress_output, cx);
        assert_eq!(
            PathBuf::from(value),
            existing,
            "同名压缩包不应自动改名为带序号的新文件"
        );
    });
    let _ = app;
}

/// 验证打开加密归档时会弹出独立密码输入框，而不是只改状态栏。
#[gpui_kit::test]
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
            app.open_archive_with_auto_password(archive, ArchiveOpenIntent::Browse, window, cx);
            assert!(app.archive_password_action.is_some());
        });
        window.draw(cx).clear(cx);
        assert!(window.find("archive-password").visible());
    });
    let _ = app;
}

/// 压缩窗口点击开始后应生成可读取的压缩包。
#[gpui_kit::test]
fn compress_window_creates_archive(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("inside.txt");
    std::fs::write(&source, b"inside").unwrap();
    let output = directory.path().join("created.zip");
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
            crate::app::util::set_input(
                &app.compress_output,
                output.to_str().expect("输出路径"),
                window,
                cx,
            );
            app.open_compress_window_for_sources(vec![source.clone()], window, cx);
        });
    });
    let handle = cx.update(|_, cx| app.read(cx).compress_window.expect("应打开压缩窗口"));
    cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("compress-window-start").visible());
        window.click("compress-window-start", cx);
    })
    .unwrap();
    let mut created = false;
    for _ in 0..200 {
        cx.run_until_parked();
        if output.is_file() && std::fs::metadata(&output).is_ok_and(|metadata| metadata.len() > 0) {
            created = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(created, "点击开始压缩后应生成压缩包");
    let listing = lumark_archive::ArchiveService::list(&output, None).expect("读取压缩包");
    assert!(
        listing
            .entries
            .iter()
            .any(|entry| entry.path.contains("inside.txt")),
        "压缩包应包含 inside.txt"
    );
    let _ = app;
}
