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

/// 滚轮缩放：向上滚动（负 delta）放大，向下缩小。
pub(super) fn wheel_zoom(zoom: f32, delta_y: f32) -> f32 {
    if delta_y.abs() < 0.01 {
        return zoom;
    }
    (zoom * (-delta_y * 0.0025).exp()).clamp(0.05, 16.0)
}

/// 滚轮增量换算为像素。
pub(super) fn wheel_delta_y(delta: gpui_kit::ScrollDelta) -> f32 {
    match delta {
        gpui_kit::ScrollDelta::Pixels(point) => point.y.as_f32(),
        gpui_kit::ScrollDelta::Lines(point) => point.y * 40.0,
    }
}

/// 画布中心（窗口坐标近似值）：标题 38 + 工具栏约 36，状态栏 26。
pub(super) fn wheel_center((width, height): (f32, f32)) -> (f32, f32) {
    (width / 2.0, height / 2.0 + 6.0)
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
