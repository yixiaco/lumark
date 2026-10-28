//! 邮件通知动作：保存 SMTP 配置、测试发送和压缩完成提醒。

use super::super::*;

/// 设置页提交的 SMTP 配置草稿。
pub(crate) struct MailSettingsDraft {
    /// 是否启用压缩完成通知。
    pub(crate) notify: bool,
    /// SMTP 主机。
    pub(crate) host: String,
    /// SMTP 端口。
    pub(crate) port: String,
    /// 登录用户名。
    pub(crate) username: String,
    /// 新密码；留空表示保留旧密码。
    pub(crate) password: String,
    /// 是否清除已保存的密码。
    pub(crate) clear_password: bool,
    /// 发件人地址。
    pub(crate) from: String,
    /// 收件人地址。
    pub(crate) to: String,
    /// 使用隐式 TLS。
    pub(crate) implicit_tls: bool,
    /// 使用 STARTTLS。
    pub(crate) starttls: bool,
}

impl LumarkApp {
    /// 保存 SMTP 配置；新密码用 DPAPI 加密后写入设置。
    pub(crate) fn save_mail_settings(&mut self, draft: MailSettingsDraft) -> Result<(), String> {
        let port = draft
            .port
            .trim()
            .parse::<u16>()
            .map_err(|_| "SMTP 端口必须是 1–65535 的数字".to_string())?;
        if port == 0 {
            return Err("SMTP 端口不能为 0".to_string());
        }
        if draft.implicit_tls && draft.starttls {
            return Err("隐式 TLS 和 STARTTLS 不能同时启用".to_string());
        }
        let protected = if draft.clear_password {
            None
        } else if draft.password.trim().is_empty() {
            self.settings.get().smtp_password
        } else {
            Some(
                platform::secret::protect_text(&draft.password)
                    .map_err(|error| error.to_string())?,
            )
        };
        self.settings
            .update(|settings| {
                settings.email_notify = draft.notify;
                settings.smtp_host = draft.host.trim().to_string();
                settings.smtp_port = port;
                settings.smtp_username = draft.username.trim().to_string();
                settings.smtp_password = protected;
                settings.smtp_from = draft.from.trim().to_string();
                settings.smtp_to = draft.to.trim().to_string();
                settings.smtp_implicit_tls = draft.implicit_tls;
                settings.smtp_starttls = draft.starttls;
            })
            .map_err(|error| error.to_string())?;
        self.status = "邮件通知设置已保存".to_string();
        Ok(())
    }

    /// 从设置构造发送配置；SMTP 未配置时返回 `None`。
    pub(crate) fn mail_config(&self) -> Result<Option<lumark_mail::MailConfig>, String> {
        let settings = self.settings.get();
        if settings.smtp_host.trim().is_empty() {
            return Ok(None);
        }
        let password = match settings.smtp_password.as_deref() {
            Some(blob) if !blob.is_empty() => {
                platform::secret::unprotect_text(blob).map_err(|error| error.to_string())?
            }
            _ => String::new(),
        };
        Ok(Some(lumark_mail::MailConfig {
            host: settings.smtp_host.clone(),
            port: settings.smtp_port,
            username: settings.smtp_username.clone(),
            password,
            from: settings.smtp_from.clone(),
            to: settings.smtp_to.clone(),
            implicit_tls: settings.smtp_implicit_tls,
            starttls: settings.smtp_starttls,
        }))
    }

    /// 发送一封测试邮件并在状态栏反馈结果。
    pub(crate) fn send_test_email(&mut self, cx: &mut Context<Self>) {
        let config = match self.mail_config() {
            Ok(Some(config)) => config,
            Ok(None) => {
                self.status = "请先启用邮件通知并填写 SMTP 配置".to_string();
                return;
            }
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.status = "正在发送测试邮件…".to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    lumark_mail::send(
                        &config,
                        "Lumark 测试邮件",
                        "这是一封来自 Lumark 的测试邮件。",
                    )
                    .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.status = match result {
                    Ok(()) => "测试邮件已发送".to_string(),
                    Err(error) => format!("测试邮件发送失败：{error}"),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// 压缩任务成功后发送通知；失败不改变任务结果。
    pub(crate) fn notify_compression_completed(
        &mut self,
        label: &str,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.get().email_notify {
            return;
        }
        let config = match self.mail_config() {
            Ok(Some(config)) => config,
            _ => return,
        };
        let subject = format!("Lumark 压缩完成：{label}");
        let body = format!("压缩任务已完成。\n\n{message}\n\nLumark");
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { lumark_mail::send(&config, &subject, &body) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |app, cx| {
                    app.status = format!("邮件通知发送失败：{error}");
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
