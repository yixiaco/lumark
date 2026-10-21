//! 读取 Windows Shell 的文件类型图标，并编码为 PNG 供界面显示。
//!
//! 归档条目没有真实路径，调用方传入带扩展名的相对名称并启用
//! `SHGFI_USEFILEATTRIBUTES`，Shell 会按扩展名返回系统关联图标。

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use image::{ImageEncoder, RgbaImage, codecs::png::PngEncoder};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
};
use windows_sys::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// 返回文件或文件夹的系统图标 PNG；Shell 不可用时返回 `None`。
pub fn file_icon_png(path: &Path, is_dir: bool, use_file_attributes: bool) -> Option<Vec<u8>> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let attributes: FILE_FLAGS_AND_ATTRIBUTES = if is_dir {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };
    let flags = SHGFI_ICON
        | SHGFI_LARGEICON
        | if use_file_attributes {
            SHGFI_USEFILEATTRIBUTES
        } else {
            0
        };
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let result = unsafe {
        SHGetFileInfoW(
            wide.as_ptr(),
            attributes,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if result == 0 || info.hIcon.is_null() {
        return None;
    }
    let png = unsafe { icon_to_png(info.hIcon) };
    unsafe {
        DestroyIcon(info.hIcon);
    }
    png
}

/// 把 HICON 转成 32 位 RGBA PNG。
unsafe fn icon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    if unsafe { GetIconInfo(icon, &mut info) } == 0 {
        return None;
    }
    let result = unsafe { icon_info_to_png(&info) };
    unsafe {
        if !info.hbmColor.is_null() {
            DeleteObject(info.hbmColor as HGDIOBJ);
        }
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask as HGDIOBJ);
        }
    }
    result
}

/// 从图标的颜色位图读取 BGRA，并按掩码补齐透明通道。
unsafe fn icon_info_to_png(info: &ICONINFO) -> Option<Vec<u8>> {
    if info.hbmColor.is_null() {
        return None;
    }
    let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };
    let object_size = std::mem::size_of::<BITMAP>() as i32;
    if unsafe {
        GetObjectW(
            info.hbmColor as HGDIOBJ,
            object_size,
            (&mut bitmap as *mut BITMAP).cast(),
        )
    } == 0
    {
        return None;
    }
    let width = bitmap.bmWidth.unsigned_abs();
    let height = bitmap.bmHeight.unsigned_abs();
    if width == 0 || height == 0 {
        return None;
    }

    let screen = unsafe { GetDC(std::ptr::null_mut()) };
    if screen.is_null() {
        return None;
    }
    let memory = unsafe { CreateCompatibleDC(screen) };
    if memory.is_null() {
        unsafe { ReleaseDC(std::ptr::null_mut(), screen) };
        return None;
    }

    let mut header: BITMAPINFO = unsafe { std::mem::zeroed() };
    header.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    header.bmiHeader.biWidth = width as i32;
    header.bmiHeader.biHeight = -(height as i32);
    header.bmiHeader.biPlanes = 1;
    header.bmiHeader.biBitCount = 32;
    header.bmiHeader.biCompression = BI_RGB;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let old = unsafe { SelectObject(memory, info.hbmColor as HGDIOBJ) };
    let lines = unsafe {
        GetDIBits(
            memory,
            info.hbmColor,
            0,
            height,
            pixels.as_mut_ptr().cast(),
            &mut header,
            DIB_RGB_COLORS,
        )
    };
    unsafe { SelectObject(memory, old) };

    if lines == 0 {
        unsafe {
            DeleteDC(memory);
            ReleaseDC(std::ptr::null_mut(), screen);
        }
        return None;
    }

    let has_alpha = pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0);
    if !has_alpha && !info.hbmMask.is_null() {
        unsafe { apply_mask(memory, info.hbmMask, width, height, &mut pixels) };
    }
    unsafe {
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
    }

    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u16;
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u16 * 255) / alpha).min(255) as u8;
            }
        }
        pixel.swap(0, 2);
    }

    let image = RgbaImage::from_raw(width, height, pixels)?;
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(png)
}

/// 用 1 位掩码把透明像素的 alpha 设为 0。
unsafe fn apply_mask(memory: HDC, mask: HBITMAP, width: u32, height: u32, pixels: &mut [u8]) {
    let stride = (width as usize).div_ceil(32) * 4;
    let mut bits = vec![0u8; stride * height as usize];
    let mut header: BITMAPINFO = unsafe { std::mem::zeroed() };
    header.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    header.bmiHeader.biWidth = width as i32;
    header.bmiHeader.biHeight = -(height as i32);
    header.bmiHeader.biPlanes = 1;
    header.bmiHeader.biBitCount = 1;
    header.bmiHeader.biCompression = BI_RGB;
    let old = unsafe { SelectObject(memory, mask as HGDIOBJ) };
    let lines = unsafe {
        GetDIBits(
            memory,
            mask,
            0,
            height,
            bits.as_mut_ptr().cast(),
            &mut header,
            DIB_RGB_COLORS,
        )
    };
    unsafe { SelectObject(memory, old) };
    if lines == 0 {
        return;
    }
    for y in 0..height as usize {
        for x in 0..width as usize {
            let set = (bits[y * stride + x / 8] >> (7 - (x % 8))) & 1 == 1;
            if set {
                pixels[(y * width as usize + x) * 4 + 3] = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 Shell 文件类型图标能转换为 RGBA PNG，并保留透明通道。
    #[test]
    fn shell_file_icon_is_png() {
        let png =
            file_icon_png(Path::new("example.txt"), false, true).expect("Shell 文件图标应可用");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png.len() > 100);
    }
}
