//! 主题模式与强调色的应用入口。
//!
//! 主题 token 和强调色派生位于 `lumark-ui-shared`，主程序只负责把设置中的
//! 主题偏好和窗口外观转换成 `ThemeMode`，再交给共享主题实现。

use gpui_kit::component::ThemeMode;
use gpui_kit::{Context, Window};
use lumark_settings::ThemePreference;

use super::LumarkApp;

pub(crate) use lumark_ui_shared::theme::{
    StatusTone, apply_theme_accent, hsla_to_hex, theme_accent_presets,
};

/// 按设置偏好应用主题，并同步强调色、圆角、字号和滚动条。
pub(crate) fn apply_theme(
    preference: ThemePreference,
    accent_color: &str,
    window: &mut Window,
    cx: &mut Context<LumarkApp>,
) {
    let mode = match preference {
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::System => lumark_ui_shared::theme_mode_for_appearance(window.appearance()),
    };
    lumark_ui_shared::theme::apply_theme(mode, accent_color, window, cx);
}
