//! Windows 系统托盘图标。
//!
//! 压缩任务在后台继续运行时可能没有任何可见窗口；托盘图标让用户能重新
//! 打开主窗口或任务进度窗口。图标使用独立的 message-only 窗口接收 Shell
//! 回调，不修改 GPUI 自己的窗口过程。

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::PlatformError;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    ExtractIconExW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, HICON, HWND_MESSAGE,
    IDI_APPLICATION, LoadIconW, RegisterClassW, WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WNDCLASSW,
};

/// 托盘图标被点击后置位，由应用轮询消费。
static ACTIVATE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// 托盘 Shell 回调消息；使用 `WM_APP` 后的私有区间。
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
/// 当前进程内的托盘图标 ID。
const TRAY_ID: u32 = 1;
/// 托盘窗口类名，注册后整个进程复用。
const TRAY_CLASS_NAME: &str = "LumarkTrayWindow";

/// 一个已注册的 Windows 托盘图标。
pub struct TrayIcon {
    hwnd: HWND,
    icon: HICON,
    owned_icon: bool,
    data: NOTIFYICONDATAW,
}

impl TrayIcon {
    /// 注册托盘图标并显示在通知区域。
    pub fn show(tooltip: &str) -> Result<Self, PlatformError> {
        let hwnd = create_message_window()?;
        let (icon, owned_icon) = load_application_icon();
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_CALLBACK_MESSAGE;
        data.hIcon = icon;
        copy_tip(&mut data.szTip, tooltip);
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
        if added == 0 {
            let error = io::Error::last_os_error();
            unsafe {
                DestroyWindow(hwnd);
                if owned_icon {
                    DestroyIcon(icon);
                }
            }
            return Err(PlatformError::Windows(error));
        }
        Ok(Self {
            hwnd,
            icon,
            owned_icon,
            data,
        })
    }
}

impl Drop for TrayIcon {
    /// 从通知区域移除图标并销毁 message-only 窗口。
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data);
            DestroyWindow(self.hwnd);
            if self.owned_icon {
                DestroyIcon(self.icon);
            }
        }
    }
}

/// 取出并清除一次托盘点击请求；应用在轮询中调用。
pub fn take_activation_request() -> bool {
    ACTIVATE_REQUESTED.swap(false, Ordering::AcqRel)
}

/// 创建用于接收托盘回调的 message-only 窗口。
fn create_message_window() -> Result<HWND, PlatformError> {
    let class_name = tray_class_name();
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if hinstance.is_null() {
        return Err(PlatformError::Windows(io::Error::last_os_error()));
    }
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.hInstance = hinstance;
    class.lpfnWndProc = Some(tray_window_proc);
    class.lpszClassName = class_name.as_ptr();
    let registered = unsafe { RegisterClassW(&class) };
    if registered == 0 {
        let error = io::Error::last_os_error();
        // 类已存在时 RegisterClassW 返回 0；继续创建窗口即可。
        if error.raw_os_error() != Some(1410) {
            return Err(PlatformError::Windows(error));
        }
    }
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        Err(PlatformError::Windows(io::Error::last_os_error()))
    } else {
        Ok(hwnd)
    }
}

/// 返回稳定的托盘窗口类名 UTF-16 缓冲区。
fn tray_class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| {
        TRAY_CLASS_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    })
}

/// 优先读取 exe 内嵌图标，失败时退回系统应用图标。
fn load_application_icon() -> (HICON, bool) {
    let mut large: HICON = std::ptr::null_mut();
    let mut small: HICON = std::ptr::null_mut();
    let count = std::env::current_exe()
        .ok()
        .map(|path| {
            let wide = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1) }
        })
        .unwrap_or(0);
    if count > 0 {
        let icon = if !large.is_null() { large } else { small };
        if !icon.is_null() {
            return (icon, true);
        }
    }
    let icon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
    (icon, false)
}

/// 把 UTF-16 提示文本复制到 NOTIFYICONDATAW 的固定长度缓冲区。
fn copy_tip(buffer: &mut [u16; 128], text: &str) {
    let max = buffer.len().saturating_sub(1);
    for (slot, value) in buffer.iter_mut().zip(text.encode_utf16().take(max)) {
        *slot = value;
    }
}

/// 处理托盘鼠标消息；只记录点击，不在这里执行 GPUI 操作。
unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == TRAY_CALLBACK_MESSAGE {
        let mouse_message = (lparam as u32) & 0xffff;
        if mouse_message == WM_LBUTTONUP || mouse_message == WM_LBUTTONDBLCLK {
            ACTIVATE_REQUESTED.store(true, Ordering::Release);
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    /// 验证托盘消息窗口和图标注册流程可完整创建并清理。
    #[test]
    fn tray_icon_can_be_created_and_removed() {
        let icon = super::TrayIcon::show("Lumark 测试").expect("创建托盘图标");
        drop(icon);
    }
}
