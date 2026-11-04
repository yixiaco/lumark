//! 内置图片查看器窗口。
//!
//! 图片由 GPUI 的 `img` 元素解码和绘制；这里只负责选择文件、预检尺寸、
//! 适应/缩放和平移。图片只读，不写回原文件或归档。
//!
//! 非适应模式采用「全尺寸舞台 + 绝对定位图片」布局：图片尺寸只由原图尺寸和
//! 缩放决定，位置由画布中心与平移偏移计算，不参与 flex 收缩，避免非等比压扁。

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::component::TitleBar;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::tag::Tag;
use std::cell::Cell;
use std::rc::Rc;

/// 解码前预检得到的图片元数据。
#[derive(Clone, Debug)]
struct ImageMeta {
    width: u32,
    height: u32,
    format: String,
    animated: bool,
    vector: bool,
}

/// 画布在窗口中的精确边界缓存：每帧 prepaint 时由画布的舞台子元素 bounds 写入。
type CanvasBoundsCell = Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>;

/// 左键拖拽的锚点：按下时的绝对鼠标位置与当时的平移偏移。
#[derive(Clone, Copy, Debug)]
struct DragPan {
    /// 鼠标按下时的窗口坐标。
    start: (f32, f32),
    /// 鼠标按下时的平移偏移。
    base: (f32, f32),
}

/// 图片查看器窗口。
pub(crate) struct ImageViewerWindowView {
    name: String,
    path_text: String,
    /// 实际用于绘制的图片路径；EXIF 方向校正后是临时 PNG。
    display_path: std::path::PathBuf,
    /// EXIF 校正生成的临时文件，窗口关闭时删除。
    temporary: Option<std::path::PathBuf>,
    /// EXIF 方向说明。
    orientation: Option<String>,
    size: u64,
    meta: Option<ImageMeta>,
    loading: bool,
    error: Option<String>,
    fit: bool,
    zoom: f32,
    /// 手动缩放后的平移偏移（像素），以画布中心为基准。
    pan: (f32, f32),
    /// 左键拖拽状态；仅手动缩放模式存在。
    drag: Option<DragPan>,
    /// 最近一帧画布的精确边界，用于绝对布局和滚轮锚点。
    canvas_bounds: CanvasBoundsCell,
    /// 用户手动旋转的 90° 次数（0–3）。
    rotation: u8,
    /// 是否正在后台旋转图片。
    rotating: bool,
    /// 旋转失败等提示。
    notice: Option<String>,
}

impl Drop for ImageViewerWindowView {
    /// 删除 EXIF 方向校正生成的临时图片。
    fn drop(&mut self) {
        if let Some(temporary) = self.temporary.take() {
            let _ = std::fs::remove_file(temporary);
        }
    }
}

