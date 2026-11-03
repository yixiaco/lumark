//! 主程序与自解压运行时共用的中英文界面开关。
//!
//! 语言只由调用方决定一次：主程序读取设置，自解压运行时读取命令行或
//! Windows 界面语言。共享控件只通过本模块取文案，不直接依赖 locale 文件。

use std::sync::atomic::{AtomicBool, Ordering};

/// 当前是否英文。
static ENGLISH: AtomicBool = AtomicBool::new(false);

/// 设置当前界面语言。
pub fn set_english(english: bool) {
    ENGLISH.store(english, Ordering::Relaxed);
}

/// 返回当前界面是否英文。
pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// 返回当前语言对应的 locale 名称。
pub fn locale() -> &'static str {
    if is_english() { "en" } else { "zh-CN" }
}

/// 按当前语言选择中文或英文文本。
pub fn tr(zh: &'static str, en: &'static str) -> &'static str {
    if is_english() { en } else { zh }
}

#[cfg(test)]
mod tests {
    use super::{is_english, locale, set_english, tr};
    use std::sync::Mutex;

    /// 语言状态是全局的，测试串行执行避免互相覆盖。
    static LOCK: Mutex<()> = Mutex::new(());

    /// 验证语言开关同时影响文本和 locale。
    #[test]
    fn language_switch_updates_text_and_locale() {
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        set_english(true);
        assert!(is_english());
        assert_eq!(locale(), "en");
        assert_eq!(tr("压缩", "Compress"), "Compress");
        set_english(false);
        assert!(!is_english());
        assert_eq!(locale(), "zh-CN");
        assert_eq!(tr("压缩", "Compress"), "压缩");
    }
}
