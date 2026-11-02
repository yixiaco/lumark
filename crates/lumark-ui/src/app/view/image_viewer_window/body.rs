use super::*;

impl ImageViewerWindowView {
    pub(super) fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if self.loading {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(cx.theme().background)
                .child(gpui_kit::component::spinner::Spinner::new())
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("正在读取图片…", "Reading image…")),
                )
                .into_any_element();
        }
        if let Some(error) = &self.error {
            return Empty::new()
                .flex_1()
                .header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(AppIcon::Picture.element(17.)),
                        )
                        .title(EmptyTitle::new().child(tr("无法显示图片", "Cannot show image")))
                        .description(EmptyDescription::new().child(error.clone())),
                )
                .into_any_element();
        }

        let (natural_w, natural_h) = self.natural_size();
        let content = if self.fit {
            gpui_kit::img(self.display_path.clone())
                .size_full()
                .object_fit(gpui_kit::ObjectFit::Contain)
                .into_any_element()
        } else {
            gpui_kit::img(self.display_path.clone())
                .w(px((natural_w * self.zoom).max(1.0)))
                .h(px((natural_h * self.zoom).max(1.0)))
                .ml(px(self.pan.0))
                .mt(px(self.pan.1))
                .object_fit(gpui_kit::ObjectFit::Fill)
                .into_any_element()
        };
        div()
            .id("image-canvas")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .flex()
            .items_center()
            .justify_center()
            .child(content)
            .on_scroll(
                cx.listener(|this, event: &gpui_kit::ScrollEvent, window, cx| {
                    let delta = wheel_delta_y(event.delta);
                    let zoom = wheel_zoom(this.zoom, delta);
                    if (zoom - this.zoom).abs() > f32::EPSILON {
                        let old_zoom = this.zoom;
                        this.fit = false;
                        if let Some(cursor) = this.cursor {
                            let bounds = window.inner_window_bounds().get_bounds();
                            let center = wheel_center((
                                bounds.size.width.as_f32(),
                                bounds.size.height.as_f32(),
                            ));
                            this.pan = anchored_pan(this.pan, cursor, center, old_zoom, zoom);
                        }
                        this.zoom = zoom;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, _| {
                    if this.fit {
                        return;
                    }
                    this.drag_pan = Some((event.position.x.as_f32(), event.position.y.as_f32()));
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    this.cursor = Some((event.position.x.as_f32(), event.position.y.as_f32()));
                    if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                        this.drag_pan = None;
                        return;
                    }
                    let Some(last) = this.drag_pan else {
                        return;
                    };
                    let position = (event.position.x.as_f32(), event.position.y.as_f32());
                    this.pan.0 += position.0 - last.0;
                    this.pan.1 += position.1 - last.1;
                    this.drag_pan = Some(position);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.drag_pan = None;
                    cx.notify();
                }),
            )
            .into_any_element()
    }
}
