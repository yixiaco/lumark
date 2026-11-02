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
