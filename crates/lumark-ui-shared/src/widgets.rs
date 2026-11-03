//! 主程序与自解压运行时共用的基础控件和布局辅助。

use gpui_kit::base::{StyledExt, h_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::Button;
use gpui_kit::{App, CursorStyle, Div, ElementId, IntoElement, ParentElement, Styled, div, px};

/// 创建统一使用指针手的按钮。
pub fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id).cursor(CursorStyle::PointingHand)
}

/// 渲染左标签右控件的经典表单行。
pub fn classic_field(label: &str, width: f32, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_3()
        .items_center()
        .child(
            div()
                .w(px(width))
                .flex_shrink_0()
                .text_sm()
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// 渲染输入框上方的小号字段标题。
pub fn form_label(cx: &App, label: &str) -> Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(label.to_string())
}

/// 渲染一行小号辅助说明文字。
pub fn hint_text(cx: &App, text: impl Into<String>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
