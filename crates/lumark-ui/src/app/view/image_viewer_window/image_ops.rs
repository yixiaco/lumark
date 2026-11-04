use super::*;
use image::GenericImageView as _;
use image::ImageDecoder as _;
use image::metadata::Orientation;

/// 图片查看器标题使用的显示名：优先资产名，其次文件名。
pub(super) fn viewer_display_name(display: &str, target: &std::path::Path) -> String {
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
pub(super) fn inspect_image(path: &std::path::Path) -> Result<ImageMeta, String> {
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
pub(super) fn rotate_image_file(
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
pub(super) struct PreparedImage {
    pub(super) meta: ImageMeta,
    pub(super) display_path: std::path::PathBuf,
    pub(super) temporary: Option<std::path::PathBuf>,
    pub(super) orientation: Option<String>,
}

/// 预检图片并按 EXIF 方向生成校正后的临时 PNG。
///
/// 只对静态栅格图应用方向校正；动图与 SVG 保持原文件，避免破坏动画或
/// 矢量渲染。方向读取或校正失败时回退到原图。
pub(super) fn prepare_image(path: &std::path::Path) -> Result<PreparedImage, String> {
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
pub(super) fn orientation_label(orientation: Orientation) -> String {
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
pub(super) fn inspect_svg_size(path: &std::path::Path) -> Result<(u32, u32), String> {
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
pub(super) fn parse_svg_length(value: &str) -> Option<u32> {
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
pub(super) fn parse_svg_view_box(value: &str) -> Option<(u32, u32)> {
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

/// 手动缩放下限。
pub(super) const MIN_ZOOM: f32 = 0.05;

/// 手动缩放上限。
pub(super) const MAX_ZOOM: f32 = 16.0;

/// 滚轮缩放的指数系数（每像素滚轮增量）。
const WHEEL_ZOOM_RATE: f32 = 0.0025;

/// 滚轮缩放倍率：`delta_y > 0`（Windows 滚轮向上）放大，`delta_y < 0`（向下）缩小。
///
/// Windows 的 `WM_MOUSEWHEEL` 向上时 `delta.y` 为正，因此这里直接使用正指数；
/// 缩放范围由 [`zoom_bounds`] 统一限制，保证适应窗口到手动缩放的比例连续。
pub(super) fn wheel_zoom(zoom: f32, delta_y: f32) -> f32 {
    if delta_y.abs() < 0.01 {
        return zoom;
    }
    zoom * (delta_y * WHEEL_ZOOM_RATE).exp()
}

/// 缩放范围：常规 `MIN_ZOOM..=MAX_ZOOM`。
///
/// 小图在适应窗口时可能被放大到 16 倍以上，全景图也可能缩小到 5% 以下；此时
/// 边界扩展到适应窗口比例，避免切换模式时比例跳变或滚轮方向反转。
pub(super) fn zoom_bounds(fit_scale: f32) -> (f32, f32) {
    (MIN_ZOOM.min(fit_scale), MAX_ZOOM.max(fit_scale))
}

/// 滚轮增量换算为像素；行模式按每行 40 像素折算。
pub(super) fn wheel_delta_y(delta: gpui_kit::ScrollDelta) -> f32 {
    match delta {
        gpui_kit::ScrollDelta::Pixels(point) => point.y.as_f32(),
        gpui_kit::ScrollDelta::Lines(point) => point.y * 40.0,
    }
}

/// 画布在窗口坐标系中的精确度量，来自 `on_children_prepainted` 的子元素 bounds。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct CanvasMetrics {
    /// 画布左上角（窗口坐标）。
    pub(super) origin: (f32, f32),
    /// 画布尺寸。
    pub(super) size: (f32, f32),
}

impl CanvasMetrics {
    /// 画布中心（窗口坐标），用于滚轮锚点和按钮缩放锚点。
    pub(super) fn center(self) -> (f32, f32) {
        (
            self.origin.0 + self.size.0 / 2.0,
            self.origin.1 + self.size.1 / 2.0,
        )
    }

    /// 适应窗口（`ObjectFit::Contain`）时的显示比例；尺寸无效时退化为 1。
    pub(super) fn fit_scale(self, natural: (f32, f32)) -> f32 {
        if self.size.0 <= 0.0 || self.size.1 <= 0.0 || natural.0 <= 0.0 || natural.1 <= 0.0 {
            return 1.0;
        }
        (self.size.0 / natural.0).min(self.size.1 / natural.1)
    }
}

/// 图片在画布内的绝对布局矩形（画布局部坐标）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ImageRect {
    /// 左边缘相对画布左边缘的偏移。
    pub(super) left: f32,
    /// 上边缘相对画布上边缘的偏移。
    pub(super) top: f32,
    /// 图片显示宽度（原图宽 × 缩放）。
    pub(super) width: f32,
    /// 图片显示高度（原图高 × 缩放）。
    pub(super) height: f32,
}

/// 计算非适应模式下图片的绝对布局矩形。
///
/// 宽高只由原图尺寸和缩放决定，与 `pan` 无关，因此拖拽只平移位置，不会
/// 触发 flex 收缩或非等比压扁；位置以画布中心为基准：
/// `left = 画布中心 + pan.x - width / 2`，`top` 同理。
pub(super) fn image_rect(
    natural: (f32, f32),
    canvas: (f32, f32),
    zoom: f32,
    pan: (f32, f32),
) -> ImageRect {
    let width = (natural.0 * zoom).max(1.0);
    let height = (natural.1 * zoom).max(1.0);
    ImageRect {
        left: canvas.0 / 2.0 + pan.0 - width / 2.0,
        top: canvas.1 / 2.0 + pan.1 - height / 2.0,
        width,
        height,
    }
}

/// 以光标为锚点调整平移：保持光标下的图片位置不漂移。
pub(super) fn anchored_pan(
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

/// 按绝对坐标计算拖拽平移：`pan = 按下时的 pan + 当前光标相对按下点的位移`。
///
/// 每次都从按下点重新计算，不累加每帧增量，因此不会积累布局回流误差，
/// 快速拖拽或指针短暂离开画布后回来也不会滞后。
pub(super) fn dragged_pan(start: (f32, f32), base: (f32, f32), cursor: (f32, f32)) -> (f32, f32) {
    (base.0 + cursor.0 - start.0, base.1 + cursor.1 - start.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 极小/极大画布下图片矩形仍居中，且宽高不为 0。
    #[test]
    fn image_rect_centers_at_extreme_canvas_sizes() {
        let natural = (400.0, 300.0);
        for canvas in [(0.0, 0.0), (1.0, 1.0), (2.5, 3.5), (100_000.0, 80_000.0)] {
            let rect = image_rect(natural, canvas, 1.0, (0.0, 0.0));
            assert!(rect.width >= 1.0 && rect.height >= 1.0);
            assert!(
                (rect.left + rect.width / 2.0 - canvas.0 / 2.0).abs() < 1.0e-3,
                "画布 {canvas:?} 中图片应水平居中"
            );
            assert!(
                (rect.top + rect.height / 2.0 - canvas.1 / 2.0).abs() < 1.0e-3,
                "画布 {canvas:?} 中图片应垂直居中"
            );
            assert!(rect.left.is_finite() && rect.top.is_finite());
        }
    }

    /// 滚轮缩放锚点：光标下的图片局部坐标在缩放前后保持不变。
    #[test]
    fn anchored_pan_keeps_cursor_point_fixed_at_extremes() {
        let natural = (400.0, 300.0);
        let canvas = (1920.0, 1080.0);
        let center = (canvas.0 / 2.0, canvas.1 / 2.0);
        for (cursor, old_zoom, new_zoom) in [
            ((0.0, 0.0), 0.05, 16.0),
            ((1920.0, 1080.0), 16.0, 0.05),
            ((1.0e-3, 1.0e-3), 1.0, 16.0),
            ((10_000.0, 10_000.0), 0.05, 1.0),
        ] {
            let before = image_rect(natural, canvas, old_zoom, (0.0, 0.0));
            let pan = anchored_pan((0.0, 0.0), cursor, center, old_zoom, new_zoom);
            let after = image_rect(natural, canvas, new_zoom, pan);
            // 光标相对图片左上角的归一化坐标应保持不变。
            let before_u = (cursor.0 - before.left) / before.width;
            let before_v = (cursor.1 - before.top) / before.height;
            let after_u = (cursor.0 - after.left) / after.width;
            let after_v = (cursor.1 - after.top) / after.height;
            assert!(
                (before_u - after_u).abs() < 1.0e-3,
                "缩放锚点水平漂移（cursor={cursor:?}，zoom {old_zoom}→{new_zoom}）"
            );
            assert!(
                (before_v - after_v).abs() < 1.0e-3,
                "缩放锚点垂直漂移（cursor={cursor:?}，zoom {old_zoom}→{new_zoom}）"
            );
        }
    }

    /// 画布度量在极端窗口尺寸下仍给出正确的中心与适应比例。
    #[test]
    fn canvas_metrics_handle_extreme_sizes() {
        let tiny = CanvasMetrics {
            origin: (0.0, 0.0),
            size: (1.0, 1.0),
        };
        assert_eq!(tiny.center(), (0.5, 0.5));
        assert!(tiny.fit_scale((400.0, 300.0)) > 0.0);
        let huge = CanvasMetrics {
            origin: (12.5, 34.5),
            size: (100_000.0, 80_000.0),
        };
        assert_eq!(huge.center(), (12.5 + 50_000.0, 34.5 + 40_000.0));
        assert!((huge.fit_scale((400.0, 300.0)) - 250.0).abs() < 1.0e-3);
        let empty = CanvasMetrics::default();
        assert_eq!(empty.fit_scale((400.0, 300.0)), 1.0);
    }
}
