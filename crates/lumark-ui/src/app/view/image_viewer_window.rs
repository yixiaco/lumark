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
use image::GenericImageView as _;
use image::ImageDecoder as _;
use image::metadata::Orientation;

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

    /// 渲染标题栏。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
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

    /// 渲染适应窗口、缩放和重置操作。
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pan = (0.0, 0.0);
                        this.set_zoom(1.0, cx);
                    })),
            )
            .child(
                button("image-zoom-out")
                    .ghost()
                    .icon(AppIcon::ZoomOut.element(16.))
                    .tooltip(tr("缩小", "Zoom out"))
                    .accessibility_label(tr("缩小", "Zoom out"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = this.zoom / 1.25;
                        this.set_zoom(next, cx);
                    })),
            )
            .child(
                button("image-zoom-in")
                    .ghost()
                    .icon(AppIcon::ZoomIn.element(16.))
                    .tooltip(tr("放大", "Zoom in"))
                    .accessibility_label(tr("放大", "Zoom in"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = this.zoom * 1.25;
                        this.set_zoom(next, cx);
                    })),
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

    /// 渲染加载、错误或图片画布。
    fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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

    /// 渲染底部尺寸、格式和缩放状态。
    fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
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

impl LumarkApp {
    /// 按文件类型选择文本查看器或图片查看器。
    pub(crate) fn open_builtin_viewer(
        &mut self,
        target: PathBuf,
        display: String,
        can_save: bool,
        archive_target: Option<super::text_viewer_window::ViewerArchiveTarget>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        if is_image_name(&display) {
            self.open_image_viewer(target, display, parent, cx);
        } else {
            self.open_text_viewer(target, display, can_save, archive_target, parent, cx);
        }
    }

    /// 在新的只读窗口中打开图片。
    pub(crate) fn open_image_viewer(
        &mut self,
        target: PathBuf,
        display: String,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let size = std::fs::metadata(&target)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        // 生成型预览（纹理/材质球等）的临时 PNG 文件名没有意义，优先使用
        // 调用方传入的资产显示名。
        let name = viewer_display_name(&display, &target);
        let window_size = gpui_kit::size(px(1000.), px(760.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - window_size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - window_size.height.as_f32()) / 2.0),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size: window_size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("图片查看器 - Lumark".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(480.), px(360.))),
            ..TitleBar::window_options()
        };
        let image_path = target.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                ImageViewerWindowView::new(image_path.clone(), name.clone(), size, window, cx)
            })
        });
        match result {
            Ok(_) => self.status = format!("已在内置图片查看器中打开 {display}"),
            Err(error) => self.status = format!("无法打开图片查看器：{error}"),
        }
    }
}

/// 图片查看器标题使用的显示名：优先资产名，其次文件名。
fn viewer_display_name(display: &str, target: &std::path::Path) -> String {
    let display = display.trim();
    if !display.is_empty() {
        return display.to_string();
    }
    target
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| tr("图片", "Image").to_string())
}

