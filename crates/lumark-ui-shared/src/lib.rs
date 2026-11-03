//! 主程序与自解压运行时共用的界面基座。
//!
//! 这里只保留两端都会用到的主题、语言、品牌资源、通用控件、进度面板和
//! 格式化函数；页面、动作和自解压运行时的业务状态仍留在各自 crate。
//! 自解压运行时只依赖本 crate 与 `gpui-kit`，不得反向依赖 `lumark-ui`。

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
