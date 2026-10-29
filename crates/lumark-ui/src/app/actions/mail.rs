//! 邮件通知动作：保存 SMTP 配置、测试发送和压缩完成提醒。

use super::super::*;
use crate::i18n::tr;
use lumark_settings::{MailHistoryEntry, MailQueueEntry};

/// 设置页提交的 SMTP 配置草稿。
pub(crate) struct MailSettingsDraft {
    /// 是否启用压缩完成通知。
    pub(crate) notify: bool,
    /// SMTP 主机。
    pub(crate) host: String,
    /// SMTP 端口。
    pub(crate) port: String,
    /// SMTP 超时秒数。
    pub(crate) timeout_secs: String,
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
    /// 使用 XOAUTH2。
    pub(crate) xoauth2: bool,
    /// OAuth2 令牌端点。
    pub(crate) oauth2_token_url: String,
    /// OAuth2 客户端 ID。
    pub(crate) oauth2_client_id: String,
    /// 新的 OAuth2 客户端密钥；留空保留旧值。
    pub(crate) oauth2_client_secret: String,
    /// 新的 OAuth2 刷新令牌；留空保留旧值。
    pub(crate) oauth2_refresh_token: String,
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
            return Err(tr("SMTP 端口不能为 0", "SMTP port cannot be 0").to_string());
        }
        let timeout_secs = draft
            .timeout_secs
            .trim()
            .parse::<u64>()
            .map_err(|_| "SMTP 超时必须是 1–300 的整数".to_string())?;
        if !(1..=300).contains(&timeout_secs) {
            return Err(tr(
                "SMTP 超时必须在 1–300 秒之间",
                "SMTP timeout must be between 1 and 300 seconds",
            )
            .to_string());
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
        let previous = self.settings.get();
        let client_secret = if draft.oauth2_client_secret.trim().is_empty() {
            previous.smtp_oauth2_client_secret
        } else {
            Some(
                platform::secret::protect_text(&draft.oauth2_client_secret)
                    .map_err(|error| error.to_string())?,
            )
        };
        let refresh_token = if draft.oauth2_refresh_token.trim().is_empty() {
            previous.smtp_oauth2_refresh_token
        } else {
            Some(
                platform::secret::protect_text(&draft.oauth2_refresh_token)
                    .map_err(|error| error.to_string())?,
            )
        };
        self.settings
            .update(|settings| {
                settings.email_notify = draft.notify;
                settings.smtp_host = draft.host.trim().to_string();
                settings.smtp_port = port;
                settings.smtp_timeout_secs = timeout_secs;
                settings.smtp_username = draft.username.trim().to_string();
                settings.smtp_password = protected;
                settings.smtp_from = draft.from.trim().to_string();
                settings.smtp_to = draft.to.trim().to_string();
                settings.smtp_implicit_tls = draft.implicit_tls;
                settings.smtp_starttls = draft.starttls;
                settings.smtp_xoauth2 = draft.xoauth2;
                settings.smtp_oauth2_token_url = draft.oauth2_token_url.trim().to_string();
                settings.smtp_oauth2_client_id = draft.oauth2_client_id.trim().to_string();
                settings.smtp_oauth2_client_secret = client_secret;
                settings.smtp_oauth2_refresh_token = refresh_token;
            })
            .map_err(|error| error.to_string())?;
        self.status = tr("邮件通知设置已保存", "Email notification settings saved").to_string();
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
        let client_secret = match settings.smtp_oauth2_client_secret.as_deref() {
            Some(blob) if !blob.is_empty() => {
                platform::secret::unprotect_text(blob).map_err(|error| error.to_string())?
            }
            _ => String::new(),
        };
        let refresh_token = match settings.smtp_oauth2_refresh_token.as_deref() {
            Some(blob) if !blob.is_empty() => {
                platform::secret::unprotect_text(blob).map_err(|error| error.to_string())?
            }
            _ => String::new(),
        };
        Ok(Some(lumark_mail::MailConfig {
            host: settings.smtp_host.clone(),
            port: settings.smtp_port,
            timeout_secs: settings.smtp_timeout_secs,
            username: settings.smtp_username.clone(),
            password,
            from: settings.smtp_from.clone(),
            to: settings.smtp_to.clone(),
            implicit_tls: settings.smtp_implicit_tls,
            starttls: settings.smtp_starttls,
            xoauth2: settings.smtp_xoauth2,
            oauth2_token_url: settings.smtp_oauth2_token_url.clone(),
            oauth2_client_id: settings.smtp_oauth2_client_id.clone(),
            oauth2_client_secret: client_secret,
            oauth2_refresh_token: refresh_token,
        }))
    }

    /// 记录一次邮件发送结果，只保留最近 8 条。
    pub(crate) fn record_mail_history(&mut self, subject: &str, result: &Result<(), String>) {
        let entry = MailHistoryEntry {
            time: chrono::Local::now().format("%m-%d %H:%M").to_string(),
            subject: subject.to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().cloned(),
        };
        let _ = self.settings.update(|settings| {
            settings.mail_history.insert(0, entry);
            settings.mail_history.truncate(8);
        });
    }

    /// 把失败的压缩完成通知加入持久化重试队列。
    pub(crate) fn queue_failed_mail(&mut self, subject: &str, body: &str, error: &str) {
        let now = chrono::Local::now();
        let entry = MailQueueEntry {
            id: format!(
                "{}-{}",
                now.timestamp_nanos_opt().unwrap_or_default(),
                subject.len()
            ),
            created_at: now.format("%m-%d %H:%M").to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            attempts: 1,
            last_error: Some(error.to_string()),
        };
        let _ = self.settings.update(|settings| {
            settings.mail_outbox.insert(0, entry);
            settings.mail_outbox.truncate(20);
        });
    }

    /// 逐封重试持久化队列；成功的移除，失败的保留并更新错误。
    pub(crate) fn retry_mail_outbox(&mut self, cx: &mut Context<Self>) {
        let entries = self.settings.get().mail_outbox;
        if entries.is_empty() {
            self.status = tr("没有待重试的邮件", "No pending emails").to_string();
            return;
        }
        let config = match self.mail_config() {
            Ok(Some(config)) => config,
            Ok(None) => {
                self.status = tr(
                    "请先配置 SMTP 主机、发件人和收件人",
                    "Configure SMTP host, sender and recipients first",
                )
                .to_string();
                return;
            }
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.status = format!(
            "{}{}",
            tr("正在重试待发送邮件：", "Retrying pending emails: "),
            entries.len()
        );
        cx.spawn(async move |this, cx| {
            let mut sent = 0usize;
            let mut failed = Vec::new();
            for entry in entries {
                let config = config.clone();
                let subject = entry.subject.clone();
                let body = entry.body.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { lumark_mail::send(&config, &subject, &body) })
                    .await;
                match result {
                    Ok(()) => sent += 1,
                    Err(error) => {
                        let mut entry = entry;
                        entry.attempts = entry.attempts.saturating_add(1);
                        entry.last_error = Some(error.to_string());
                        failed.push(entry);
                    }
                }
            }
            let _ = this.update(cx, |app, cx| {
                let _ = app
                    .settings
                    .update(|settings| settings.mail_outbox = failed);
                app.status = format!(
                    "{}{}{}",
                    tr("重试完成：已发送 ", "Retry complete: sent "),
                    sent,
                    tr(" 封，失败项保留在队列", " ; failures stay queued")
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// 发送一封测试邮件并在状态栏反馈结果。
    pub(crate) fn send_test_email(&mut self, cx: &mut Context<Self>) {
        let config = match self.mail_config() {
            Ok(Some(config)) => config,
            Ok(None) => {
                self.status = tr(
                    "请先启用邮件通知并填写 SMTP 配置",
                    "Enable email notifications and fill SMTP settings first",
                )
                .to_string();
                return;
            }
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.status = tr("正在发送测试邮件…", "Sending test email...").to_string();
        let subject = tr("Lumark 测试邮件", "Lumark test email").to_string();
        cx.spawn(async move |this, cx| {
            let send_subject = subject.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    lumark_mail::send(
                        &config,
                        &send_subject,
                        tr(
                            "这是一封来自 Lumark 的测试邮件。",
                            "This is a test email from Lumark.",
                        ),
                    )
                    .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.record_mail_history(&subject, &result);
                app.status = match result {
                    Ok(()) => tr("测试邮件已发送", "Test email sent").to_string(),
                    Err(error) => format!(
                        "{}{error}",
                        tr("测试邮件发送失败：", "Failed to send test email: ")
                    ),
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
        let subject = format!(
            "{}{label}",
            tr("Lumark 压缩完成：", "Lumark compression complete: ")
        );
        let body = format!(
            "{}\n\n{message}\n\nLumark",
            tr("压缩任务已完成。", "The compression task has completed.")
        );
        let history_subject = subject.clone();
        let queue_body = body.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { lumark_mail::send(&config, &subject, &body) })
                .await;
            let history = result.as_ref().map(|_| ()).map_err(ToString::to_string);
            let _ = this.update(cx, |app, cx| {
                app.record_mail_history(&history_subject, &history);
                if let Err(error) = &result {
                    app.queue_failed_mail(&history_subject, &queue_body, &error.to_string());
                    app.status = format!(
                        "{}{error}",
                        tr("邮件通知发送失败：", "Failed to send email notification: ")
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