/// 读取图片格式与像素尺寸，并在解码前执行内存上限检查。
fn inspect_image(path: &std::path::Path) -> Result<ImageMeta, String> {
    const MAX_PIXELS: u64 = 100_000_000;
    const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

    let length = std::fs::metadata(path)
        .map_err(|error| format!("{}：{error}", tr("无法读取图片", "Cannot read image")))?
        .len();
    if length > MAX_FILE_BYTES {
        return Err(tr(
            "图片文件超过 512 MB，已拒绝在内存中解码",
            "Image exceeds 512 MB and is too large to decode in memory",
        )
        .to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "svg" {
        let (width, height) = inspect_svg_size(path)?;
        return Ok(ImageMeta {
            width,
            height,
            format: "SVG".to_string(),
            animated: false,
            vector: true,
        });
    }

    let reader = image::ImageReader::open(path)
        .map_err(|error| format!("{}：{error}", tr("无法打开图片", "Cannot open image")))?
        .with_guessed_format()
        .map_err(|error| {
            format!(
                "{}：{error}",
                tr("无法识别图片格式", "Unknown image format")
            )
        })?;
    let format = reader
        .format()
        .map(|format| format!("{format:?}").to_ascii_uppercase())
        .unwrap_or_else(|| extension.to_ascii_uppercase());
    let (width, height) = reader.into_dimensions().map_err(|error| {
        format!(
            "{}：{error}",
            tr("无法读取图片尺寸", "Cannot read image dimensions")
        )
    })?;
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_PIXELS {
        return Err(tr(
            "图片像素超过 1 亿，已拒绝在内存中解码",
            "Image exceeds 100 megapixels and is too large to decode in memory",
        )
        .to_string());
    }
    let animated = matches!(format.as_str(), "GIF" | "WEBP");
    Ok(ImageMeta {
        width,
        height,
        format,
        animated,
        vector: false,
    })
}

/// 在后台旋转图片并写入新的临时 PNG。
fn rotate_image_file(
    source: &std::path::Path,
    delta: i32,
) -> Result<(std::path::PathBuf, u32, u32), String> {
    let mut image = image::ImageReader::open(source)
        .map_err(|error| format!("{}：{error}", tr("无法打开图片", "Cannot open image")))?
        .with_guessed_format()
        .map_err(|error| {
            format!(
                "{}：{error}",
                tr("无法识别图片格式", "Unknown image format")
            )
        })?
        .decode()
        .map_err(|error| format!("{}：{error}", tr("无法解码图片", "Cannot decode image")))?;
    let steps = delta.rem_euclid(4);
    for _ in 0..steps {
        image = if delta > 0 {
            image.rotate90()
        } else {
            image.rotate270()
        };
    }
    let temporary =
        std::env::temp_dir().join(format!("lumark-rotate-{}.png", uuid::Uuid::new_v4()));
    image.save(&temporary).map_err(|error| {
        format!(
            "{}：{error}",
            tr("无法保存旋转结果", "Cannot save rotation")
        )
    })?;
    let (width, height) = image.dimensions();
    Ok((temporary, width, height))
}
/// 图片显示准备结果：元数据、实际绘制路径和 EXIF 方向说明。
struct PreparedImage {
    meta: ImageMeta,
    display_path: std::path::PathBuf,
    temporary: Option<std::path::PathBuf>,
    orientation: Option<String>,
}

/// 预检图片并按 EXIF 方向生成校正后的临时 PNG。
///
/// 只对静态栅格图应用方向校正；动图与 SVG 保持原文件，避免破坏动画或
/// 矢量渲染。方向读取或校正失败时回退到原图。
fn prepare_image(path: &std::path::Path) -> Result<PreparedImage, String> {
    let meta = inspect_image(path)?;
    let original = |meta: ImageMeta| PreparedImage {
        meta,
        display_path: path.to_path_buf(),
        temporary: None,
        orientation: None,
    };
    if meta.vector || meta.animated {
        return Ok(original(meta));
    }
    let Ok(reader) = image::ImageReader::open(path).and_then(|reader| reader.with_guessed_format())
    else {
        return Ok(original(meta));
    };
    let Ok(mut decoder) = reader.into_decoder() else {
        return Ok(original(meta));
    };
    let Ok(orientation) = decoder.orientation() else {
        return Ok(original(meta));
    };
    if orientation == Orientation::NoTransforms {
        return Ok(original(meta));
    }
    let Ok(mut image) = image::DynamicImage::from_decoder(decoder) else {
        return Ok(original(meta));
    };
    image.apply_orientation(orientation);
    let temporary = std::env::temp_dir().join(format!("lumark-image-{}.png", uuid::Uuid::new_v4()));
    if image.save(&temporary).is_err() {
        return Ok(original(meta));
    }
    let (width, height) = image.dimensions();
    Ok(PreparedImage {
        meta: ImageMeta {
            width,
            height,
            format: meta.format,
            animated: meta.animated,
            vector: false,
        },
        display_path: temporary.clone(),
        temporary: Some(temporary),
        orientation: Some(orientation_label(orientation)),
    })
}

/// EXIF 方向的中文说明。
fn orientation_label(orientation: Orientation) -> String {
    let label = match orientation {
        Orientation::NoTransforms => "原方向",
        Orientation::Rotate90 => "旋转 90°",
        Orientation::Rotate180 => "旋转 180°",
        Orientation::Rotate270 => "旋转 270°",
        Orientation::FlipHorizontal => "水平翻转",
        Orientation::FlipVertical => "垂直翻转",
        Orientation::Rotate90FlipH => "旋转 90° + 水平翻转",
        Orientation::Rotate270FlipH => "旋转 270° + 水平翻转",
    };
    format!("EXIF {label}")
}
/// 从 SVG 根节点读取 `width`/`height` 或 `viewBox`，避免为预检构建完整树。
fn inspect_svg_size(path: &std::path::Path) -> Result<(u32, u32), String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("{}：{error}", tr("无法读取图片", "Cannot read image")))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut reader = quick_xml::Reader::from_reader(text.as_bytes());
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(quick_xml::events::Event::Start(event))
            | Ok(quick_xml::events::Event::Empty(event)) => {
                if event.local_name().as_ref() != b"svg" {
                    continue;
                }
                let mut width = None;
                let mut height = None;
                let mut view_box = None;
                for attribute in event.attributes().flatten() {
                    let value = String::from_utf8_lossy(&attribute.value).to_string();
                    match attribute.key.local_name().as_ref() {
                        b"width" => width = parse_svg_length(&value),
                        b"height" => height = parse_svg_length(&value),
                        b"viewBox" | b"viewbox" => view_box = parse_svg_view_box(&value),
                        _ => {}
                    }
                }
                if let (Some(width), Some(height)) = (width, height) {
                    return Ok((width, height));
                }
                if let Some((width, height)) = view_box {
                    return Ok((width, height));
                }
                return Ok((1024, 1024));
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    Err(tr("SVG 缺少可用的尺寸", "SVG has no usable size").to_string())
}

/// 解析 SVG 长度，忽略单位；无法解析时返回 `None`。
fn parse_svg_length(value: &str) -> Option<u32> {
    let number = value
        .trim()
        .trim_end_matches(|character: char| character.is_ascii_alphabetic());
    number
        .parse::<f32>()
        .ok()
        .filter(|number| *number > 0.0)
        .map(|number| number.round() as u32)
}

/// 解析 SVG `viewBox` 的宽高。
fn parse_svg_view_box(value: &str) -> Option<(u32, u32)> {
    let parts = value
        .split(|character: char| character == ',' || character.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<f32>().ok())
        .collect::<Vec<_>>();
    if parts.len() == 4 && parts[2] > 0.0 && parts[3] > 0.0 {
        Some((parts[2].round() as u32, parts[3].round() as u32))
    } else {
        None
    }
}

/// 滚轮缩放：向上滚动（负 delta）放大，向下缩小。
fn wheel_zoom(zoom: f32, delta_y: f32) -> f32 {
    if delta_y.abs() < 0.01 {
        return zoom;
    }
    (zoom * (-delta_y * 0.0025).exp()).clamp(0.05, 16.0)
}

/// 滚轮增量换算为像素。
fn wheel_delta_y(delta: gpui_kit::ScrollDelta) -> f32 {
    match delta {
        gpui_kit::ScrollDelta::Pixels(point) => point.y.as_f32(),
        gpui_kit::ScrollDelta::Lines(point) => point.y * 40.0,
    }
}

/// 画布中心（窗口坐标近似值）：标题 38 + 工具栏约 36，状态栏 26。
fn wheel_center((width, height): (f32, f32)) -> (f32, f32) {
    (width / 2.0, height / 2.0 + 6.0)
}

/// 以光标为锚点调整平移：保持光标下的图片位置不漂移。
fn anchored_pan(
    pan: (f32, f32),
    cursor: (f32, f32),
    center: (f32, f32),
    old_zoom: f32,
    new_zoom: f32,
) -> (f32, f32) {
    if old_zoom <= f32::EPSILON || (new_zoom - old_zoom).abs() <= f32::EPSILON {
        return pan;
    }
    let factor = new_zoom / old_zoom;
    let offset_x = cursor.0 - center.0 - pan.0;
    let offset_y = cursor.1 - center.1 - pan.1;
    (
        pan.0 + (1.0 - factor) * offset_x,
        pan.1 + (1.0 - factor) * offset_y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成一张最小 JPEG。
    fn encode_jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut image = image::RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x * 40) as u8, (y * 80) as u8, 100]);
        }
        let mut bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 90)
            .encode_image(&image)
            .expect("编码 JPEG");
        bytes
    }

    /// 给 JPEG 插入包含 Orientation 标签的 EXIF APP1 段。
    fn with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II*\0");
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let mut output = vec![0xFF, 0xD8];
        output.extend_from_slice(&[0xFF, 0xE1]);
        output.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        output.extend_from_slice(&payload);
        output.extend_from_slice(&jpeg[2..]);
        output
    }

    /// EXIF 方向 6（旋转 90°）会生成校正后的临时 PNG，宽高互换。
    #[test]
    fn corrects_exif_orientation_for_static_jpeg() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rotated.jpg");
        std::fs::write(&path, with_exif_orientation(&encode_jpeg(2, 3), 6)).unwrap();
        let prepared = prepare_image(&path).expect("准备图片");
        assert!(prepared.orientation.is_some(), "应识别 EXIF 方向");
        assert_eq!((prepared.meta.width, prepared.meta.height), (3, 2));
        assert_ne!(prepared.display_path, path);
        assert!(prepared.display_path.is_file());
        let temporary = prepared.temporary.clone().expect("应有临时文件");
        assert!(temporary.exists(), "校正结果应写入临时图片");
        let _ = std::fs::remove_file(temporary);
    }

    /// 没有 EXIF 方向的图片保持原文件。
    #[test]
    fn keeps_original_without_orientation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plain.jpg");
        std::fs::write(&path, encode_jpeg(4, 2)).unwrap();
        let prepared = prepare_image(&path).expect("准备图片");
        assert!(prepared.orientation.is_none());
        assert_eq!(prepared.display_path, path);
        assert!(prepared.temporary.is_none());
    }
}

