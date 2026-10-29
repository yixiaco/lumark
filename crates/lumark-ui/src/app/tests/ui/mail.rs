//! 邮件通知设置和 DPAPI 凭据测试。

use super::*;
use crate::app::actions::mail::MailSettingsDraft;

/// 验证 SMTP 密码以 DPAPI 密文保存，并能还原为发送配置。
#[gpui_kit::test]
fn mail_settings_protect_password_and_build_config(cx: &mut gpui_kit::TestAppContext) {
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
    app.update(cx, |app, _| {
        app.save_mail_settings(MailSettingsDraft {
            notify: true,
            host: "smtp.example.com".to_string(),
            port: "465".to_string(),
            timeout_secs: "30".to_string(),
            username: "user".to_string(),
            password: "smtp-pass".to_string(),
            clear_password: false,
            from: "Lumark <lumark@example.com>".to_string(),
            to: "user@example.com".to_string(),
            implicit_tls: true,
            starttls: false,
            xoauth2: false,
            oauth2_token_url: String::new(),
            oauth2_client_id: String::new(),
            oauth2_client_secret: String::new(),
            oauth2_refresh_token: String::new(),
        })
        .unwrap();
        let settings = app.settings.get();
        assert!(settings.email_notify);
        assert!(settings.smtp_password.is_some());
        assert_ne!(settings.smtp_password.as_deref(), Some("smtp-pass"));
        let config = app.mail_config().unwrap().unwrap();
        assert_eq!(config.host, "smtp.example.com");
        assert_eq!(config.password, "smtp-pass");
        assert_eq!(config.timeout_secs, 30);
        assert!(config.implicit_tls);
        assert!(!config.xoauth2);

        app.record_mail_history("test", &Ok(()));
        app.record_mail_history("failed", &Err("smtp down".to_string()));
        let history = app.settings.get().mail_history;
        assert_eq!(history.len(), 2);
        assert!(!history[0].success);
        assert_eq!(history[0].error.as_deref(), Some("smtp down"));
        assert!(history[1].success);

        app.queue_failed_mail("queued subject", "queued body", "smtp down");
        let outbox = app.settings.get().mail_outbox;
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].attempts, 1);
        assert_eq!(outbox[0].last_error.as_deref(), Some("smtp down"));

        app.save_mail_settings(MailSettingsDraft {
            notify: false,
            host: "smtp.example.com".to_string(),
            port: "465".to_string(),
            timeout_secs: "30".to_string(),
            username: "user".to_string(),
            password: String::new(),
            clear_password: true,
            from: "Lumark <lumark@example.com>".to_string(),
            to: "user@example.com".to_string(),
            implicit_tls: true,
            starttls: false,
            xoauth2: false,
            oauth2_token_url: String::new(),
            oauth2_client_id: String::new(),
            oauth2_client_secret: String::new(),
            oauth2_refresh_token: String::new(),
        })
        .unwrap();
        assert!(app.settings.get().smtp_password.is_none());
    });
}
