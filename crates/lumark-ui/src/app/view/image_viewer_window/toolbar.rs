use super::*;

impl ImageViewerWindowView {
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                button("image-fit")
                    .ghost()
                    .selected(self.fit)
                    .label(tr("适应窗口", "Fit"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_view();
                        cx.notify();
                    })),
            )
            .child(
                button("image-actual")
                    .ghost()
                    .selected(!self.fit && (self.zoom - 1.0).abs() < f32::EPSILON)
                    .label("100%")
                    .on_click(cx.listener(|this, _, _, cx| this.actual_size(cx))),
            )
            .child(
                button("image-zoom-out")
                    .ghost()
                    .icon(AppIcon::ZoomOut.element(16.))
                    .tooltip(tr("缩小", "Zoom out"))
                    .accessibility_label(tr("缩小", "Zoom out"))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by_factor(1.0 / 1.25, cx))),
            )
            .child(
                button("image-zoom-in")
                    .ghost()
                    .icon(AppIcon::ZoomIn.element(16.))
                    .tooltip(tr("放大", "Zoom in"))
                    .accessibility_label(tr("放大", "Zoom in"))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by_factor(1.25, cx))),
            )
            .child(
                button("image-rotate-left")
                    .ghost()
                    .label(tr("左转", "Rotate left"))
                    .tooltip(tr("向左旋转 90°", "Rotate left 90°"))
                    .accessibility_label(tr("向左旋转", "Rotate left"))
                    .on_click(cx.listener(|this, _, _, cx| this.rotate(-1, cx))),
            )
            .child(
                button("image-rotate-right")
                    .ghost()
                    .label(tr("右转", "Rotate right"))
                    .tooltip(tr("向右旋转 90°", "Rotate right 90°"))
                    .accessibility_label(tr("向右旋转", "Rotate right"))
                    .on_click(cx.listener(|this, _, _, cx| this.rotate(1, cx))),
            )
            .child(
                button("image-reset")
                    .ghost()
                    .icon(AppIcon::Refresh.element(13.))
                    .label(tr("重置", "Reset"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_view();
                        if this.rotation != 0 {
                            let delta = -(this.rotation as i32);
                            this.rotate(delta, cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.fit {
                        tr("适应窗口", "Fit to window").to_string()
                    } else {
                        format!("{}%", self.zoom_percent())
                    }),
            )
    }
}
