//! 主程序界面基座。
//!
//! 这里保留主题、语言、品牌资源、通用控件、进度面板和格式化函数；页面和
//! 业务状态留在 `lumark-ui`。自解压运行时使用原生 Win32 UI，不依赖本 crate，
//! 避免把 GPUI/DirectX 链接进 stub 后触发 360 QVM 误报。

pub mod assets;
pub mod chrome;
pub mod format;
pub mod i18n;
pub mod progress;
pub mod theme;
pub mod widgets;

pub use assets::{AppIcon, brand_mark};
pub use chrome::{banner_gradient, brand_lockup, dialog_banner};
pub use format::{format_duration, format_size};
pub use i18n::{is_english, locale, set_english, tr};
pub use progress::{ProgressKind, ProgressPanel, ProgressSnapshot, ProgressState};
pub use theme::{
    StatusTone, apply_theme, apply_theme_accent, hsla_to_hex, theme_accent_presets,
    theme_mode_for_appearance,
};
pub use widgets::{button, classic_field, form_label, hint_text};
