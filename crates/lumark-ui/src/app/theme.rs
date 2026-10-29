//! 主题模式与强调色的应用入口。
//!
//! 主题颜色以 gpui-kit 官方 `ThemeColor` 调色板为基线，只在基线上覆盖
//! 强调色相关字段，再一次性写回 `Theme::colors`；`Theme::update` 负责
//! 同步语义 token、Base 投影和所有窗口，页面不得自行拼装颜色。

use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeColor, ThemeMode};
use gpui_kit::{Context, Window, hsla, px};
use lumark_settings::ThemePreference;

use super::LumarkApp;

/// 默认强调色：蓝色。
const DEFAULT_ACCENT_COLOR: &str = "#2563eb";

/// 普通中性表面上的状态语义色。
///
/// gpui-kit 的 `success_foreground`、`warning_foreground` 和
/// `danger_foreground` 表示彩色状态底面上的文字色，浅色主题下接近白色，
/// 不能直接当作普通页面上的图标或文字颜色。这里按当前主题模式派生状态色：
/// 图标使用状态色并在浅色主题下略微压暗，正文文字进一步压暗以保证对比度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusTone {
    /// 成功或完成状态。
    Success,
    /// 警告或暂停状态。
    Warning,
    /// 危险或失败状态。
    Danger,
}

impl StatusTone {
    /// 返回用于图标和装饰性强调的状态色。
    pub(crate) fn color(self, cx: &gpui_kit::App) -> gpui_kit::Hsla {
        let color = match self {
            Self::Success => cx.theme().success,
            Self::Warning => cx.theme().warning,
            Self::Danger => cx.theme().danger,
        };
        if cx.theme().mode.is_dark() {
            return color;
        }
        let ceiling = match self {
            Self::Success => 0.40,
            Self::Warning => 0.42,
            Self::Danger => 0.50,
        };
        hsla(color.h, color.s, color.l.min(ceiling), color.a)
    }

    /// 返回普通中性表面上可读的状态文字色。
    pub(crate) fn text_color(self, cx: &gpui_kit::App) -> gpui_kit::Hsla {
        let color = self.color(cx);
        if cx.theme().mode.is_dark() {
            return color;
        }
        let lightness = match self {
            Self::Success | Self::Warning => 0.27,
            Self::Danger => 0.38,
        };
        hsla(color.h, color.s, color.l.min(lightness), color.a)
    }
}

/// 在官方主题基线上应用 Lumark 的强调色。
///
/// 中性色、语义状态色和组件 token 全部来自 `ThemeColor::light()/dark()`；
/// 这里只覆盖品牌强调色及其派生项，避免手写 token 与官方组件投影漂移。
pub(crate) fn apply_theme(
    preference: ThemePreference,
    accent_color: &str,
    window: &mut Window,
    cx: &mut Context<LumarkApp>,
) {
    let mode = match preference {
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::System => match window.appearance() {
            gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark => {
                ThemeMode::Dark
            }
            _ => ThemeMode::Light,
        },
    };
    Theme::change(mode, Some(window), cx);
    let palette = accent_palette(mode, accent_color);
    Theme::update(cx, |theme| {
        theme.colors = palette;
        theme.radius = px(6.);
        theme.radius_lg = px(14.);
        theme.font_size = px(13.5);
        theme.shadow = true;
    });
    Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);
}

/// 只刷新强调色相关的官方主题字段，供设置页颜色选择器实时调用。
pub(crate) fn apply_theme_accent(accent_color: &str, cx: &mut Context<LumarkApp>) {
    let mode = cx.theme().mode;
    let palette = accent_palette(mode, accent_color);
    Theme::update(cx, |theme| {
        theme.colors = palette;
    });
}

