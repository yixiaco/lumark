//! 压缩窗口标题栏和底部按钮区。

use super::*;
use crate::i18n::tr;

impl CompressWindowView {
    /// 渲染窗口标题栏。
    pub(super) fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
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
                            .child(AppIcon::Compression.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_semibold()
                            .child(tr("压缩文件 - Lumark", "Compress - Lumark")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} {}", self.sources.len(), tr("个项目", "items"))),
                    ),
            )
    }

    /// 渲染所有布局共用的底部操作栏。
    pub(super) fn render_footer(&self, cx: &Context<Self>, classic: bool) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .p_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                button("compress-window-switch")
                    .ghost()
                    .icon(AppIcon::Tool.element(15.))
                    .label(if classic {
                        "切换至轻巧模式"
                    } else {
                        "切换至经典模式"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.switch_layout(window, cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                button("compress-window-cancel")
                    .label(tr("取消", "Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close(window, cx);
                    })),
            )
            .child(
                button("compress-window-start")
                    .primary()
                    .icon(AppIcon::Compression.element(15.))
                    .label(if classic { "确定" } else { "立即压缩" })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start(window, cx);
                    })),
            )
    }
}
