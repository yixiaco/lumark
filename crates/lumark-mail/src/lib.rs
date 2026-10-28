//! 压缩完成邮件通知的 SMTP 发送器。
//!
//! 配置由设置页提供；密码由调用方从 DPAPI 解密后传入，本模块不持久化任何
//! 凭据。网络发送在后台线程执行，失败只返回错误文本，不影响压缩任务。

use lettre::message::Message;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};

/// 一次 SMTP 发送所需的配置。
#[derive(Debug, Clone, Default)]
pub struct MailConfig {
    /// SMTP 主机。
    pub host: String,
    /// SMTP 端口。
    pub port: u16,
    /// 登录用户名；为空表示不做认证。
    pub username: String,
    /// 登录密码；由调用方从 DPAPI 解密后传入。
    pub password: String,
    /// 发件人地址。
    pub from: String,
    /// 收件人地址；多个地址用逗号分隔。
    pub to: String,
    /// 使用隐式 TLS（常见端口 465）。
    pub implicit_tls: bool,
    /// 使用 STARTTLS（常见端口 587）。
    pub starttls: bool,
}

/// 邮件发送错误。
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// 配置不完整或互斥。
    #[error("{0}")]
    Invalid(String),
    /// 地址或消息构建失败。
    #[error("构建邮件失败：{0}")]
    Message(String),
    /// SMTP 连接或发送失败。
    #[error("发送邮件失败：{0}")]
    Send(String),
}

impl MailConfig {
    /// 校验主机、端口、地址和 TLS 选项。
    pub fn validate(&self) -> Result<(), MailError> {
        if self.host.trim().is_empty() {
            return Err(MailError::Invalid("SMTP 主机不能为空".to_string()));
        }
        if self.port == 0 {
            return Err(MailError::Invalid("SMTP 端口不能为 0".to_string()));
        }
        if self.from.trim().is_empty() {
            return Err(MailError::Invalid("发件人地址不能为空".to_string()));
        }
        if self.to.trim().is_empty() {
            return Err(MailError::Invalid("收件人地址不能为空".to_string()));
        }
        if self.implicit_tls && self.starttls {
            return Err(MailError::Invalid(
                "隐式 TLS 和 STARTTLS 不能同时启用".to_string(),
            ));
        }
        Ok(())
    }

    /// 构建邮件消息；供发送和单元测试共用。
    pub fn build_message(&self, subject: &str, body: &str) -> Result<Message, MailError> {
        self.validate()?;
        let from = self
            .from
            .parse()
            .map_err(|error| MailError::Message(format!("发件人地址无效：{error}")))?;
        let mut builder = Message::builder().from(from).subject(subject);
        for recipient in self
            .to
            .split([',', ';'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let mailbox = recipient
                .parse()
                .map_err(|error| MailError::Message(format!("收件人地址无效：{error}")))?;
            builder = builder.to(mailbox);
        }
        builder
            .body(body.to_string())
            .map_err(|error| MailError::Message(error.to_string()))
    }
}

/// 发送一封纯文本邮件。
pub fn send(config: &MailConfig, subject: &str, body: &str) -> Result<(), MailError> {
    let message = config.build_message(subject, body)?;
    let builder = if config.implicit_tls {
        SmtpTransport::relay(config.host.trim())
            .map_err(|error| MailError::Send(error.to_string()))?
            .port(config.port)
    } else if config.starttls {
        SmtpTransport::starttls_relay(config.host.trim())
            .map_err(|error| MailError::Send(error.to_string()))?
            .port(config.port)
    } else {
        SmtpTransport::builder_dangerous(config.host.trim()).port(config.port)
    };
    let builder = if config.username.trim().is_empty() {
        builder
    } else {
        builder.credentials(Credentials::new(
            config.username.clone(),
            config.password.clone(),
        ))
    };
    builder
        .build()
        .send(&message)
        .map(|_| ())
        .map_err(|error| MailError::Send(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个用于本地测试的完整配置。
    fn config() -> MailConfig {
        MailConfig {
            host: "smtp.example.com".to_string(),
            port: 465,
            username: "user".to_string(),
            password: "secret".to_string(),
            from: "Lumark <lumark@example.com>".to_string(),
            to: "user@example.com, second@example.com".to_string(),
            implicit_tls: true,
            starttls: false,
        }
    }

    /// 验证配置校验会拒绝空主机和互斥 TLS。
    #[test]
    fn validation_rejects_missing_host_and_mixed_tls() {
        let mut missing_host = config();
        missing_host.host.clear();
        assert!(matches!(
            missing_host.validate(),
            Err(MailError::Invalid(_))
        ));
        let mut mixed_tls = config();
        mixed_tls.starttls = true;
        assert!(matches!(mixed_tls.validate(), Err(MailError::Invalid(_))));
    }

    /// 验证多收件人邮件可以成功构建。
    #[test]
    fn builds_message_with_multiple_recipients() {
        let message = config().build_message("压缩完成", "archive.zip").unwrap();
        let formatted = message.formatted();
        let raw = String::from_utf8_lossy(&formatted);
        assert!(raw.contains("user@example.com"));
        assert!(raw.contains("second@example.com"));
        assert!(raw.contains("压缩完成") || raw.contains("=?utf-8?"));
    }
}
