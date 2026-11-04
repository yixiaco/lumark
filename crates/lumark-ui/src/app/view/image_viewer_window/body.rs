use super::*;

impl ImageViewerWindowView {
    /// 渲染图片画布：适应窗口时用 Contain 铺满，其余模式用绝对定位的等比图片。
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

        let natural = self.natural_size();
        let canvas_size = self
            .canvas_metrics()
            .map(|canvas| canvas.size)
            .unwrap_or((0.0, 0.0));
        let image = if self.fit {
            // 适应窗口保持 Contain：图片铺满舞台并按比例留边，不进入绝对布局。
            gpui_kit::img(self.display_path.clone())
                .size_full()
                .object_fit(gpui_kit::ObjectFit::Contain)
                .into_any_element()
        } else {
            let rect = image_rect(natural, canvas_size, self.zoom, self.pan);
            // 绝对定位 + 固定宽高：图片不参与 flex 收缩，宽高严格按缩放比例，
            // 靠近窗口边缘时也不会被压扁。
            gpui_kit::img(self.display_path.clone())
                .absolute()
                .left(px(rect.left))
                .top(px(rect.top))
                .w(px(rect.width))
                .h(px(rect.height))
                .debug_selector(|| "image-viewer-image".into())
                .flex_shrink_0()
                .object_fit(gpui_kit::ObjectFit::Fill)
                .into_any_element()
        };

        let canvas_bounds = self.canvas_bounds.clone();
        // 手动缩放模式的绝对布局依赖画布中心；窗口尺寸变化时本帧 render 只能
        // 拿到上一帧 bounds，因此尺寸变化后补一帧，让下一帧按新中心重新布局。
        let relayout_on_resize = !self.fit;
        div()
            .on_children_prepainted(move |children, window, _cx| {
                let Some(bounds) = children.first().copied() else {
                    return;
                };
                let changed = canvas_bounds.get() != Some(bounds);
                canvas_bounds.set(Some(bounds));
                if changed && relayout_on_resize {
                    window.request_animation_frame();
                }
            })
            .id("image-canvas")
            .debug_selector(|| "image-viewer-canvas".into())
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(
                // 全尺寸相对舞台：画布 bounds 即舞台 bounds，绝对定位图片以它定位。
                div().relative().size_full().child(image),
            )
            .map(|this| {
                if self.fit {
                    this.cursor_default()
                } else if self.drag.is_some() {
                    this.cursor_grabbing()
                } else {
                    this.cursor_grab()
                }
            })
            .on_scroll(cx.listener(|this, event: &gpui_kit::ScrollEvent, _, cx| {
                let cursor = (event.position.x.as_f32(), event.position.y.as_f32());
                this.zoom_by_wheel(wheel_delta_y(event.delta), cursor, cx);
            }))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, cx| {
                    if this.fit {
                        return;
                    }
                    // 记录按下时的绝对位置与平移值，拖动过程中始终按绝对坐标计算。
                    this.drag = Some(DragPan {
                        start: (event.position.x.as_f32(), event.position.y.as_f32()),
                        base: this.pan,
                    });
                    cx.notify();
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                        if this.drag.take().is_some() {
                            cx.notify();
                        }
                        return;
                    }
                    let Some(drag) = this.drag else {
                        return;
                    };
                    let cursor = (event.position.x.as_f32(), event.position.y.as_f32());
                    let pan = dragged_pan(drag.start, drag.base, cursor);
                    if pan != this.pan {
                        this.pan = pan;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    if this.drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }
}
