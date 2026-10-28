//! 自解压运行时的中英文界面选择。
//!
//! 优先读取 `LUMARK_LANG`（`en`/`zh` 前缀），否则使用 Windows 用户界面
//! 语言；非中文界面显示英文。该模块只覆盖自解压对话框的静态文本，压缩
//! 包内部错误仍使用归档层的中文描述。

/// 从命令行参数读取 `/LANG=` 或 `--lang=` 覆盖值。
pub(crate) fn language_override(arguments: &[String]) -> Option<&str> {
    arguments.iter().find_map(|argument| {
        argument
            .strip_prefix("/LANG=")
            .or_else(|| argument.strip_prefix("--lang="))
            .filter(|value| !value.trim().is_empty())
    })
}

/// 返回当前是否使用英文。
pub(crate) fn is_english() -> bool {
    let override_value = std::env::var("LUMARK_LANG").ok();
    #[cfg(windows)]
    let ui_language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    #[cfg(not(windows))]
    let ui_language = 0;
    choose_language(override_value.as_deref(), ui_language)
}

/// 根据环境变量和 Windows 主语言选择语言；抽成纯函数便于测试。
fn choose_language(override_value: Option<&str>, ui_language: u16) -> bool {
    if let Some(value) = override_value {
        let value = value.trim().to_ascii_lowercase();
        if value.starts_with("en") {
            return true;
        }
        if value.starts_with("zh") {
            return false;
        }
    }
    // LANGID 低 10 位为主语言；0x04 是中文。0 表示 API 不可用，回退中文。
    let primary = ui_language & 0x03ff;
    primary != 0 && primary != 0x04
}

/// 返回中文或英文字符串。
pub(crate) fn t(zh: &'static str, en: &'static str) -> &'static str {
    if is_english() { en } else { zh }
}

#[cfg(test)]
mod tests {
    use super::{choose_language, language_override};

    /// 验证环境变量优先于系统语言。
    #[test]
    fn environment_override_wins() {
        assert!(choose_language(Some("en-US"), 0x0804));
        assert!(!choose_language(Some("zh-CN"), 0x0409));
    }

    /// 验证窗口系统语言回退。
    #[test]
    fn windows_language_fallback() {
        assert!(!choose_language(None, 0x0804));
        assert!(choose_language(None, 0x0409));
        assert!(!choose_language(None, 0));
    }

    /// 验证命令行语言参数解析。
    #[test]
    fn language_argument_is_parsed() {
        let arguments = vec!["/S".to_string(), "/LANG=en-US".to_string()];
        assert_eq!(language_override(&arguments), Some("en-US"));
        assert_eq!(language_override(&["--lang=zh".to_string()]), Some("zh"));
        assert_eq!(language_override(&["/S".to_string()]), None);
    }
}
