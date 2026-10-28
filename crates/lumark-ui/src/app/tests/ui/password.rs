//! 密码弹窗、自动验证和保存流程测试。

use super::*;

/// 验证自动尝试失败后弹出密码框，并默认勾选“保存到密码库”。
#[gpui_kit::test]
fn auto_password_failure_offers_saving_password(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let archive = directory.path().join("secret.zip");
    std::fs::write(&archive, b"placeholder").unwrap();
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
            app.apply_password_failure(
                PendingPasswordAction::Test {
                    archive: archive.clone(),
                },
                window,
                cx,
            );
            assert!(app.show_archive_password_prompt);
            assert!(app.archive_password_after_failure);
            assert!(app.archive_password_save_to_vault);
        });
        window.draw(cx).clear(cx);
        assert!(window.find("archive-password").visible());
        assert!(
            window.find("archive-password-save").visible(),
            "自动尝试失败后没有渲染保存密码的勾选项"
        );
    });
    let _ = app;
}

/// 验证手动输入密码并勾选保存时先走校验任务，不会立即写入密码库。
#[gpui_kit::test]
fn manual_password_with_save_verifies_before_writing(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let archive = directory.path().join("secret.zip");
    std::fs::write(&archive, b"placeholder").unwrap();
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
            app.apply_password_failure(
                PendingPasswordAction::Test {
                    archive: archive.clone(),
                },
                window,
                cx,
            );
            set_input(&app.archive_password_input, "archive-pass", window, cx);
            app.confirm_archive_password(window, cx);
            assert!(!app.show_archive_password_prompt);
            assert_eq!(app.pending_password_jobs.len(), 1);
            assert!(
                app.pending_password_jobs
                    .values()
                    .all(|job| job.save_on_success),
                "手动密码没有标记为验证成功后保存"
            );
            assert_eq!(app.vault.entry_count(), 0, "密码在验证完成前就写入了密码库");
        });
    });
    let _ = app;
}

/// 验证校验通过的密码按归档路径写入密码库，并跳过重复记录。
#[gpui_kit::test]
fn verified_password_is_saved_with_archive_pattern(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) =
        VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
    VaultCrypto::write_file(&vault_path, &vault).unwrap();
    let archive = directory.path().join("secret.zip");
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            vault_path,
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            app.vault.unlock("master-pass").unwrap();
            assert!(app.remember_verified_password(&archive, "archive-pass"));
            assert!(
                !app.remember_verified_password(&archive, "archive-pass"),
                "相同密码不应重复写入"
            );
            assert_eq!(app.vault.entry_count(), 1);
            let entry = app.vault.entries().into_iter().next().unwrap();
            assert_eq!(entry.password.expose(), "archive-pass");
            assert_eq!(
                entry.archive_pattern.as_deref(),
                Some(archive.to_string_lossy().as_ref())
            );
        });
    });
    let _ = app;
}

/// 验证校验进度弹窗和状态栏都提供停止自动尝试的入口。
#[gpui_kit::test]
fn password_attempt_dialog_offers_stop_button(cx: &mut gpui_kit::TestAppContext) {
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
            app.password_attempt = Some(PasswordAttemptProgress {
                task_id: 999,
                archive: directory.path().join("secret.zip"),
                index: 3,
                total: 10,
                started: std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_millis(500))
                    .expect("backdated attempt"),
                hidden: false,
            });
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("password-attempt-stop").visible(),
            "校验弹窗没有停止按钮"
        );
        assert!(
            window.find("stop-password-attempt").visible(),
            "状态栏没有停止入口"
        );
    });
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.cancel_password_attempt(cx);
            assert!(app.password_attempt.is_none());
            assert_eq!(app.status, "密码验证已经结束");
        });
    });
    let _ = app;
}

/// 验证独立密码编辑窗口能渲染并保持输入控件可交互。
#[gpui_kit::test]
fn password_editor_window_renders_controls(cx: &mut gpui_kit::TestAppContext) {
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
    let handle = cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.vault.unlock("master-pass").unwrap();
            app.open_password_editor_window(None, window, cx);
        });
        app.read(cx).password_editor_window.expect("密码编辑窗口")
    });
    let _ = cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("password-editor-label").visible());
        assert!(window.find("password-editor-value").visible());
        assert!(window.find("password-editor-pattern").visible());
        assert!(window.find("password-editor-save").visible());
    });
    let _ = app;
}
