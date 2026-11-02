use super::*;

impl ImageViewerWindowView {
    pub(super) fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let dimensions = self
            .meta
            .as_ref()
            .map(|meta| {
                if meta.vector {
                    tr("矢量图", "Vector").to_string()
                } else {
                    format!("{} × {}", meta.width, meta.height)
                }
            })
            .unwrap_or_else(|| tr("未知尺寸", "Unknown size").to_string());
        let format = self
            .meta
            .as_ref()
            .map(|meta| meta.format.clone())
            .unwrap_or_else(|| {
                std::path::Path::new(&self.path_text)
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_ascii_uppercase())
                    .unwrap_or_default()
            });
        h_flex()
            .h(px(26.))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .child(self.path_text.clone()),
            )
            .child(dimensions)
            .child(format)
            .when_some(
                self.meta.as_ref().filter(|meta| meta.animated),
                |this, _| this.child(Tag::secondary().xsmall().child(tr("动画", "Animated"))),
            )
            .when_some(self.orientation.as_ref(), |this, label| {
                this.child(Tag::secondary().xsmall().child(label.clone()))
            })
            .when(self.rotation != 0 || self.notice.is_some(), |this| {
                let label = match &self.notice {
                    Some(notice) => notice.clone(),
                    None => format!("{} {}°", tr("旋转", "Rotated"), self.rotation as u32 * 90),
                };
                this.child(
                    button("image-rotation")
                        .ghost()
                        .xsmall()
                        .label(label)
                        .tooltip(tr(
                            "点击恢复原方向",
                            "Click to restore the original orientation",
                        ))
                        .accessibility_label(tr("恢复图片方向", "Restore image orientation"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.rotation != 0 {
                                let delta = -(this.rotation as i32);
                                this.rotate(delta, cx);
                            }
                            this.notice = None;
                            cx.notify();
                        })),
                )
            })
            .child(format_size(self.size))
            .child(if self.fit {
                tr("适应窗口", "Fit").to_string()
            } else {
                format!("{}%", self.zoom_percent())
            })
    }
}
