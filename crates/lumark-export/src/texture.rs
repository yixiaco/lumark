//! 贴图编码：PNG / JPEG / TGA / WebP。
//!
//! 输入统一为 RGBA8；JPEG 丢失 alpha（按不透明编码），TGA 使用未压缩
//! 32 位 BGRA（top-left 原点），WebP 使用无损编码。

use image::{ExtendedColorType, ImageEncoder};
use serde::{Deserialize, Serialize};

use crate::ExportError;
use crate::gltf::ExportTexture;

/// 贴图导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureExportFormat {
    /// PNG（无损、保留 alpha）。
    Png,
    /// JPEG（有损、忽略 alpha）。
    Jpeg,
    /// TGA（未压缩 32 位 BGRA）。
    Tga,
    /// WebP（无损）。
    Webp,
}

impl TextureExportFormat {
    /// 全部格式。
    pub fn all() -> [Self; 4] {
        [Self::Png, Self::Jpeg, Self::Tga, Self::Webp]
    }

    /// 文件扩展名。
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tga => "tga",
            Self::Webp => "webp",
        }
    }

    /// 界面标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Tga => "TGA",
            Self::Webp => "WebP",
        }
    }
}

/// 贴图导出选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureExportOptions {
    /// 目标格式。
    pub format: TextureExportFormat,
    /// JPEG 质量（1–100），其它格式忽略。
    pub quality: u8,
}

impl Default for TextureExportOptions {
    fn default() -> Self {
        Self {
            format: TextureExportFormat::Png,
            quality: 90,
        }
    }
}

/// 编码一张贴图。
pub fn encode_texture(
    texture: &ExportTexture,
    options: &TextureExportOptions,
) -> Result<Vec<u8>, ExportError> {
    if texture.width == 0
        || texture.height == 0
        || texture.rgba.len() < texture.width as usize * texture.height as usize * 4
    {
        return Err(ExportError::InvalidTexture(texture.name.clone()));
    }
    match options.format {
        TextureExportFormat::Png => encode_png(texture),
        TextureExportFormat::Jpeg => encode_jpeg(texture, options.quality),
        TextureExportFormat::Tga => Ok(encode_tga(texture)),
        TextureExportFormat::Webp => encode_webp(texture),
    }
}

/// PNG：复用 glTF/USDA 的编码实现。
fn encode_png(texture: &ExportTexture) -> Result<Vec<u8>, ExportError> {
    crate::gltf::encode_png(texture)
}

/// JPEG：转 RGB 后按质量编码。
fn encode_jpeg(texture: &ExportTexture, quality: u8) -> Result<Vec<u8>, ExportError> {
    let mut rgb = Vec::with_capacity(texture.rgba.len() / 4 * 3);
    for pixel in texture.rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
        .write_image(&rgb, texture.width, texture.height, ExtendedColorType::Rgb8)
        .map_err(|error| ExportError::Serialization(error.to_string()))?;
    Ok(out)
}

/// TGA：18 字节头 + 未压缩 BGRA。
fn encode_tga(texture: &ExportTexture) -> Vec<u8> {
    let mut out = Vec::with_capacity(18 + texture.rgba.len());
    out.push(0); // ID length
    out.push(0); // color map type
    out.push(2); // uncompressed true-color
    out.extend_from_slice(&[0; 5]); // color map spec
    out.extend_from_slice(&0u16.to_le_bytes()); // x origin
    out.extend_from_slice(&0u16.to_le_bytes()); // y origin
    out.extend_from_slice(&(texture.width as u16).to_le_bytes());
    out.extend_from_slice(&(texture.height as u16).to_le_bytes());
    out.push(32); // bits per pixel
    out.push(0x28); // top-left origin + 8 alpha bits
    for pixel in texture.rgba.as_chunks::<4>().0 {
        out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    out
}

/// WebP：无损编码。
fn encode_webp(texture: &ExportTexture) -> Result<Vec<u8>, ExportError> {
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(
            &texture.rgba,
            texture.width,
            texture.height,
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| ExportError::Serialization(error.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture() -> ExportTexture {
        let mut rgba = Vec::new();
        for _ in 0..4 {
            rgba.extend_from_slice(&[255, 0, 0, 255]);
        }
        ExportTexture {
            name: "T_Test".to_string(),
            width: 2,
            height: 2,
            rgba,
        }
    }

    /// 四种格式的签名与可解码性。
    #[test]
    fn encodes_all_formats() {
        let texture = texture();
        let png = encode_texture(
            &texture,
            &TextureExportOptions {
                format: TextureExportFormat::Png,
                quality: 90,
            },
        )
        .expect("PNG");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory(&png).expect("解码 PNG");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));

        let jpeg = encode_texture(
            &texture,
            &TextureExportOptions {
                format: TextureExportFormat::Jpeg,
                quality: 85,
            },
        )
        .expect("JPEG");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let decoded = image::load_from_memory(&jpeg).expect("解码 JPEG");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));

        let tga = encode_texture(
            &texture,
            &TextureExportOptions {
                format: TextureExportFormat::Tga,
                quality: 90,
            },
        )
        .expect("TGA");
        assert_eq!(tga[2], 2, "TGA 应为未压缩 true-color");
        assert_eq!(u16::from_le_bytes([tga[12], tga[13]]), 2);
        assert_eq!(tga[16], 32);
        assert_eq!(&tga[18..22], &[0, 0, 255, 255], "TGA 应为 BGRA 与 alpha");

        let webp = encode_texture(
            &texture,
            &TextureExportOptions {
                format: TextureExportFormat::Webp,
                quality: 90,
            },
        )
        .expect("WebP");
        assert_eq!(&webp[..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let decoded = image::load_from_memory(&webp).expect("解码 WebP");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    /// 扩展名/标签。
    #[test]
    fn format_metadata() {
        assert_eq!(TextureExportFormat::Jpeg.extension(), "jpg");
        assert_eq!(TextureExportFormat::Tga.label(), "TGA");
        assert_eq!(TextureExportFormat::all().len(), 4);
    }

    /// 尺寸与像素不一致时报错。
    #[test]
    fn rejects_invalid_texture() {
        let texture = ExportTexture {
            name: "T_Bad".to_string(),
            width: 4,
            height: 4,
            rgba: vec![0; 4],
        };
        assert!(matches!(
            encode_texture(&texture, &TextureExportOptions::default()),
            Err(ExportError::InvalidTexture(_))
        ));
    }
}
