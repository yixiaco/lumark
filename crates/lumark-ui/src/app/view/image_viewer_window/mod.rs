//! 内置图片查看器窗口。
//!
//! 图片由 GPUI 的 `img` 元素解码和绘制；这里只负责选择文件、预检尺寸、
//! 适应/缩放和平移。图片只读，不写回原文件或归档。

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::component::TitleBar;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::tag::Tag;

/// 解码前预检得到的图片元数据。
#[derive(Clone, Debug)]
struct ImageMeta {
    width: u32,
    height: u32,
    format: String,
    animated: bool,
    vector: bool,
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
    /// 手动缩放后的平移偏移（像素）。
    pan: (f32, f32),
    /// 左键拖动平移的上一帧位置。
    drag_pan: Option<(f32, f32)>,
    /// 最近一次鼠标位置（窗口坐标），用于滚轮缩放锚点。
    cursor: Option<(f32, f32)>,
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
            drag_pan: None,
            cursor: None,
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

    /// 设置缩放并退出适应窗口模式。
    fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.fit = false;
        self.zoom = zoom.clamp(0.05, 16.0);
        cx.notify();
    }

    /// 重置缩放、平移和适应窗口状态。
    fn reset_view(&mut self) {
        self.fit = true;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        self.drag_pan = None;
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
