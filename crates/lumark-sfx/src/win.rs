//! 自解压原生界面共享的 DPI、字体、文本和尺寸工具。

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH,
    DeleteObject, FF_DONTCARE, GetDC, GetDeviceCaps, HFONT, LOGPIXELSX, OUT_DEFAULT_PRECIS,
    ReleaseDC,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, SetProcessDPIAware, SetWindowTextW,
};

/// 让自解压界面按系统 DPI 缩放，避免高缩放下控件模糊或错位。
pub(crate) fn enable_dpi_awareness() {
    // SAFETY: 进程启动早期调用，失败时退回 Windows 的位图缩放。
    unsafe {
        SetProcessDPIAware();
    }
}

/// 读取屏幕 DPI；无法读取时按 96 DPI 处理。
pub(crate) fn dpi() -> u32 {
    // SAFETY: GetDC/ReleaseDC 成对调用，只读取设备能力。
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() {
            return 96;
        }
        let value = GetDeviceCaps(hdc, LOGPIXELSX as i32).max(96) as u32;
        ReleaseDC(std::ptr::null_mut(), hdc);
        value
    }
}

/// 把 96 DPI 下的逻辑像素换算为当前 DPI 的像素。
pub(crate) fn scale(value: i32) -> i32 {
    (value * dpi() as i32 + 48) / 96
}

/// 按磅值创建界面字体；返回的句柄需要在窗口销毁后删除。
pub(crate) fn make_font(point_size: i32, weight: i32) -> HFONT {
    let face = wide("Microsoft YaHei UI");
    let height = -((point_size * dpi() as i32 + 36) / 72);
    // SAFETY: 字体名称缓冲区在调用期间保持有效；失败时返回 null，调用方回退默认字体。
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            face.as_ptr(),
        )
    }
}

/// 给控件设置字体；字体句柄为空时保留系统默认字体。
pub(crate) fn set_font(hwnd: HWND, font: HFONT) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETFONT};
    if hwnd.is_null() || font.is_null() {
        return;
    }
    // SAFETY: WM_SETFONT 只读取字体句柄，不转移所有权。
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
    }
}

/// 删除字体句柄；null 句柄直接忽略。
pub(crate) fn delete_font(font: HFONT) {
    if font.is_null() {
        return;
    }
    // SAFETY: 字体由本模块创建，只在窗口销毁后删除。
    unsafe {
        DeleteObject(font as _);
    }
}

/// 读取编辑框或静态控件文本。
pub(crate) fn text_of(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    // SAFETY: 先读长度再分配缓冲区，GetWindowTextW 最多写入缓冲区长度。
    unsafe {
        let length = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        buffer.truncate(read.max(0) as usize);
        String::from_utf16_lossy(&buffer)
    }
}

/// 设置控件文本。
pub(crate) fn set_text(hwnd: HWND, text: &str) {
    if hwnd.is_null() {
        return;
    }
    let value = wide(text);
    // SAFETY: UTF-16 字符串以 NUL 结尾，调用期间保持有效。
    unsafe {
        SetWindowTextW(hwnd, value.as_ptr());
    }
}

/// 把 Rust 字符串转换为以 NUL 结尾的 UTF-16。
pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 资源 ID 1 的 `MAKEINTRESOURCE` 指针，供图标和清单资源使用。
pub(crate) fn icon_resource() -> *const u16 {
    std::ptr::without_provenance(1)
}

/// 把以 NUL 结尾的 UTF-16 缓冲区转换为 Rust 字符串。
pub(crate) fn from_wide(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|item| *item == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

/// 把字节数格式化为紧凑的 B/KB/MB/GB 文本。
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 把秒数格式化为 `HH:MM:SS`，未知时返回占位符。
pub(crate) fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--:--".to_string();
    }
    let total = seconds.round() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}
