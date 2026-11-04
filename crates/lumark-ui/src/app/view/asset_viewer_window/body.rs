use super::*;

impl AssetViewerWindowView {
    /// 渲染画布与检查器；正式窗口用可拖拽分栏，测试/无状态路径固定宽度。
    pub(super) fn render_content(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if !self.inspector_open {
            return div()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .child(self.render_body(cx))
                .into_any_element();
        }
        let body = self.render_body(cx);
        let inspector = self.render_inspector(cx);
        if let Some(state) = self.inspector_state.as_ref() {
            gpui_kit::base::h_resizable("viewer-split")
                .with_state(state)
                .child(gpui_kit::base::resizable_panel().child(body))
                .child(
                    gpui_kit::base::resizable_panel()
                        .size(px(320.))
                        .size_range(px(260.)..px(640.))
                        .child(inspector),
                )
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .items_stretch()
                .overflow_hidden()
                .child(body)
                .child(div().w(px(320.)).h_full().flex_shrink_0().child(inspector))
                .into_any_element()
        }
    }

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
                        None if self.rendering => v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(gpui_kit::component::spinner::Spinner::new())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("正在渲染…", "Rendering…")),
                            )
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
                        mode: DragMode::Orbit,
                    });
                }),
            )
            .on_mouse_down(
                gpui_kit::MouseButton::Right,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, _| {
                    this.drag = Some(DragState {
                        last: (event.position.x.as_f32(), event.position.y.as_f32()),
                        mode: DragMode::Pan,
                    });
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    let Some(drag) = this.drag.as_ref() else {
                        return;
                    };
                    let expected = match drag.mode {
                        DragMode::Orbit => gpui_kit::MouseButton::Left,
                        DragMode::Pan => gpui_kit::MouseButton::Right,
                    };
                    if event.pressed_button != Some(expected) {
                        this.drag = None;
                        return;
                    }
                    let position = (event.position.x.as_f32(), event.position.y.as_f32());
                    let dx = position.0 - drag.last.0;
                    let dy = position.1 - drag.last.1;
                    let mode = drag.mode;
                    if let Some(drag) = this.drag.as_mut() {
                        drag.last = position;
                    }
                    match mode {
                        DragMode::Orbit => {
                            (this.yaw, this.pitch) = apply_orbit_drag(this.yaw, this.pitch, dx, dy);
                        }
                        DragMode::Pan => {
                            this.pan = apply_pan_drag(this.pan, dx, dy);
                        }
                    }
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
            .on_mouse_up(
                gpui_kit::MouseButton::Right,
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

/// 右键平移累计：场景跟手移动，屏幕像素直接累加。
pub(super) fn apply_pan_drag(pan: [f32; 2], dx: f32, dy: f32) -> [f32; 2] {
    [pan[0] + dx, pan[1] + dy]
}

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
