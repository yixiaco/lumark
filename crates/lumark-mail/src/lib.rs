//! 压缩完成邮件通知的 SMTP 发送器。
//!
//! 配置由设置页提供；密码由调用方从 DPAPI 解密后传入，本模块不持久化任何
//! 凭据。网络发送在后台线程执行，失败只返回错误文本，不影响压缩任务。

use std::time::Duration;

use lettre::message::Message;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};

/// 一次 SMTP 发送所需的配置。
#[derive(Debug, Clone)]
pub struct MailConfig {
    /// SMTP 主机。
    pub host: String,
    /// SMTP 端口。
    pub port: u16,
    /// SMTP 连接和命令超时秒数，允许 1–300 秒。
    pub timeout_secs: u64,
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
    /// 使用 XOAUTH2；没有刷新令牌时密码字段保存访问令牌。
    pub xoauth2: bool,
    /// OAuth2 令牌端点；配置刷新令牌后使用。
    pub oauth2_token_url: String,
    /// OAuth2 客户端 ID。
    pub oauth2_client_id: String,
    /// OAuth2 客户端密钥；公共客户端可为空。
    pub oauth2_client_secret: String,
    /// OAuth2 刷新令牌；为空时直接使用密码字段中的访问令牌。
    pub oauth2_refresh_token: String,
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
    /// 可重试的瞬时 SMTP/网络错误。
    #[error("邮件暂时发送失败：{0}")]
    Transient(String),
    /// OAuth2 刷新令牌换取访问令牌失败。
    #[error("OAuth2 认证失败：{0}")]
    OAuth(String),
}

impl MailError {
    /// 判断错误是否适合自动重试。
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient(_))
    }
}

impl Default for MailConfig {
    /// 返回安全的默认 SMTP 配置。
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 465,
            timeout_secs: 30,
            username: String::new(),
            password: String::new(),
            from: String::new(),
            to: String::new(),
            implicit_tls: true,
            starttls: false,
            xoauth2: false,
            oauth2_token_url: String::new(),
            oauth2_client_id: String::new(),
            oauth2_client_secret: String::new(),
            oauth2_refresh_token: String::new(),
        }
    }
}

impl MailConfig {
    /// 校验主机、端口、超时、地址和 TLS 选项。
    pub fn validate(&self) -> Result<(), MailError> {
        if self.host.trim().is_empty() {
            return Err(MailError::Invalid("SMTP 主机不能为空".to_string()));
        }
        if self.port == 0 {
            return Err(MailError::Invalid("SMTP 端口不能为 0".to_string()));
        }
        if !(1..=300).contains(&self.timeout_secs) {
            return Err(MailError::Invalid(
                "SMTP 超时必须在 1–300 秒之间".to_string(),
            ));
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
        if self.xoauth2 {
            if self.username.trim().is_empty() {
                return Err(MailError::Invalid("XOAUTH2 需要登录用户名".to_string()));
            }
            if self.oauth2_refresh_token.is_empty() {
                if self.password.is_empty() {
                    return Err(MailError::Invalid(
                        "XOAUTH2 需要访问令牌或刷新令牌".to_string(),
                    ));
                }
            } else if self.oauth2_token_url.trim().is_empty()
                || self.oauth2_client_id.trim().is_empty()
            {
                return Err(MailError::Invalid(
                    "配置刷新令牌时必须填写令牌端点和客户端 ID".to_string(),
                ));
            }
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
///
/// 对瞬时 SMTP/网络错误最多尝试 3 次，每次退避 500ms 的整数倍；认证失败、
/// 地址错误等永久错误不会重试。
pub fn send(config: &MailConfig, subject: &str, body: &str) -> Result<(), MailError> {
    send_with_retry(config, subject, body, 3, Duration::from_millis(500))
}

/// 发送一封纯文本邮件，并允许调用方指定总尝试次数和退避基数。
pub fn send_with_retry(
    config: &MailConfig,
    subject: &str,
    body: &str,
    attempts: u32,
    backoff: Duration,
) -> Result<(), MailError> {
    let message = config.build_message(subject, body)?;
    let access_token = if config.xoauth2 && !config.oauth2_refresh_token.is_empty() {
        refresh_access_token(config)?
    } else {
        config.password.clone()
    };
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match send_once(config, &message, &access_token) {
            Ok(()) => return Ok(()),
            Err(error) if error.is_retryable() && attempt < attempts => {
                std::thread::sleep(backoff.saturating_mul(attempt));
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| MailError::Send("没有执行 SMTP 发送".to_string())))
}

/// 使用刷新令牌从 OAuth2 令牌端点获取新的访问令牌。
fn refresh_access_token(config: &MailConfig) -> Result<String, MailError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|error| MailError::OAuth(format!("创建 HTTP 客户端失败：{error}")))?;
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", config.oauth2_refresh_token.clone()),
        ("client_id", config.oauth2_client_id.clone()),
    ];
    if !config.oauth2_client_secret.is_empty() {
        form.push(("client_secret", config.oauth2_client_secret.clone()));
    }
    let response = client
        .post(config.oauth2_token_url.trim())
        .form(&form)
        .send()
        .map_err(|error| MailError::OAuth(format!("请求令牌端点失败：{error}")))?;
    let status = response.status();
    let text = response
        .text()
        .map_err(|error| MailError::OAuth(format!("读取令牌响应失败：{error}")))?;
    if !status.is_success() {
        return Err(MailError::OAuth(format!(
            "令牌端点返回 {status}：{}",
            text.chars().take(300).collect::<String>()
        )));
    }
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| MailError::OAuth(format!("解析令牌响应失败：{error}")))?;
    value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| MailError::OAuth("令牌响应缺少 access_token".to_string()))
}

