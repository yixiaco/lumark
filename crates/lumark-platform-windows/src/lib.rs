//! Windows shell and desktop integration.

use std::path::{Path, PathBuf};

pub mod ipc;
pub mod shell;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("Windows 操作失败：{0}")]
    Windows(std::io::Error),
    #[error("路径为空")]
    EmptyPaths,
}

/// Open a folder, or reveal a file in Explorer.
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let mut command = std::process::Command::new("explorer.exe");
    if path.is_file() {
        command.arg(format!("/select,{}", path.display()));
    } else {
        command.arg(path);
    }
    command.spawn().map(|_| ()).map_err(PlatformError::Windows)
}

/// Open a path with the user's default application.
pub fn open_default(path: &Path) -> Result<(), PlatformError> {
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(PlatformError::Windows)
}

/// Open the Windows default-apps page.
pub fn open_default_apps() -> Result<(), PlatformError> {
    std::process::Command::new("explorer.exe")
        .arg("ms-settings:defaultapps")
        .spawn()
        .map(|_| ())
        .map_err(PlatformError::Windows)
}

/// Delete files or directories through the Windows Recycle Bin when possible.
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

/// A process-wide named instance guard. The handle lives for the app lifetime.
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
