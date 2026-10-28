//! 主界面最小本地化状态。
//!
//! 设置中的 `language` 字段切换当前 UI 语言；当前先接入动作带等核心文本，
//! 其余页面按模块逐步迁移，避免一次性替换全部中文。

use std::sync::atomic::{AtomicBool, Ordering};

/// 当前是否英文。
static ENGLISH: AtomicBool = AtomicBool::new(false);

/// 根据设置值更新当前语言。
pub(crate) fn set_language(language: &str) {
    let english = language.trim().to_ascii_lowercase().starts_with("en");
    ENGLISH.store(english, Ordering::Relaxed);
    let locale = if english { "en" } else { "zh-CN" };
    rust_i18n::set_locale(locale);
    gpui_kit::component::set_locale(locale);
}

/// 返回当前语言是否为英文。
pub(crate) fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// 按当前语言选择中文或英文文本。
pub(crate) fn tr(zh: &'static str, en: &'static str) -> &'static str {
    if is_english() { en } else { zh }
}

/// 从应用 locale 文件读取 key。
pub(crate) fn tr_key(key: &str) -> String {
    rust_i18n::t!(key).to_string()
}

#[cfg(test)]
mod tests {
    use super::{is_english, set_language, tr, tr_key};
    use std::sync::Mutex;

    /// 语言状态是全局的，测试串行执行避免互相覆盖。
    static LOCK: Mutex<()> = Mutex::new(());

    /// 验证语言开关、应用文案和官方 locale 文件。
    #[test]
    fn language_switch_updates_text_and_locales() {
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        set_language("en-US");
        assert!(is_english());
        assert_eq!(tr("压缩", "Compress"), "Compress");
        assert_eq!(tr_key("app.compress"), "Compress");
        set_language("zh-CN");
        assert!(!is_english());
        assert_eq!(tr("压缩", "Compress"), "压缩");
        assert_eq!(tr_key("app.compress"), "压缩");
    }
}