/// 执行一次 SMTP 发送，并把可恢复错误标记为 `Transient`。
fn send_once(config: &MailConfig, message: &Message, access_token: &str) -> Result<(), MailError> {
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
            access_token.to_string(),
        ))
    };
    let builder = if config.xoauth2 {
        builder.authentication(vec![Mechanism::Xoauth2])
    } else {
        builder
    };
    builder
        .timeout(Some(Duration::from_secs(config.timeout_secs)))
        .build()
        .send(message)
        .map(|_| ())
        .map_err(|error| {
            let message = error.to_string();
            if error.is_transient() || error.is_timeout() {
                MailError::Transient(message)
            } else {
                MailError::Send(message)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个用于本地测试的完整配置。
    fn config() -> MailConfig {
        MailConfig {
            host: "smtp.example.com".to_string(),
            port: 465,
            timeout_secs: 30,
            username: "user".to_string(),
            password: "secret".to_string(),
            from: "Lumark <lumark@example.com>".to_string(),
            to: "user@example.com, second@example.com".to_string(),
            implicit_tls: true,
            starttls: false,
            xoauth2: false,
            oauth2_token_url: String::new(),
            oauth2_client_id: String::new(),
            oauth2_client_secret: String::new(),
            oauth2_refresh_token: String::new(),
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
        let mut bad_timeout = config();
        bad_timeout.timeout_secs = 0;
        assert!(matches!(bad_timeout.validate(), Err(MailError::Invalid(_))));
        let mut missing_token = config();
        missing_token.xoauth2 = true;
        missing_token.password.clear();
        assert!(matches!(
            missing_token.validate(),
            Err(MailError::Invalid(_))
        ));
    }

    /// 刷新令牌请求应解析令牌端点返回的 access_token。
    #[test]
    fn refreshes_access_token_from_endpoint() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let body = r#"{"access_token":"fresh-token","token_type":"Bearer"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let mut config = config();
        config.xoauth2 = true;
        config.oauth2_token_url = format!("http://{address}/token");
        config.oauth2_client_id = "client".to_string();
        config.oauth2_refresh_token = "refresh".to_string();
        assert_eq!(refresh_access_token(&config).unwrap(), "fresh-token");
        server.join().unwrap();
    }

    /// 只有瞬时错误应被标记为可重试。
    #[test]
    fn transient_errors_are_retryable() {
        assert!(MailError::Transient("try again".to_string()).is_retryable());
        assert!(!MailError::Send("bad auth".to_string()).is_retryable());
        assert!(!MailError::Invalid("bad config".to_string()).is_retryable());
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