impl ImageViewerWindowView {
    /// 创建只读图片视图，并在后台预检图片格式和像素尺寸。
    pub(crate) fn new(
        path: std::path::PathBuf,
        name: String,
        size: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path_text = path.display().to_string();
        let inspect_path = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { prepare_image(&inspect_path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(prepared) => {
                        this.meta = Some(prepared.meta);
                        this.display_path = prepared.display_path;
                        this.temporary = prepared.temporary;
                        this.orientation = prepared.orientation;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            name,
            display_path: path,
            temporary: None,
            orientation: None,
            path_text,
            size,
            meta: None,
            loading: true,
            error: None,
            fit: true,
            zoom: 1.0,
            pan: (0.0, 0.0),
            drag: None,
            canvas_bounds: Rc::new(Cell::new(None)),
            rotation: 0,
            rotating: false,
            notice: None,
        }
    }

    /// 当前用于状态栏和标题的缩放比例。
    fn zoom_percent(&self) -> u32 {
        (self.zoom * 100.0).round() as u32
    }

    /// 图片的像素尺寸；SVG 或预检失败时使用一个可平移的默认画布。
    fn natural_size(&self) -> (f32, f32) {
        self.meta
            .as_ref()
            .map(|meta| (meta.width.max(1) as f32, meta.height.max(1) as f32))
            .unwrap_or((1024.0, 768.0))
    }

    /// 画布在窗口中的精确度量；首帧 prepaint 之前返回 `None`。
    fn canvas_metrics(&self) -> Option<CanvasMetrics> {
        self.canvas_bounds.get().map(|bounds| CanvasMetrics {
            origin: (bounds.origin.x.as_f32(), bounds.origin.y.as_f32()),
            size: (bounds.size.width.as_f32(), bounds.size.height.as_f32()),
        })
    }

    /// 适应窗口（`ObjectFit::Contain`）时的显示比例；画布未知时退化为 1。
    fn fit_scale(&self) -> f32 {
        self.canvas_metrics()
            .map(|canvas| canvas.fit_scale(self.natural_size()))
            .unwrap_or(1.0)
    }

    /// 当前实际显示比例：适应窗口时由画布尺寸决定，否则为用户缩放值。
    fn display_zoom(&self) -> f32 {
        if self.fit {
            self.fit_scale()
        } else {
            self.zoom
        }
    }

    /// 以 `anchor`（窗口坐标）为锚点把显示比例调整到 `next_zoom`。
    ///
    /// 适应窗口态从当前显示比例继续并清零平移基准；锚点公式保持光标或
    /// 画布中心的图片内容不漂移。
    fn zoom_to(&mut self, anchor: (f32, f32), next_zoom: f32, cx: &mut Context<Self>) {
        let old_zoom = self.display_zoom();
        let (min_zoom, max_zoom) = zoom_bounds(self.fit_scale());
        let next_zoom = next_zoom.clamp(min_zoom, max_zoom);
        if (next_zoom - old_zoom).abs() <= f32::EPSILON {
            return;
        }
        let pan = if self.fit { (0.0, 0.0) } else { self.pan };
        let pan = match self.canvas_metrics() {
            Some(canvas) => anchored_pan(pan, anchor, canvas.center(), old_zoom, next_zoom),
            None => pan,
        };
        self.fit = false;
        self.zoom = next_zoom;
        self.pan = pan;
        self.drag = None;
        cx.notify();
    }

    /// 滚轮缩放：Windows 滚轮向上（`delta.y > 0`）放大、向下缩小，锚点为光标。
    fn zoom_by_wheel(&mut self, delta_y: f32, cursor: (f32, f32), cx: &mut Context<Self>) {
        if delta_y.abs() < 0.01 {
            return;
        }
        let next_zoom = wheel_zoom(self.display_zoom(), delta_y);
        self.zoom_to(cursor, next_zoom, cx);
    }

    /// 工具栏按钮缩放：以画布中心为锚点按 `factor` 缩放。
    fn zoom_by_factor(&mut self, factor: f32, cx: &mut Context<Self>) {
        let next_zoom = self.display_zoom() * factor;
        let anchor = self
            .canvas_metrics()
            .map(CanvasMetrics::center)
            .unwrap_or((0.0, 0.0));
        self.zoom_to(anchor, next_zoom, cx);
    }

    /// 切换到 100% 显示并清空平移。
    fn actual_size(&mut self, cx: &mut Context<Self>) {
        self.fit = false;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        self.drag = None;
        cx.notify();
    }

    /// 重置缩放、平移和适应窗口状态。
    fn reset_view(&mut self) {
        self.fit = true;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        self.drag = None;
    }

    /// 在后台旋转图片（`delta` 为正表示顺时针），完成后替换临时图片。
    fn rotate(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self
            .meta
            .as_ref()
            .is_some_and(|meta| meta.animated || meta.vector)
        {
            self.notice = Some(
                tr(
                    "动画或矢量图片不支持旋转",
                    "Animated or vector images cannot be rotated",
                )
                .to_string(),
            );
            cx.notify();
            return;
        }
        if self.rotating || delta == 0 {
            return;
        }
        self.rotating = true;
        self.notice = None;
        let source = self.display_path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { rotate_image_file(&source, delta) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.rotating = false;
                match result {
                    Ok((path, width, height)) => {
                        if let Some(previous) = this.temporary.take() {
                            let _ = std::fs::remove_file(previous);
                        }
                        this.display_path = path.clone();
                        this.temporary = Some(path);
                        this.rotation = ((this.rotation as i32 + delta).rem_euclid(4)) as u8;
                        if let Some(meta) = this.meta.as_mut() {
                            meta.width = width;
                            meta.height = height;
                        }
                    }
                    Err(error) => this.notice = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ImageViewerWindowView {
    /// 组合标题、工具栏、图片画布和状态栏。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(self.render_toolbar(cx))
            .child(self.render_body(cx))
            .child(self.render_status_bar(cx))
    }
}

mod app;
mod body;
mod image_ops;
mod status;
#[cfg(test)]
mod tests;
mod title;
mod toolbar;

use image_ops::*;
