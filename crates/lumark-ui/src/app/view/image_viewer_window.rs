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
use gpui_kit::component::scroll::ScrollableElement as _;
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
    path: std::path::PathBuf,
    path_text: String,
    size: u64,
    meta: Option<ImageMeta>,
    loading: bool,
    error: Option<String>,
    fit: bool,
    zoom: f32,
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
                .spawn(async move { inspect_image(&inspect_path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(meta) => this.meta = Some(meta),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            name,
            path,
            path_text,
            size,
            meta: None,
            loading: true,
            error: None,
            fit: true,
            zoom: 1.0,
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
                        this.fit = true;
                        this.zoom = 1.0;
                        cx.notify();
                    })),
            )
            .child(
                button("image-actual")
                    .ghost()
                    .selected(!self.fit && (self.zoom - 1.0).abs() < f32::EPSILON)
                    .label("100%")
                    .on_click(cx.listener(|this, _, _, cx| this.set_zoom(1.0, cx))),
            )
            .child(
                button("image-zoom-out")
                    .ghost()
                    .icon(AppIcon::Close.element(13.))
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
                    .icon(AppIcon::Add.element(13.))
                    .tooltip(tr("放大", "Zoom in"))
                    .accessibility_label(tr("放大", "Zoom in"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = this.zoom * 1.25;
                        this.set_zoom(next, cx);
                    })),
            )
            .child(
                button("image-reset")
                    .ghost()
                    .icon(AppIcon::Refresh.element(13.))
                    .label(tr("重置", "Reset"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.fit = true;
                        this.zoom = 1.0;
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

        if self.fit {
            v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .items_center()
                .justify_center()
                .bg(cx.theme().background)
                .child(
                    gpui_kit::img(self.path.clone())
                        .size_full()
                        .object_fit(gpui_kit::ObjectFit::Contain),
                )
                .into_any_element()
        } else {
            let (natural_w, natural_h) = self.natural_size();
            let width = (natural_w * self.zoom).max(1.0);
            let height = (natural_h * self.zoom).max(1.0);
            div()
                .id("image-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_scrollbar()
                .bg(cx.theme().background)
                .child(
                    div()
                        .min_w_full()
                        .min_h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui_kit::img(self.path.clone())
                                .w(px(width))
                                .h(px(height))
                                .object_fit(gpui_kit::ObjectFit::Fill),
                        ),
                )
                .into_any_element()
        }
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
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| display.clone());
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
