use super::*;

impl ImageViewerWindowView {
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
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Picture.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .text_sm()
                            .font_semibold()
                            .child(format!(
                                "{} - {} - Lumark",
                                self.name,
                                tr("图片查看器", "Image viewer")
                            )),
                    )
                    .when_some(self.meta.as_ref(), |this, meta| {
                        this.child(Tag::secondary().xsmall().child(meta.format.clone()))
                    }),
            )
    }
}
