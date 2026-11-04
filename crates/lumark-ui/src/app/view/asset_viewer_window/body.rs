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
        // 用 `img` 的 Contain 显示渲染结果；外层画布负责裁剪与尺寸约束，
        // 保证图像只在画布范围内绘制，不会覆盖工具栏/状态栏。
        div()
            .id("model-viewer-canvas")
            .debug_selector(|| "model-viewer-canvas".into())
            .flex_1()
            .h_full()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(
                div()
                    .debug_selector(|| "model-viewer-image-container".into())
                    .size_full()
                    .overflow_hidden()
                    .child(match &self.image {
                        Some(image) => gpui_kit::img(image.clone())
                            .size_full()
                            .object_fit(gpui_kit::ObjectFit::Contain)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    }),
            )
            .on_scroll(cx.listener(|this, event: &gpui_kit::ScrollEvent, _, cx| {
                let delta = match event.delta {
                    gpui_kit::ScrollDelta::Pixels(point) => point.y.as_f32(),
                    gpui_kit::ScrollDelta::Lines(point) => point.y * 40.0,
                };
                if delta.abs() < 0.01 {
                    return;
                }
                // 滚轮向上（delta > 0）放大，与 FModel 查看器一致。
                let factor = (delta * 0.0025).exp();
                this.zoom = (this.zoom * factor).clamp(0.2, 6.0);
                this.request_preview(cx);
                cx.notify();
            }))
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
                    (this.yaw, this.pitch) = apply_orbit_drag(this.yaw, this.pitch, dx, dy);
                    this.request_preview(cx);
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

/// 轨道拖拽灵敏度（度/像素）。
pub(super) const ORBIT_SENSITIVITY: f32 = 0.4;

/// 按鼠标拖拽更新轨道相机角度。
///
/// 水平方向与 FModel Snooper `Camera.Modify` 保持一致：鼠标向右拖拽时相机
/// 绕竖直轴的负方向旋转（即鼠标拖拽方向与当前实现原来是反的）；俯仰方向
/// 向下拖拽抬高相机（看到模型顶部）。
pub(super) fn apply_orbit_drag(yaw: f32, pitch: f32, dx: f32, dy: f32) -> (f32, f32) {
    (
        (yaw - dx * ORBIT_SENSITIVITY).rem_euclid(360.0),
        (pitch + dy * ORBIT_SENSITIVITY).clamp(-89.0, 89.0),
    )
}
