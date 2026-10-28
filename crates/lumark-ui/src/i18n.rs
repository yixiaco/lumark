//! 主界面最小本地化状态。
//!
//! 设置中的 `language` 字段切换当前 UI 语言；当前先接入动作带等核心文本，
//! 其余页面按模块逐步迁移，避免一次性替换全部中文。

use std::sync::atomic::{AtomicBool, Ordering};

/// 当前是否英文。
static ENGLISH: AtomicBool = AtomicBool::new(false);

/// 根据设置值更新当前语言。
pub(crate) fn set_language(language: &str) {
    ENGLISH.store(
        language.trim().to_ascii_lowercase().starts_with("en"),
        Ordering::Relaxed,
    );
}

/// 返回当前语言是否为英文。
pub(crate) fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// 按当前语言选择中文或英文文本。
pub(crate) fn tr(zh: &'static str, en: &'static str) -> &'static str {
    if is_english() { en } else { zh }
}

#[cfg(test)]
mod tests {
    use super::tr;
    use super::{is_english, set_language};

    /// 验证语言开关和文本选择。
    #[test]
    fn language_toggle_switches_text() {
        set_language("en-US");
        assert!(is_english());
        assert_eq!(tr("压缩", "Compress"), "Compress");
        set_language("zh-CN");
        assert_eq!(tr("压缩", "Compress"), "压缩");
    }
}
