//! Windows 会话锁定状态查询。

/// 返回当前进程所在 Windows 会话是否处于锁定状态。
///
/// 通过 `WTSSessionInfoEx` 读取会话标志；查询失败或系统返回未知值时按
/// 未锁定处理，避免误锁密码库。
#[cfg(windows)]
pub fn session_locked() -> bool {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    use windows_sys::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory,
        WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
    };

    unsafe {
        let mut buffer: *mut u16 = null_mut();
        let mut bytes = 0u32;
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        ) == 0
            || buffer.is_null()
        {
            return false;
        }
        let info = &*(buffer as *const WTSINFOEXW);
        let locked = info.Level == 1
            && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_LOCK as i32;
        WTSFreeMemory(buffer as *mut c_void);
        locked
    }
}

/// 非 Windows 平台没有会话锁定概念。
#[cfg(not(windows))]
pub fn session_locked() -> bool {
    false
}
