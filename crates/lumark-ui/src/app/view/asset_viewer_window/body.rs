use super::*;

impl AssetViewerWindowView {
    pub(super) fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if let Some(error) = &self.error {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .text_sm()
                .text_color(StatusTone::Danger.color(cx))
                .child(error.clone())
                .into_any_element();
        }
        div()
            .id("model-viewer-canvas")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(match &self.image {
                Some(image) => gpui_kit::img(image.clone())
                    .size_full()
                    .object_fit(gpui_kit::ObjectFit::Contain)
                    .into_any_element(),
                None => div().size_full().into_any_element(),
            })
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, _| {
                    this.drag = Some(DragState {
                        last: (event.position.x.as_f32(), event.position.y.as_f32()),
                    });
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                        this.drag = None;
                        return;
                    }
                    let Some(drag) = this.drag.as_mut() else {
                        return;
                    };
                    let position = (event.position.x.as_f32(), event.position.y.as_f32());
                    let dx = position.0 - drag.last.0;
                    let dy = position.1 - drag.last.1;
                    drag.last = position;
                    this.yaw = (this.yaw + dx * 0.4).rem_euclid(360.0);
                    this.pitch = (this.pitch + dy * 0.4).clamp(-89.0, 89.0);
                    this.render_preview();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .into_any_element()
    }
}
