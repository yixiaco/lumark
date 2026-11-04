//! 把窗口带到最前。
//!
//! Windows 前台锁会阻止后台进程直接抢焦点；这里先临时置顶再取消置顶，
//! 让窗口在视觉上压过其它窗口，随后调用 `SetForegroundWindow` 尝试取得
//! 键盘焦点。该流程可在激活新窗口、托盘恢复和 Shell 单实例转发时复用。

/// 把指定原生窗口带到最前并尽量取得前台焦点。
///
/// `hwnd` 为空句柄时直接返回；非 Windows 平台没有该语义，调用被忽略。
pub fn bring_to_front(hwnd: isize) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, SW_RESTORE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
        };

        let window = HWND(hwnd as *mut core::ffi::c_void);
        if window.0.is_null() {
            return;
        }
        // 最小化窗口必须先恢复，否则置顶只作用于任务栏图标。
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
        // 置顶再取消置顶：绕开前台锁，让窗口立即出现在最上层。
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = SetWindowPos(window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
        let _ = SetForegroundWindow(window);
        let _ = BringWindowToTop(window);
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

#[cfg(test)]
mod tests {
    use super::bring_to_front;

    /// 空句柄应直接返回，不触碰系统 API 也不 panic。
    #[test]
    fn ignores_null_handle() {
        bring_to_front(0);
    }
}
