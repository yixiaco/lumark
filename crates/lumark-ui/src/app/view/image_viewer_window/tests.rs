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

/// 滚轮方向（GPUI/Windows：向上 `delta.y > 0` 放大）与缩放范围。
#[test]
fn wheel_zoom_directions_and_bounds() {
    assert!(wheel_zoom(1.0, 120.0) > 1.0, "滚轮向上应放大");
    assert!(wheel_zoom(1.0, -120.0) < 1.0, "滚轮向下应缩小");
    assert_eq!(wheel_zoom(1.0, 0.0), 1.0);
    let (min_zoom, max_zoom) = zoom_bounds(1.0);
    assert_eq!((min_zoom, max_zoom), (MIN_ZOOM, MAX_ZOOM));
    assert_eq!(
        wheel_zoom(1.0, 100_000.0).clamp(min_zoom, max_zoom),
        MAX_ZOOM
    );
    assert_eq!(
        wheel_zoom(1.0, -100_000.0).clamp(min_zoom, max_zoom),
        MIN_ZOOM
    );
    // 小图/全景图在适应窗口时超出常规范围，边界扩展到适应比例以免方向反转。
    assert_eq!(zoom_bounds(0.02).0, 0.02);
    assert_eq!(zoom_bounds(380.0).1, 380.0);
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

/// 绝对布局：图片以画布中心为基准缩放，平移只改变位置、不改变尺寸。
#[test]
fn image_rect_centers_image_and_keeps_size_when_panned() {
    let natural = (200.0, 100.0);
    let canvas = (800.0, 600.0);
    let rect = image_rect(natural, canvas, 2.0, (0.0, 0.0));
    assert_eq!((rect.width, rect.height), (400.0, 200.0));
    assert_eq!((rect.left, rect.top), (200.0, 200.0), "2 倍缩放应居中");
    // 平移只改变位置：宽高不变，不会出现 flex 收缩导致的非等比压缩。
    let panned = image_rect(natural, canvas, 2.0, (50.0, -30.0));
    assert_eq!((panned.left, panned.top), (250.0, 170.0));
    assert_eq!((panned.width, panned.height), (rect.width, rect.height));
    // 缩小后仍以画布中心居中。
    let small = image_rect(natural, canvas, 0.5, (0.0, 0.0));
    assert_eq!((small.width, small.height), (100.0, 50.0));
    assert_eq!((small.left, small.top), (350.0, 275.0));
}

/// 画布中心与适应窗口比例来自精确 bounds。
#[test]
fn canvas_metrics_center_and_fit_scale() {
    let canvas = CanvasMetrics {
        origin: (10.0, 50.0),
        size: (800.0, 600.0),
    };
    assert_eq!(canvas.center(), (410.0, 350.0));
    assert_eq!(canvas.fit_scale((1600.0, 1200.0)), 0.5);
    assert_eq!(canvas.fit_scale((2000.0, 300.0)), 0.4);
    // 无效尺寸退化为 1，避免除零。
    assert_eq!(CanvasMetrics::default().fit_scale((100.0, 100.0)), 1.0);
}

/// 拖拽按绝对坐标 1:1 累计位移，且不改变图片尺寸。
#[test]
fn drag_pan_tracks_cursor_one_to_one() {
    let natural = (100.0, 80.0);
    let canvas = (600.0, 400.0);
    let before = image_rect(natural, canvas, 1.5, (0.0, 0.0));
    let pan = dragged_pan((120.0, 200.0), (0.0, 0.0), (170.0, 260.0));
    assert_eq!(pan, (50.0, 60.0));
    let after = image_rect(natural, canvas, 1.5, pan);
    assert_eq!((after.width, after.height), (before.width, before.height));
    assert_eq!((after.left - before.left, after.top - before.top), pan);
    // 再次移动仍以按下点为基准，不累加布局误差。
    assert_eq!(
        dragged_pan((120.0, 200.0), (0.0, 0.0), (220.0, 200.0)),
        (100.0, 0.0)
    );
}

/// 适应窗口时滚轮从当前显示比例继续，锚点使用精确画布中心。
#[test]
fn fit_scale_zoom_uses_precise_canvas_center() {
    let canvas = CanvasMetrics {
        origin: (10.0, 50.0),
        size: (800.0, 600.0),
    };
    let fit_scale = canvas.fit_scale((1600.0, 1200.0));
    assert_eq!(fit_scale, 0.5);
    let (min_zoom, max_zoom) = zoom_bounds(fit_scale);
    let next_zoom = wheel_zoom(fit_scale, 120.0).clamp(min_zoom, max_zoom);
    assert!(next_zoom > fit_scale, "适应窗口向上滚动应放大");
    // 光标在画布中心右侧，放大后图片应向左移动以保持锚点。
    let pan = anchored_pan(
        (0.0, 0.0),
        (600.0, 350.0),
        canvas.center(),
        fit_scale,
        next_zoom,
    );
    assert!(pan.0 < 0.0);
    assert!(pan.1.abs() < f32::EPSILON);
}

/// 极端窗口尺寸下图片中心仍与画布中心像素级对齐。
#[gpui_kit::test]
fn image_view_centers_at_extreme_window_sizes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photo.png");
    image::RgbaImage::from_pixel(400, 300, image::Rgba([20, 120, 220, 255]))
        .save(&path)
        .expect("写入测试图片");
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view =
            ImageViewerWindowView::new(path.clone(), "photo.png".to_string(), 0, window, cx);
        view.loading = false;
        view.meta = Some(ImageMeta {
            width: 400,
            height: 300,
            format: "PNG".to_string(),
            animated: false,
            vector: false,
        });
        view.fit = false;
        view.zoom = 1.0;
        view
    });
    let _ = view;
    for (width, height) in [(120.0, 90.0), (2400.0, 1500.0)] {
        cx.simulate_resize(gpui_kit::size(px(width), px(height)));
        // 尺寸变化后需要补一帧：绝对布局依赖上一帧画布中心。
        for _ in 0..2 {
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
            });
        }
        let canvas = cx
            .debug_bounds("image-viewer-canvas")
            .expect("画布应记录布局 bounds");
        let image = cx
            .debug_bounds("image-viewer-image")
            .expect("非适应模式应绘制图片");
        let canvas_center = (
            canvas.origin.x.as_f32() + canvas.size.width.as_f32() / 2.0,
            canvas.origin.y.as_f32() + canvas.size.height.as_f32() / 2.0,
        );
        let image_center = (
            image.origin.x.as_f32() + image.size.width.as_f32() / 2.0,
            image.origin.y.as_f32() + image.size.height.as_f32() / 2.0,
        );
        assert!(
            (canvas_center.0 - image_center.0).abs() <= 1.0
                && (canvas_center.1 - image_center.1).abs() <= 1.0,
            "窗口 {width}x{height} 下图片中心 {image_center:?} 应与画布中心 {canvas_center:?} 对齐"
        );
        assert!(
            (image.size.width.as_f32() - 400.0).abs() <= 1.0
                && (image.size.height.as_f32() - 300.0).abs() <= 1.0,
            "缩放 1.0 时图片尺寸应保持原图尺寸"
        );
    }
}
