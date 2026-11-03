//! 自解压运行时的 GPUI 界面：启动窗口、错误窗口和窗口生命周期。

use std::path::PathBuf;

use gpui_kit::base::{StyledExt, h_flex, v_flex};
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::{ActiveTheme as _, TitleBar};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, IntoElement, ParentElement, Render, Styled,
    TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};
use lumark_sfx::{SfxFooter, SfxPlan};
use lumark_ui_shared::assets::AppIcon;
use lumark_ui_shared::chrome::dialog_banner;
use lumark_ui_shared::i18n::{locale, set_english, tr};
use lumark_ui_shared::widgets::button;
use zeroize::Zeroizing;

mod render;
mod view;
mod worker;

#[cfg(test)]
mod tests;

use view::SfxView;

/// 自解压主窗口宽度。
const WINDOW_WIDTH: f32 = 620.;
/// 自解压主窗口高度；需要同时容纳设置表单和完整进度面板。
const WINDOW_HEIGHT: f32 = 540.;

/// 启动 GPUI 应用并打开交互式自解压窗口。
pub(crate) fn run(
    exe: PathBuf,
    footer: SfxFooter,
    plan: SfxPlan,
    default_target: PathBuf,
    initial_password: Option<Zeroizing<String>>,
) {
    apply_language();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let options = window_options(cx);
            let result = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| {
                    SfxView::new(
                        exe,
                        footer,
                        plan,
                        default_target,
                        initial_password,
                        window,
                        cx,
                    )
                })
            });
            if let Err(error) = result {
                eprintln!("无法打开自解压窗口：{error}");
                cx.quit();
            }
        });
}

/// 以独立错误窗口展示无法启动自解压的原因。
pub(crate) fn run_error(message: String) {
    apply_language();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let options = error_window_options(cx);
            let result = gpui_kit::open_window(options, cx, move |_, cx| {
                cx.new(move |_| ErrorView { message })
            });
            if let Err(error) = result {
                eprintln!("无法打开错误窗口：{error}");
                cx.quit();
            }
        });
}

/// 把自解压运行时的语言同步到共享 i18n 和官方组件 locale。
fn apply_language() {
    set_english(crate::lang::is_english());
    gpui_kit::component::set_locale(locale());
}

/// 生成自解压主窗口选项：居中、固定尺寸、透明自绘标题栏。
fn window_options(cx: &mut App) -> WindowOptions {
    let size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let origin = centered_origin(cx, WINDOW_WIDTH, WINDOW_HEIGHT);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size })),
        titlebar: Some(TitlebarOptions {
            title: Some(tr("Lumark 自解压", "Lumark Self-Extractor").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        kind: WindowKind::Normal,
        focus: true,
        show: true,
        is_resizable: false,
        is_minimizable: true,
        is_movable: true,
        window_min_size: Some(size),
        ..TitleBar::window_options()
    }
}

/// 生成错误窗口选项。
fn error_window_options(cx: &mut App) -> WindowOptions {
    let width = 560.;
    let height = 320.;
    let size = size(px(width), px(height));
    let origin = centered_origin(cx, width, height);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size })),
        titlebar: Some(TitlebarOptions {
            title: Some(tr("Lumark 自解压", "Lumark Self-Extractor").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        kind: WindowKind::Normal,
        focus: true,
        show: true,
        is_resizable: false,
        is_minimizable: true,
        is_movable: true,
        window_min_size: Some(size),
        ..TitleBar::window_options()
    }
}

/// 把窗口放在主显示器中央；拿不到显示器时回退到固定位置。
fn centered_origin(cx: &App, width: f32, height: f32) -> gpui_kit::Point<gpui_kit::Pixels> {
    let Some(display) = cx.primary_display() else {
        return point(px(120.), px(120.));
    };
    let bounds = display.bounds();
    point(
        px(bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - width) / 2.0),
        px(bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - height) / 2.0),
    )
}

/// 无法启动自解压时的最小错误窗口。
struct ErrorView {
    message: String,
}

impl Render for ErrorView {
    /// 渲染品牌标题栏、错误文本和关闭按钮。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    .h(px(38.))
                    .bg(cx.theme().title_bar)
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(24.))
                                    .rounded(px(7.))
                                    .bg(cx.theme().button_primary)
                                    .text_color(cx.theme().button_primary_foreground)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(AppIcon::Zip.element(14.)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_semibold()
                                    .child(tr("Lumark 自解压", "Lumark Self-Extractor")),
                            ),
                    ),
            )
            .child(dialog_banner(
                cx,
                54.,
                tr(
                    "自解压包 · 无法启动",
                    "Self-extracting archive · cannot start",
                ),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .px_5()
                    .py_4()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(self.message.clone()),
                    )
                    .child(div().flex_1()),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .justify_end()
                    .p_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        button("sfx-error-close")
                            .primary()
                            .label(tr("关闭", "Close"))
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