/// 在官方调色板基线上生成强调色 palette。
///
/// 只保留用户选择的色相和饱和度，明度按浅色/深色模式收敛到可读区间，
/// 再用相对亮度决定主按钮文字使用白色还是深色，避免浅黄色等主题失去对比度。
/// 未覆盖的字段继续使用 gpui-kit 官方默认值。
fn accent_palette(mode: ThemeMode, accent_color: &str) -> ThemeColor {
    let mut palette = if mode.is_dark() {
        *ThemeColor::dark()
    } else {
        *ThemeColor::light()
    };
    let dark = mode.is_dark();
    let base = gpui_kit::component::try_parse_color(accent_color)
        .or_else(|_| gpui_kit::component::try_parse_color(DEFAULT_ACCENT_COLOR))
        .unwrap_or_else(|_| hsla(221. / 360., 0.83, 0.53, 1.));
    let saturation = base.s.clamp(0.12, 0.92);
    let lightness = if dark {
        base.l.clamp(0.56, 0.76)
    } else {
        base.l.clamp(0.34, 0.68)
    };
    let primary = hsla(base.h, saturation, lightness, 1.);
    let primary_hover = hsla(
        base.h,
        saturation,
        (lightness + if dark { 0.05 } else { 0.06 }).min(0.82),
        1.,
    );
    let primary_active = hsla(
        base.h,
        saturation,
        (lightness - if dark { 0.05 } else { 0.07 }).max(0.24),
        1.,
    );
    let rgba = gpui_kit::Rgba::from(primary);
    let luminance = 0.2126 * rgba.r + 0.7152 * rgba.g + 0.0722 * rgba.b;
    let primary_foreground = if luminance > 0.56 {
        hsla(base.h, 0.25, 0.12, 1.)
    } else {
        hsla(0., 0., 1., 1.)
    };
    let accent = if dark {
        hsla(base.h, (saturation * 0.38).clamp(0.12, 0.36), 0.19, 1.)
    } else {
        hsla(base.h, (saturation * 0.50).clamp(0.18, 0.52), 0.94, 1.)
    };
    let accent_foreground = if dark {
        hsla(base.h, (saturation * 0.55).clamp(0.20, 0.55), 0.80, 1.)
    } else {
        hsla(base.h, (saturation * 0.70).clamp(0.25, 0.75), 0.32, 1.)
    };

    palette.primary = primary;
    palette.primary_hover = primary_hover;
    palette.primary_active = primary_active;
    palette.primary_foreground = primary_foreground;
    palette.button_primary = primary;
    palette.button_primary_hover = primary_hover;
    palette.button_primary_active = primary_active;
    palette.button_primary_foreground = primary_foreground;
    palette.accent = accent;
    palette.accent_foreground = accent_foreground;
    palette.ring = primary;
    palette.selection = accent;
    palette.link = primary;
    palette.link_hover = primary_hover;
    palette.link_active = primary_active;
    palette.list_active = accent;
    palette.list_active_border = primary;
    palette.sidebar_accent = accent;
    palette.sidebar_accent_foreground = accent_foreground;
    palette.table_active = accent;
    palette.table_active_border = primary;
    palette.switch = primary;
    palette.slider_thumb = primary;
    palette.progress_bar = primary;
    palette
}

/// 将颜色选择器返回的 Hsla 转成可持久化的十六进制字符串。
pub(crate) fn hsla_to_hex(color: gpui_kit::Hsla) -> String {
    let rgba = gpui_kit::Rgba::from(color);
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r.clamp(0., 1.) * 255.).round() as u8,
        (rgba.g.clamp(0., 1.) * 255.).round() as u8,
        (rgba.b.clamp(0., 1.) * 255.).round() as u8
    )
}

/// 设置页颜色选择器提供的快捷主题色。
pub(crate) fn theme_accent_presets() -> Vec<gpui_kit::Hsla> {
    [
        "#2563eb", "#0d9488", "#16a34a", "#7c3aed", "#ea580c", "#e11d48",
    ]
    .iter()
    .filter_map(|hex| gpui_kit::component::try_parse_color(hex).ok())
    .collect()
}
