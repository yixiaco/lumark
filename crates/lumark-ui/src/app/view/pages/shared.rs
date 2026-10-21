//! 页面共享的紧凑指标卡片组件。
use super::super::super::*;
use super::super::components::{AppIcon, card};

/// 渲染内容页上方的紧凑指标卡片。
pub(super) fn stat_card(
    cx: &Context<LumarkApp>,
    icon: AppIcon,
    label: &str,
    value: &str,
) -> impl IntoElement {
    card(cx).flex_1().p_4().child(
        h_flex()
            .gap_3()
            .items_center()
            .child(
                div()
                    .size(px(36.))
                    .rounded(px(10.))
                    .bg(cx.theme().primary.opacity(0.12))
                    .text_color(cx.theme().primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.element(18.)),
            )
            .child(
                v_flex()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    )
                    .child(div().text_lg().font_semibold().child(value.to_string())),
            ),
    )
}
