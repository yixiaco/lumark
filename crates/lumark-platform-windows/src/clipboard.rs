//! 把文件路径写入 Windows 剪贴板，供资源管理器直接粘贴。
//!
//! 归档条目本身没有真实路径，调用方需要先把它们解压到本窗口专属的临时目录，
//! 再把临时路径写入 `CF_HDROP`。临时文件的生命周期由调用方控制。

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY};
use windows_sys::Win32::UI::Shell::DROPFILES;

use crate::PlatformError;

/// 把一组真实存在的文件或目录路径写入剪贴板。
pub fn set_clipboard_files(paths: &[PathBuf]) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::EmptyPaths);
    }
    if paths.iter().any(|path| !path.exists()) {
        return Err(PlatformError::Windows(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "剪贴板文件不存在",
        )));
    }

    let data = build_hdrop(paths);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err(last_error("打开剪贴板"));
        }
        let _clipboard = ClipboardGuard;
        if EmptyClipboard() == 0 {
            return Err(last_error("清空剪贴板"));
        }
        let handle = GlobalAlloc(GMEM_MOVEABLE, data.len());
        if handle.is_null() {
            return Err(last_error("分配剪贴板内存"));
        }
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            GlobalFree(handle);
            return Err(last_error("锁定剪贴板内存"));
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), pointer.cast::<u8>(), data.len());
        GlobalUnlock(handle);
        if SetClipboardData(CF_HDROP as u32, handle).is_null() {
            GlobalFree(handle);
            return Err(last_error("写入文件剪贴板"));
        }
        set_copy_effect();
    }
    Ok(())
}

/// 按 `DROPFILES + UTF-16 多字符串 + 结尾空字符` 构造 `CF_HDROP` 数据。
fn build_hdrop(paths: &[PathBuf]) -> Vec<u8> {
    let header_size = std::mem::size_of::<DROPFILES>();
    let mut data = Vec::with_capacity(
        header_size
            + paths
                .iter()
                .map(|path| path.as_os_str().len() * 2 + 2)
                .sum::<usize>()
            + 2,
    );
    data.extend_from_slice(&(header_size as u32).to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&1i32.to_le_bytes());
    for path in paths {
        for unit in path.as_os_str().encode_wide() {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&0u16.to_le_bytes());
    }
    data.extend_from_slice(&0u16.to_le_bytes());
    data
}

/// 让资源管理器把粘贴识别为复制，而不是移动。
unsafe fn set_copy_effect() {
    let name = "Preferred DropEffect"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
    if format == 0 {
        return;
    }
    let effect = DROPEFFECT_COPY.to_le_bytes();
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, effect.len());
        if handle.is_null() {
            return;
        }
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            GlobalFree(handle);
            return;
        }
        std::ptr::copy_nonoverlapping(effect.as_ptr(), pointer.cast::<u8>(), effect.len());
        GlobalUnlock(handle);
        if SetClipboardData(format, handle).is_null() {
            GlobalFree(handle);
        }
    }
}

/// 剪贴板打开后必须关闭，失败路径同样依赖这个守卫。
struct ClipboardGuard;

impl Drop for ClipboardGuard {
    /// 释放剪贴板内存。
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

/// 读取最近的 Win32 错误并转换为平台错误。
fn last_error(operation: &str) -> PlatformError {
    let error = std::io::Error::last_os_error();
    PlatformError::Windows(std::io::Error::new(
        error.kind(),
        format!("{operation}失败：{error}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 `CF_HDROP` 头部使用宽字符标志，并以双空字符结束。
    #[test]
    fn hdrop_uses_wide_strings_and_double_terminator() {
        let data = build_hdrop(&[PathBuf::from(r"C:\temp\a.txt")]);
        let header_size = std::mem::size_of::<DROPFILES>();
        assert_eq!(header_size, 20);
        assert_eq!(
            u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize,
            header_size
        );
        assert_eq!(i32::from_le_bytes(data[16..20].try_into().unwrap()), 1);
        assert_eq!(&data[data.len() - 2..], &[0, 0]);
        assert!(data.len() > header_size + 2);
    }
}
