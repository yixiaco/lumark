//! 给 PE 可执行文件替换图标资源。
//!
//! 使用 `BeginUpdateResourceW` 直接改目标文件的 RT_ICON / RT_GROUP_ICON，
//! 不依赖外部资源编译器；调用方应先把运行时模块复制到临时文件，再追加
//! 自解压载荷。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_ICON};

use crate::PlatformError;

/// 一个 ICO 图像。
struct IconImage {
    width: u8,
    height: u8,
    color_count: u8,
    planes: u16,
    bit_count: u16,
    bytes: Vec<u8>,
}

/// 用 ICO 文件替换可执行文件的图标资源。
pub fn replace_icon(executable: &Path, icon: &Path) -> Result<(), PlatformError> {
    let data = std::fs::read(icon).map_err(PlatformError::Windows)?;
    let images = parse_ico(&data)?;
    let path = wide(executable.as_os_str());
    let handle = unsafe { BeginUpdateResourceW(path.as_ptr(), 0) };
    if handle.is_null() {
        return Err(PlatformError::Windows(std::io::Error::last_os_error()));
    }
    let result = unsafe { write_resources(handle, &images) };
    let end = unsafe { EndUpdateResourceW(handle, i32::from(result.is_err())) };
    if result.is_ok() && end == 0 {
        return Err(PlatformError::Windows(std::io::Error::last_os_error()));
    }
    result
}

/// 写入 RT_ICON 和 RT_GROUP_ICON 资源。
#[allow(clippy::manual_dangling_ptr)]
unsafe fn write_resources(handle: HANDLE, images: &[IconImage]) -> Result<(), PlatformError> {
    let mut group = Vec::with_capacity(6 + images.len() * 14);
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        group.push(image.width);
        group.push(image.height);
        group.push(image.color_count);
        group.push(0);
        group.extend_from_slice(&image.planes.to_le_bytes());
        group.extend_from_slice(&image.bit_count.to_le_bytes());
        group.extend_from_slice(&(image.bytes.len() as u32).to_le_bytes());
        group.extend_from_slice(&((index + 1) as u16).to_le_bytes());
        let id = (index + 1) as *const u16;
        let ok = unsafe {
            UpdateResourceW(
                handle,
                RT_ICON,
                id,
                0,
                image.bytes.as_ptr().cast(),
                image.bytes.len() as u32,
            )
        };
        if ok == 0 {
            return Err(PlatformError::Windows(std::io::Error::last_os_error()));
        }
    }
    let ok = unsafe {
        UpdateResourceW(
            handle,
            RT_GROUP_ICON,
            1usize as *const u16,
            0,
            group.as_ptr().cast(),
            group.len() as u32,
        )
    };
    if ok == 0 {
        return Err(PlatformError::Windows(std::io::Error::last_os_error()));
    }
    Ok(())
}

/// 解析 ICO 目录和各个图像数据。
fn parse_ico(data: &[u8]) -> Result<Vec<IconImage>, PlatformError> {
    if data.len() < 6 {
        return Err(invalid("ICO 头过短"));
    }
    let reserved = u16::from_le_bytes([data[0], data[1]]);
    let kind = u16::from_le_bytes([data[2], data[3]]);
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    if reserved != 0 || kind != 1 || count == 0 || count > 64 {
        return Err(invalid("不是有效的 ICO 文件"));
    }
    let directory_end = 6 + count * 16;
    if data.len() < directory_end {
        return Err(invalid("ICO 目录被截断"));
    }
    let mut images = Vec::with_capacity(count);
    for index in 0..count {
        let offset = 6 + index * 16;
        let width = data[offset];
        let height = data[offset + 1];
        let color_count = data[offset + 2];
        let planes = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let bit_count = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let size = u32::from_le_bytes([
            data[offset + 8],
            data[offset + 9],
            data[offset + 10],
            data[offset + 11],
        ]) as usize;
        let image_offset = u32::from_le_bytes([
            data[offset + 12],
            data[offset + 13],
            data[offset + 14],
            data[offset + 15],
        ]) as usize;
        let end = image_offset
            .checked_add(size)
            .ok_or_else(|| invalid("ICO 图像偏移溢出"))?;
        if size == 0 || end > data.len() {
            return Err(invalid("ICO 图像数据越界"));
        }
        images.push(IconImage {
            width,
            height,
            color_count,
            planes,
            bit_count,
            bytes: data[image_offset..end].to_vec(),
        });
    }
    Ok(images)
}

/// 构造 Windows 路径宽字符串。
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

/// 构造统一的参数错误。
fn invalid(message: &str) -> PlatformError {
    PlatformError::Windows(std::io::Error::other(message.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个 1x1、32 位的最小 ICO。
    fn tiny_ico() -> Vec<u8> {
        let mut image = Vec::new();
        image.extend_from_slice(&40u32.to_le_bytes()); // biSize
        image.extend_from_slice(&1i32.to_le_bytes()); // biWidth
        image.extend_from_slice(&2i32.to_le_bytes()); // biHeight（含掩码）
        image.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        image.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
        image.extend_from_slice(&0u32.to_le_bytes()); // biCompression
        image.extend_from_slice(&4u32.to_le_bytes()); // biSizeImage
        image.extend_from_slice(&[0u8; 16]); // 其余字段
        image.extend_from_slice(&[0u8; 4]); // 像素
        image.extend_from_slice(&[0u8; 4]); // AND 掩码

        let mut ico = Vec::new();
        ico.extend_from_slice(&0u16.to_le_bytes());
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.push(1); // width
        ico.push(1); // height
        ico.push(0); // colors
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend(image);
        ico
    }

    /// 验证 ICO 解析和 PE 图标资源替换。
    #[test]
    fn replaces_icon_on_executable_copy() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("copy.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let icon = directory.path().join("icon.ico");
        std::fs::write(&icon, tiny_ico()).unwrap();
        replace_icon(&executable, &icon).unwrap();
        assert!(std::fs::metadata(&executable).unwrap().len() > 0);
    }
}
