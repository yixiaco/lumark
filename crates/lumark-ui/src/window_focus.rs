//! 窗口激活与置顶辅助。
//!
//! GPUI 的 `activate_window` 已负责恢复最小化窗口和请求前台焦点，但
//! Windows 前台锁仍可能让新窗口停在其它窗口后面；这里在激活后追加一次
//! 原生置顶，覆盖压缩/解压弹窗、查看器、托盘激活和 Shell 转发等入口。

/// 把窗口带到最前；取不到原生句柄或非 Windows 平台时静默忽略。
pub(crate) fn bring_to_front(window: &gpui_kit::Window) {
    #[cfg(windows)]
    {
        use raw_window_handle::RawWindowHandle;
        let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::Win32(raw) = handle.as_raw() {
            lumark_platform_windows::foreground::bring_to_front(raw.hwnd.get());
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// 激活窗口并在同一帧请求系统把窗口带到最前。
pub(crate) fn activate_and_raise(window: &mut gpui_kit::Window) {
    window.activate_window();
    bring_to_front(window);
}
