//! 设置窗口的邮件通知选项卡。

use super::super::*;
use super::components::AppIcon;
use super::settings_window::{SettingsWindowView, settings_card, settings_row, settings_section};
use crate::app::actions::mail::MailSettingsDraft;
use gpui_kit::Entity;

impl SettingsWindowView {
    /// 渲染 SMTP 配置、启用开关、测试发送和清除密码操作。
    pub(super) fn render_notification_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        let tls_label = if settings.smtp_implicit_tls {
            "隐式 TLS（465）"
        } else if settings.smtp_starttls {
            "STARTTLS（587）"
        } else {
            "无 TLS"
        };
        settings_card(cx)
            .p_4()
            .gap_3()
            .child(settings_section(
                cx,
                AppIcon::Time,
                "邮件通知",
                Some("压缩成功后通过 SMTP 发送一封简短通知".to_string()),
            ))
            .child(settings_row(
                cx,
                "启用通知",
                "只影响压缩任务；测试发送不要求先启用。",
                button("settings-mail-toggle")
                    .label(if settings.email_notify {
                        "已启用"
                    } else {
                        "已关闭"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = !this.settings.email_notify;
                        this.app.update(cx, |app, cx| {
                            let _ = app.settings.update(|settings| settings.email_notify = next);
                            app.status = if next {
                                "邮件通知已启用".to_string()
                            } else {
                                "邮件通知已关闭".to_string()
                            };
                            cx.notify();
                        });
                        this.settings.email_notify = next;
                        cx.notify();
                    })),
            ))
            .child(self.mail_input_row("SMTP 主机", "settings-smtp-host", &self.smtp_host))
            .child(self.mail_input_row("SMTP 端口", "settings-smtp-port", &self.smtp_port))
            .child(self.mail_input_row("登录用户名", "settings-smtp-user", &self.smtp_username))
            .child(self.mail_input_row("登录密码", "settings-smtp-password", &self.smtp_password))
            .child(settings_row(
                cx,
                "清除已保存密码",
                "勾选后保存会删除 DPAPI 密文；下次发送不再认证。",
                button("settings-smtp-clear")
                    .label(if self.smtp_clear_password {
                        "将清除"
                    } else {
                        "保留密码"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.smtp_clear_password = !this.smtp_clear_password;
                        cx.notify();
                    })),
            ))
            .child(self.mail_input_row("发件人", "settings-smtp-from", &self.smtp_from))
            .child(self.mail_input_row("收件人", "settings-smtp-to", &self.smtp_to))
            .child(settings_row(
                cx,
                "加密方式",
                "按服务器要求选择；隐式 TLS 与 STARTTLS 互斥。",
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
                            app.status = "邮件加密方式已更新".to_string();
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
                    .child(button("settings-smtp-test").label("发送测试邮件").on_click(
                        cx.listener(|this, _, window, cx| {
                            if this.save_mail_form(window, cx).is_ok() {
                                this.app.update(cx, |app, cx| app.send_test_email(cx));
                            }
                        }),
                    ))
                    .child(
                        button("settings-smtp-save")
                            .primary()
                            .label("保存邮件设置")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let _ = this.save_mail_form(window, cx);
                            })),
                    ),
            )
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
            username: input_value(&self.smtp_username, cx),
            password: input_value(&self.smtp_password, cx),
            clear_password: self.smtp_clear_password,
            from: input_value(&self.smtp_from, cx),
            to: input_value(&self.smtp_to, cx),
            implicit_tls: self.settings.smtp_implicit_tls,
            starttls: self.settings.smtp_starttls,
        };
        let result = self.app.update(cx, |app, _| app.save_mail_settings(draft));
        match &result {
            Ok(()) => {
                self.status = "邮件通知设置已保存".to_string();
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
