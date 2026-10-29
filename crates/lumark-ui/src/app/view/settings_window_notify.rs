//! 设置窗口的邮件通知选项卡。

use super::super::*;
use super::components::AppIcon;
use super::settings_window::{SettingsWindowView, settings_card, settings_row, settings_section};
use crate::app::actions::mail::MailSettingsDraft;
use crate::i18n::tr;
use gpui_kit::Entity;

impl SettingsWindowView {
    /// 渲染 SMTP 配置、启用开关、测试发送和清除密码操作。
    pub(super) fn render_notification_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        let tls_label = if settings.smtp_implicit_tls {
            tr("隐式 TLS（465）", "Implicit TLS (465)")
        } else if settings.smtp_starttls {
            tr("STARTTLS（587）", "STARTTLS (587)")
        } else {
            tr("无 TLS", "No TLS")
        };
        settings_card(cx)
            .p_4()
            .gap_3()
            .child(settings_section(
                cx,
                AppIcon::Time,
                tr("邮件通知", "Email notifications"),
                Some(tr("压缩成功后通过 SMTP 发送一封简短通知", "Send a short SMTP notification after a successful compression").to_string()),
            ))
            .child(settings_row(
                cx,
                tr("启用通知", "Enable notifications"),
                tr("只影响压缩任务；测试发送不要求先启用。", "Applies to compression tasks only; test sending does not require enabling first."),
                button("settings-mail-toggle")
                    .label(if settings.email_notify {
                        tr("已启用", "Enabled")
                    } else {
                        "已关闭"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = !this.settings.email_notify;
                        this.app.update(cx, |app, cx| {
                            let _ = app.settings.update(|settings| settings.email_notify = next);
                            app.status = if next {
                                tr("邮件通知已启用", "Email notifications enabled").to_string()
                            } else {
                                tr("邮件通知已关闭", "Email notifications disabled").to_string()
                            };
                            cx.notify();
                        });
                        this.settings.email_notify = next;
                        cx.notify();
                    })),
            ))
            .child(self.mail_input_row(tr("SMTP 主机", "SMTP host"), "settings-smtp-host", &self.smtp_host))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(self.mail_input_row(
                                tr("SMTP 端口", "SMTP port"),
                                "settings-smtp-port",
                                &self.smtp_port,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(self.mail_input_row(
                                tr("SMTP 超时（秒）", "SMTP timeout (seconds)"),
                                "settings-smtp-timeout",
                                &self.smtp_timeout,
                            )),
                    ),
            )
            .child(self.mail_input_row(tr("登录用户名", "Username"), "settings-smtp-user", &self.smtp_username))
            .child(self.mail_input_row(
                if self.settings.smtp_xoauth2 {
                    tr("OAuth2 访问令牌", "OAuth2 access token")
                } else {
                    tr("登录密码", "Password")
                },
                "settings-smtp-password",
                &self.smtp_password,
            ))
            .child(settings_row(
                cx,
                tr("使用 XOAUTH2", "Use XOAUTH2"),
                tr(
                    "启用后密码字段保存 OAuth2 访问令牌，适用于 Gmail/Outlook 等支持 XOAUTH2 的服务器。",
                    "When enabled, the password field stores an OAuth2 access token for servers supporting XOAUTH2.",
                ),
                button("settings-smtp-xoauth2")
                    .label(if self.settings.smtp_xoauth2 {
                        tr("已启用", "Enabled")
                    } else {
                        tr("未启用", "Disabled")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = !this.settings.smtp_xoauth2;
                        this.app.update(cx, |app, cx| {
                            let _ = app.settings.update(|settings| settings.smtp_xoauth2 = next);
                            app.status = if next {
                                tr("已启用 XOAUTH2", "XOAUTH2 enabled").to_string()
                            } else {
                                tr("已停用 XOAUTH2", "XOAUTH2 disabled").to_string()
                            };
                            cx.notify();
                        });
                        this.settings.smtp_xoauth2 = next;
                        cx.notify();
                    })),
            ))
            .when(self.settings.smtp_xoauth2, |this| {
                this.child(self.mail_input_row(
                    tr("OAuth2 令牌端点", "OAuth2 token endpoint"),
                    "settings-smtp-oauth2-token-url",
                    &self.smtp_oauth2_token_url,
                ))
                .child(self.mail_input_row(
                    tr("OAuth2 客户端 ID", "OAuth2 client ID"),
                    "settings-smtp-oauth2-client-id",
                    &self.smtp_oauth2_client_id,
                ))
                .child(self.mail_input_row(
                    tr("OAuth2 客户端密钥", "OAuth2 client secret"),
                    "settings-smtp-oauth2-client-secret",
                    &self.smtp_oauth2_client_secret,
                ))
                .child(self.mail_input_row(
                    tr("OAuth2 刷新令牌", "OAuth2 refresh token"),
                    "settings-smtp-oauth2-refresh-token",
                    &self.smtp_oauth2_refresh_token,
                ))
            })
            .child(settings_row(
                cx,
                tr("清除已保存密码", "Clear saved password"),
                tr("勾选后保存会删除 DPAPI 密文；下次发送不再认证。", "Saving with this checked removes the DPAPI blob; future sends will not authenticate."),
                button("settings-smtp-clear")
                    .label(if self.smtp_clear_password {
                        tr("将清除", "Will clear")
                    } else {
                        tr("保留密码", "Keep password")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.smtp_clear_password = !this.smtp_clear_password;
                        cx.notify();
                    })),
            ))
            .child(self.mail_input_row(tr("发件人", "From"), "settings-smtp-from", &self.smtp_from))
            .child(self.mail_input_row(tr("收件人", "To"), "settings-smtp-to", &self.smtp_to))
            .child(settings_row(
                cx,
                tr("加密方式", "Encryption"),
                tr("按服务器要求选择；隐式 TLS 与 STARTTLS 互斥。", "Choose according to the server; implicit TLS and STARTTLS are mutually exclusive."),
                button("settings-smtp-tls")
                    .label(tls_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let (implicit, starttls) = if this.settings.smtp_implicit_tls {
                            (false, true)
                        } else if this.settings.smtp_starttls {
                            (false, false)
                        } else {
                            (true, false)
                        };
                        this.app.update(cx, |app, cx| {
                            let _ = app.settings.update(|settings| {
                                settings.smtp_implicit_tls = implicit;
                                settings.smtp_starttls = starttls;
                            });
                            app.status = tr("邮件加密方式已更新", "Email encryption updated").to_string();
                            cx.notify();
                        });
                        this.settings.smtp_implicit_tls = implicit;
                        this.settings.smtp_starttls = starttls;
                        cx.notify();
                    })),
            ))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(button("settings-smtp-test").label(tr("发送测试邮件", "Send test email")).on_click(
                        cx.listener(|this, _, window, cx| {
                            if this.save_mail_form(window, cx).is_ok() {
                                this.app.update(cx, |app, cx| app.send_test_email(cx));
                            }
                        }),
                    ))
                    .child(
                        button("settings-smtp-save")
                            .primary()
                            .label(tr("保存邮件设置", "Save email settings"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let _ = this.save_mail_form(window, cx);
                            })),
                    ),
            )
            .when(!self.settings.mail_outbox.is_empty(), |this| {
                this.child(settings_section(
                    cx,
                    AppIcon::Time,
                    tr("待重试通知", "Pending notifications"),
                    Some(
                        tr(
                            "压缩完成通知发送失败时会持久化到这里；修复 SMTP 后可手动重试。",
                            "Failed compression notifications are persisted here; retry after fixing SMTP.",
                        )
                        .to_string(),
                    ),
                ))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} {}",
                                    self.settings.mail_outbox.len(),
                                    tr("封待发送", "pending")
                                )),
                        )
                        .child(button("settings-mail-retry").label(tr("重试全部", "Retry all")).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.app.update(cx, |app, cx| app.retry_mail_outbox(cx));
                            }),
                        )),
                )
            })
            .when(!self.settings.mail_history.is_empty(), |this| {
                this.child(settings_section(
                    cx,
                    AppIcon::Time,
                    tr("最近发送", "Recent deliveries"),
                    Some(
                        tr(
                            "只保存最近 8 次测试或压缩通知的结果，便于排查 SMTP 配置。",
                            "The last 8 test or compression notification results are kept for SMTP troubleshooting.",
                        )
                        .to_string(),
                    ),
                ))
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .children(self.settings.mail_history.iter().take(5).map(|entry| {
                            let status = if entry.success {
                                tr("成功", "sent")
                            } else {
                                tr("失败", "failed")
                            };
                            let detail = entry
                                .error
                                .as_deref()
                                .unwrap_or_default();
                            v_flex()
                                .w_full()
                                .gap_0p5()
                                .child(
                                    div().w_full().text_xs().overflow_hidden().child(format!(
                                        "{}  {}  {}",
                                        entry.time, status, entry.subject
                                    )),
                                )
                                .when(!detail.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .w_full()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .overflow_hidden()
                                            .child(detail.to_string()),
                                    )
                                })
                        })),
                )
            })
    }

    /// 渲染带标题的邮件输入行。
    fn mail_input_row(
        &self,
        label: &'static str,
        id: &'static str,
        input: &Entity<InputState>,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(div().text_xs().child(label))
            .child(Input::new(input).id(id).w_full())
    }

    /// 把当前输入框内容保存到设置，并返回结果文本。
    fn save_mail_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let draft = MailSettingsDraft {
            notify: self.settings.email_notify,
            host: input_value(&self.smtp_host, cx),
            port: input_value(&self.smtp_port, cx),
            timeout_secs: input_value(&self.smtp_timeout, cx),
            username: input_value(&self.smtp_username, cx),
            password: input_value(&self.smtp_password, cx),
            clear_password: self.smtp_clear_password,
            from: input_value(&self.smtp_from, cx),
            to: input_value(&self.smtp_to, cx),
            implicit_tls: self.settings.smtp_implicit_tls,
            starttls: self.settings.smtp_starttls,
            xoauth2: self.settings.smtp_xoauth2,
            oauth2_token_url: input_value(&self.smtp_oauth2_token_url, cx),
            oauth2_client_id: input_value(&self.smtp_oauth2_client_id, cx),
            oauth2_client_secret: input_value(&self.smtp_oauth2_client_secret, cx),
            oauth2_refresh_token: input_value(&self.smtp_oauth2_refresh_token, cx),
        };
        let result = self.app.update(cx, |app, _| app.save_mail_settings(draft));
        match &result {
            Ok(()) => {
                self.status =
                    tr("邮件通知设置已保存", "Email notification settings saved").to_string();
                self.smtp_clear_password = false;
                set_input(&self.smtp_password, "", window, cx);
                self.refresh(cx);
            }
            Err(error) => self.status = error.clone(),
        }
        cx.notify();
        result
    }
}
