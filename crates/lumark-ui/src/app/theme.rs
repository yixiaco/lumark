//! 主题模式与强调色的应用入口。
//!
//! 用户偏好只保存在设置中，中性的表面色、文字色以及从强调色派生的语义
//! token 集中在这里维护，保证主窗口、设置窗口和后续新窗口保持一致。

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{Context, Window, hsla, px};
use lumark_settings::ThemePreference;

use super::LumarkApp;

/// 默认强调色：蓝色。
const DEFAULT_ACCENT_COLOR: &str = "#2563eb";

/// 在 gpui-kit 基础主题上应用 Lumark 的中性表面和用户强调色。
///
/// 语义颜色会同步刷新，确保原生控件与自定义表面在浅色和深色模式之间保持一致；
/// 强调色来自设置，默认蓝色，修改后无需重启即可刷新所有窗口。
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
    Theme::update(cx, |theme| {
        let dark = theme.mode.is_dark();
        let background = if dark {
            hsla(217. / 360., 0.25, 0.08, 1.)
        } else {
            hsla(220. / 360., 0.33, 0.97, 1.)
        };
        let surface = if dark {
            hsla(217. / 360., 0.22, 0.12, 1.)
        } else {
            hsla(0., 0., 1., 1.)
        };
        let foreground = if dark {
            hsla(214. / 360., 0.24, 0.92, 1.)
        } else {
            hsla(220. / 360., 0.27, 0.15, 1.)
        };
        let muted_foreground = if dark {
            hsla(215. / 360., 0.12, 0.62, 1.)
        } else {
            hsla(217. / 360., 0.12, 0.47, 1.)
        };
        let border = if dark {
            hsla(216. / 360., 0.18, 0.20, 1.)
        } else {
            hsla(214. / 360., 0.32, 0.91, 1.)
        };

        theme.background = background;
        theme.foreground = foreground;
        theme.popover = surface;
        theme.popover_foreground = foreground;
        theme.border = border;
        theme.input = border;
        theme.muted = if dark {
            hsla(217. / 360., 0.19, 0.16, 1.)
        } else {
            hsla(218. / 360., 0.32, 0.96, 1.)
        };
        theme.muted_foreground = muted_foreground;
        theme.secondary = theme.muted;
        theme.secondary_hover = if dark {
            hsla(217. / 360., 0.18, 0.20, 1.)
        } else {
            hsla(216. / 360., 0.29, 0.93, 1.)
        };
        theme.secondary_active = if dark {
            hsla(217. / 360., 0.17, 0.24, 1.)
        } else {
            hsla(216. / 360., 0.27, 0.90, 1.)
        };
        theme.secondary_foreground = foreground;
        theme.button = if dark {
            hsla(217. / 360., 0.19, 0.16, 1.)
        } else {
            hsla(0., 0., 1., 1.)
        };
        theme.button_hover = theme.secondary_hover;
        theme.button_active = theme.secondary_active;
        theme.button_foreground = foreground;
        theme.title_bar = if dark {
            hsla(217. / 360., 0.25, 0.09, 1.)
        } else {
            hsla(0., 0., 1., 1.)
        };
        theme.title_bar_border = border;
        theme.sidebar = if dark {
            hsla(217. / 360., 0.24, 0.10, 1.)
        } else {
            hsla(220. / 360., 0.28, 0.985, 1.)
        };
        theme.sidebar_foreground = foreground;
        theme.sidebar_border = border;
        theme.sidebar_primary = if dark {
            hsla(217. / 360., 0.20, 0.15, 1.)
        } else {
            hsla(218. / 360., 0.31, 0.955, 1.)
        };
        theme.list_hover = if dark {
            hsla(217. / 360., 0.19, 0.16, 1.)
        } else {
            hsla(218. / 360., 0.32, 0.955, 1.)
        };
        theme.list_head = if dark {
            hsla(217. / 360., 0.21, 0.13, 1.)
        } else {
            hsla(218. / 360., 0.30, 0.975, 1.)
        };
        theme.table = surface;
        theme.table_head = theme.list_head;
        theme.table_hover = theme.list_hover;
        theme.status_bar = theme.title_bar;
        theme.status_bar_border = border;
        theme.group_box = surface;
        theme.group_box_foreground = foreground;
        theme.overlay = hsla(220. / 360., 0.22, 0.08, 0.40);
        theme.success = if dark {
            hsla(156. / 360., 0.45, 0.20, 1.)
        } else {
            hsla(151. / 360., 0.52, 0.91, 1.)
        };
        theme.success_foreground = if dark {
            hsla(151. / 360., 0.60, 0.72, 1.)
        } else {
            hsla(152. / 360., 0.62, 0.27, 1.)
        };
        theme.warning = if dark {
            hsla(38. / 360., 0.50, 0.20, 1.)
        } else {
            hsla(40. / 360., 0.84, 0.91, 1.)
        };
        theme.warning_foreground = if dark {
            hsla(41. / 360., 0.86, 0.70, 1.)
        } else {
            hsla(33. / 360., 0.75, 0.30, 1.)
        };
        theme.danger = if dark {
            hsla(356. / 360., 0.52, 0.22, 1.)
        } else {
            hsla(356. / 360., 0.78, 0.94, 1.)
        };
        theme.danger_foreground = if dark {
            hsla(356. / 360., 0.84, 0.74, 1.)
        } else {
            hsla(356. / 360., 0.66, 0.38, 1.)
        };
        theme.radius = px(6.);
        theme.radius_lg = px(14.);
        theme.font_size = px(13.5);
        theme.shadow = true;
        theme.scrollbar_mode = gpui_kit::component::scroll::ScrollbarMode::Always;
        apply_accent_color(theme, accent_color);
    });
}

/// 只刷新强调色相关的语义 token，供设置页颜色选择器实时调用。
pub(crate) fn apply_theme_accent(accent_color: &str, cx: &mut Context<LumarkApp>) {
    Theme::update(cx, |theme| apply_accent_color(theme, accent_color));
}

/// 从用户选定的主题色派生主色、悬停色、选中底色和前景色。
///
/// 只保留用户选择的色相和饱和度，明度按浅色/深色模式收敛到可读区间，
/// 再用相对亮度决定主按钮文字使用白色还是深色，避免浅黄色等主题失去对比度。
fn apply_accent_color(theme: &mut Theme, accent_color: &str) {
    let dark = theme.mode.is_dark();
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

    theme.primary = primary;
    theme.primary_hover = primary_hover;
    theme.primary_active = primary_active;
    theme.primary_foreground = primary_foreground;
    theme.button_primary = primary;
    theme.button_primary_hover = primary_hover;
    theme.button_primary_active = primary_active;
    theme.button_primary_foreground = primary_foreground;
    theme.accent = accent;
    theme.accent_foreground = accent_foreground;
    theme.selection = accent;
    theme.ring = primary;
    theme.link = primary;
    theme.link_hover = primary_hover;
    theme.link_active = primary_active;
    theme.list_active = accent;
    theme.list_active_border = primary;
    theme.sidebar_accent = accent;
    theme.sidebar_accent_foreground = accent_foreground;
    theme.table_active = accent;
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
