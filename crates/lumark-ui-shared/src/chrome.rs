//! 主程序与自解压运行时共用的品牌横幅。

use gpui_kit::base::{StyledExt, h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::assets::brand_mark;
use crate::i18n::tr;

/// 返回从主题强调色浅色过渡到页面底色的横幅背景。
///
/// 渐变只取当前主题的主色和中性表面色，浅色和深色模式各自保持可读对比，
/// 用户修改主题色后动作带和对话框横幅会同步换色。
pub fn banner_gradient(cx: &App) -> gpui_kit::Div {
    let from = cx.theme().primary.opacity(if cx.theme().mode.is_dark() {
        0.18
    } else {
        0.10
    });
    let to = cx.theme().background;
    div().bg(gpui_kit::linear_gradient(
        110.,
        gpui_kit::linear_color_stop(from, 0.),
        gpui_kit::linear_color_stop(to, 1.),
    ))
}

/// 渲染标题栏、动作带和对话框共用的品牌标识。
pub fn brand_lockup(cx: &App, slogan: bool) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_2()
        .child(brand_mark(34.))
        .child(
            v_flex()
                .gap(px(0.))
                .child(
                    div()
                        .text_base()
                        .font_semibold()
                        .whitespace_nowrap()
                        .child("Lumark"),
                )
                .when(slogan, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .whitespace_nowrap()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(
                                "高效专业 · 值得信赖",
                                "Efficient, professional, trustworthy",
                            )),
                    )
                }),
        )
}

/// 渲染压缩、解压和自解压窗口顶部的品牌横幅。
///
/// `tagline` 由调用方提供，主程序与自解压运行时可以各自表达当前任务。
pub fn dialog_banner(cx: &App, height: f32, tagline: impl Into<SharedString>) -> impl IntoElement {
    banner_gradient(cx)
        .w_full()
        .h(px(height))
        .px_5()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(brand_lockup(cx, false))
        .child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child(tagline.into()),
        )
}
