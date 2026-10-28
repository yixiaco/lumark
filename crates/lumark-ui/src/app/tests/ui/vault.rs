use super::*;

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
/// 验证密码库导入/导出弹窗提供带标签的路径行，并使用标准按钮高度。
fn vault_transfer_dialog_uses_standard_buttons(cx: &mut gpui_kit::TestAppContext) {
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
        let export_dir = app.read(cx).vault_export_dir.clone();
        set_input(&export_dir, directory.path().to_str().unwrap(), window, cx);
        app.update(cx, |app, cx| {
            app.vault.unlock("master-pass").unwrap();
            app.screen = Screen::Passwords;
            app.show_vault_transfer = true;
            cx.notify();
        });
        window.draw(cx).clear(cx);
        for id in [
            "vault-export-dir",
            "vault-import-path",
            "vault-export-browse",
        ] {
            assert!(window.find(id).visible(), "导入/导出弹窗缺少控件：{id}");
        }
        for id in [
            "vault-export",
            "vault-export-csv",
            "vault-import",
            "vault-transfer-cancel",
            "vault-export-browse",
        ] {
            let button = window.find(id);
            assert!(
                button.bounds().size.height >= px(30.),
                "弹窗按钮仍为小尺寸：{id} 高度 {:?}",
                button.bounds().size.height
            );
        }
    });
    let _ = app;
}

#[gpui_kit::test]
/// 验证密码库锁定时，导入/导出弹窗先要求解锁，解锁后切换为完整表单。
fn vault_transfer_dialog_requires_unlock_when_locked(cx: &mut gpui_kit::TestAppContext) {
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
            app.screen = Screen::Passwords;
            app.show_vault_transfer = true;
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("vault-transfer-unlock").visible(),
            "锁定时应显示主密码输入框"
        );
        assert!(
            window.find("vault-transfer-unlock-confirm").visible(),
            "锁定时应显示解锁按钮"
        );
        assert!(
            window.try_find("vault-export").is_none(),
            "锁定时不应显示导出按钮"
        );
        assert!(
            window.try_find("vault-export-csv").is_none(),
            "锁定时不应显示 CSV 按钮"
        );

        app.update(cx, |app, cx| {
            app.vault.unlock("master-pass").unwrap();
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(
            window.find("vault-export-dir").visible(),
            "解锁后应显示导出位置"
        );
        assert!(window.find("vault-import-path").visible());
        assert!(
            window.try_find("vault-transfer-unlock").is_none(),
            "解锁后不应继续显示解锁输入框"
        );
    });
    let _ = app;
}

#[gpui_kit::test]
/// 验证密码库锁定时只能导出明文 CSV，解锁后同一弹窗写入所选文件夹。
fn vault_csv_export_requires_unlock(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let vault_path = directory.path().join("vault.dat");
    let (vault, _) = VaultCrypto::new_master(
        vec![PasswordEntry::new("CSV 记录", "csv-secret-123")],
        VaultSettings::default(),
        "master-pass",
    )
    .unwrap();
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
        let export_dir = app.read(cx).vault_export_dir.clone();
        set_input(&export_dir, directory.path().to_str().unwrap(), window, cx);
        app.update(cx, |app, cx| {
            app.screen = Screen::Passwords;
            app.show_vault_transfer = true;
            app.export_vault_csv(window, cx);
            assert!(
                app.status.contains("请先"),
                "锁定状态应提示先解锁：{}",
                app.status
            );
        });
        assert!(
            !std::fs::read_dir(directory.path())
                .unwrap()
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "csv")),
            "锁定状态不应写入 CSV"
        );

        app.update(cx, |app, cx| {
            app.vault.unlock("master-pass").unwrap();
            app.export_vault_csv(window, cx);
            assert!(
                app.status.contains("已导出 1 条明文记录"),
                "CSV 导出状态异常：{}",
                app.status
            );
        });
        let csv = std::fs::read_dir(directory.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "csv"))
            .expect("应生成 CSV 文件");
        let content = std::fs::read_to_string(&csv).unwrap();
        assert!(content.contains("CSV 记录"));
        assert!(content.contains("csv-secret-123"));
    });
    let _ = app;
}

#[gpui_kit::test]
/// 验证加密备份写入所选导出文件夹，而不是把输入当作完整文件路径。
fn vault_encrypted_export_writes_into_selected_folder(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let export_dir = directory.path().join("exports");
    std::fs::create_dir(&export_dir).unwrap();
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
        let export_dir_input = app.read(cx).vault_export_dir.clone();
        let passphrase = app.read(cx).vault_transfer_passphrase.clone();
        set_input(&export_dir_input, export_dir.to_str().unwrap(), window, cx);
        set_input(&passphrase, "export-password", window, cx);
        app.update(cx, |app, cx| {
            app.vault.unlock("master-pass").unwrap();
            app.vault
                .add_entry(PasswordEntry::new("备份记录", "backup-secret-123"))
                .unwrap();
            app.export_vault(window, cx);
            assert!(
                app.status.contains("已加密导出"),
                "加密导出状态异常：{}",
                app.status
            );
        });
        let exported = std::fs::read_dir(&export_dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "lumarkvault"))
            .expect("应在导出文件夹生成加密备份");
        assert_eq!(exported.parent(), Some(export_dir.as_path()));
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
