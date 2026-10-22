//! Windows Shell 和桌面集成。

use std::path::{Path, PathBuf};

pub mod clipboard;
pub mod drag;
pub mod icons;
pub mod ipc;
pub mod power;
pub mod shell;
pub mod tray;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("Windows 操作失败：{0}")]
    Windows(std::io::Error),
    #[error("路径为空")]
    EmptyPaths,
}

/// 打开文件夹，或在资源管理器中定位文件。
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let mut command = std::process::Command::new("explorer.exe");
    if path.is_file() {
        command.arg(format!("/select,{}", path.display()));
    } else {
        command.arg(path);
    }
    command.spawn().map(|_| ()).map_err(PlatformError::Windows)
}

/// 使用用户默认应用打开路径。
pub fn open_default(path: &Path) -> Result<(), PlatformError> {
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(PlatformError::Windows)
}

/// 打开 Windows 默认应用设置页。
pub fn open_default_apps() -> Result<(), PlatformError> {
    open_settings_uri("ms-settings:defaultapps")
}

/// 打开 Windows 默认应用中 Lumark 的专属设置页。
///
/// 需要先在 `HKEY_CURRENT_USER\Software\RegisteredApplications` 注册
/// `Lumark`；ShellExecute 会直接激活设置应用，比转交 explorer 更可靠。
pub fn open_default_apps_for_lumark() -> Result<(), PlatformError> {
    open_settings_uri("ms-settings:defaultapps?registeredAppUser=Lumark")
}

/// 通过 ShellExecute 打开设置 URI，避免 explorer 偶发不激活设置窗口。
fn open_settings_uri(uri: &str) -> Result<(), PlatformError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let wide = |value: &str| {
            std::ffi::OsStr::new(value)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        };
        let operation = wide("open");
        let target = wide(uri);
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize > 32 {
            return Ok(());
        }
        Err(PlatformError::Windows(std::io::Error::other(format!(
            "无法打开 Windows 设置：ShellExecute 返回 {}",
            result as isize
        ))))
    }
    #[cfg(not(windows))]
    {
        let _ = uri;
        Err(PlatformError::Windows(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Windows Settings is only available on Windows",
        )))
    }
}

/// 尽可能通过 Windows 回收站删除文件或目录。
pub fn recycle(paths: &[PathBuf]) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::EmptyPaths);
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            SHFILEOPSTRUCTW, SHFileOperationW,
        };

        let mut from = Vec::<u16>::new();
        for path in paths {
            from.extend(path.as_os_str().encode_wide());
            from.push(0);
        }
        from.push(0);
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };
        let result = unsafe { SHFileOperationW(&mut operation) };
        if result == 0 {
            Ok(())
        } else {
            Err(PlatformError::Windows(std::io::Error::from_raw_os_error(
                result,
            )))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = paths;
        Err(PlatformError::Windows(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Recycle Bin is only available on Windows",
        )))
    }
}

/// 进程级命名实例守卫；句柄在应用生命周期内保持有效。
pub struct InstanceGuard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl InstanceGuard {
    pub fn acquire(name: &str) -> Result<Option<Self>, PlatformError> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS};
            use windows_sys::Win32::System::Threading::CreateMutexW;

            let wide: Vec<u16> = std::ffi::OsStr::new(name)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err(PlatformError::Windows(std::io::Error::last_os_error()));
            }
            let already_running =
                unsafe { windows_sys::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS };
            if already_running {
                unsafe { CloseHandle(handle) };
                Ok(None)
            } else {
                Ok(Some(Self { handle }))
            }
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            Ok(Some(Self {}))
        }
    }
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(self.handle) };
    }
}