/// 手动旋转能在两个方向生成正确的宽高。
#[test]
fn rotates_image_file_both_directions() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("image.png");
    let mut image = image::RgbaImage::new(2, 3);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x * 100) as u8, (y * 60) as u8, 40, 255]);
    }
    image.save(&source).unwrap();
    let (right, width, height) = rotate_image_file(&source, 1).expect("右转");
    assert_eq!((width, height), (3, 2));
    let _ = std::fs::remove_file(right);
    let (left, width, height) = rotate_image_file(&source, -1).expect("左转");
    assert_eq!((width, height), (3, 2));
    let _ = std::fs::remove_file(left);
    let (half, width, height) = rotate_image_file(&source, 2).expect("180°");
    assert_eq!((width, height), (2, 3));
    let _ = std::fs::remove_file(half);
}

/// 生成型预览使用资产显示名，而不是临时文件名。
#[test]
fn viewer_display_name_prefers_asset_name() {
    let temporary = std::path::Path::new("C:/tmp/ue-1234.png");
    assert_eq!(
        viewer_display_name("MAT-ChickenA", temporary),
        "MAT-ChickenA"
    );
    assert_eq!(viewer_display_name("  ", temporary), "ue-1234.png");
}

/// 滚轮放大/缩小方向与缩放上下限。
#[test]
fn wheel_zoom_directions_and_bounds() {
    assert!(wheel_zoom(1.0, -120.0) > 1.0);
    assert!(wheel_zoom(1.0, 120.0) < 1.0);
    assert_eq!(wheel_zoom(1.0, 0.0), 1.0);
    assert!(wheel_zoom(100.0, -100_000.0) <= 16.0);
    assert!(wheel_zoom(0.001, 100_000.0) >= 0.05);
}

/// 光标锚点：放大时光标下的图片位置保持不动。
#[test]
fn anchored_zoom_keeps_cursor_point() {
    let center = (400.0, 300.0);
    let cursor = (500.0, 300.0);
    let pan = (0.0, 0.0);
    let next = anchored_pan(pan, cursor, center, 1.0, 2.0);
    assert!((next.0 + 100.0).abs() < f32::EPSILON);
    assert!(next.1.abs() < f32::EPSILON);
    // 光标正好在中心时不产生平移。
    let same = anchored_pan(pan, center, center, 1.0, 2.0);
    assert_eq!(same, pan);
    // 已有平移且放大同样满足锚点公式。
    let moved = anchored_pan((10.0, 0.0), cursor, center, 1.0, 2.0);
    assert!((moved.0 + 80.0).abs() < f32::EPSILON);
}
