//! 拖拽目标解析：识别资源管理器窗口或桌面，并返回投放目录。
//!
//! Lumark 不再让资源管理器通过虚拟文件流复制数据，而是只把原生拖拽当作
//! “选择目标”的手势；释放后由 Lumark 自己把条目解压到解析出的目录，因此
//! 可以显示进度、正确处理多选，也不会阻塞资源管理器。

use std::path::PathBuf;

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Shell::{CSIDL_DESKTOPDIRECTORY, SHGetFolderPathW};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetCursorPos, WindowFromPoint,
};

/// 返回当前鼠标位置对应的可写投放目录。
pub fn current_target() -> Option<PathBuf> {
    let (window, class) = cursor_window()?;
    match class.as_str() {
        "CabinetWClass" | "ExploreWClass" => explorer_folder(window.0 as isize),
        "Progman" | "WorkerW" => desktop_folder(),
        _ => None,
    }
}

/// 返回鼠标下的根窗口和窗口类名。
fn cursor_window() -> Option<(windows::Win32::Foundation::HWND, String)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point).ok()? };
    let window = unsafe { WindowFromPoint(point) };
    if window.0.is_null() {
        return None;
    }
    let root = unsafe { GetAncestor(window, GA_ROOT) };
    let root = if root.0.is_null() { window } else { root };
    let mut class = [0u16; 256];
    let length = unsafe { GetClassNameW(root, &mut class) };
    if length <= 0 {
        return None;
    }
    Some((root, String::from_utf16_lossy(&class[..length as usize])))
}

/// 通过 ShellWindows 找到资源管理器窗口当前打开的文件夹。
fn explorer_folder(window: isize) -> Option<PathBuf> {
    use windows57::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows57::Win32::UI::Shell::{IShellWindows, IWebBrowser2};
    use windows57::core::{GUID, Interface as _, VARIANT};

    // CLSID_ShellWindows；windows 0.61 不再生成 IShellWindows 接口，单独使用
    // 已存在的 0.57 types 只做只读查询。
    const CLSID_SHELL_WINDOWS: GUID = GUID::from_u128(0x9BA05972_F6A8_11CF_A442_00A0C90A8F39);

    let shell: IShellWindows =
        unsafe { CoCreateInstance(&CLSID_SHELL_WINDOWS, None, CLSCTX_ALL).ok()? };
    let count = unsafe { shell.Count().ok()? };
    for index in 0..count {
        let dispatch = match unsafe { shell.Item(&VARIANT::from(index)) } {
            Ok(dispatch) => dispatch,
            Err(_) => continue,
        };
        let browser: IWebBrowser2 = match dispatch.cast() {
            Ok(browser) => browser,
            Err(_) => continue,
        };
        let hwnd = unsafe { browser.HWND().ok()? };
        if hwnd.0 != window {
            continue;
        }
        let location = unsafe { browser.LocationURL().ok()? }.to_string();
        return url::Url::parse(&location)
            .ok()
            .and_then(|url| url.to_file_path().ok());
    }
    None
}

/// 返回当前用户桌面目录。
fn desktop_folder() -> Option<PathBuf> {
    let mut buffer = [0u16; 260];
    unsafe {
        SHGetFolderPathW(None, CSIDL_DESKTOPDIRECTORY as i32, None, 0, &mut buffer).ok()?;
    }
    let length = buffer.iter().position(|value| *value == 0)?;
    let path = String::from_utf16(&buffer[..length]).ok()?;
    let path = PathBuf::from(path);
    path.is_dir().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `file:` URL 的路径部分能还原为 Windows 路径。
    #[test]
    fn file_url_to_path() {
        let url = url::Url::parse("file:///D:/work/%E4%B8%AD%E6%96%87").unwrap();
        assert_eq!(url.to_file_path().unwrap(), PathBuf::from("D:\\work\\中文"));
    }
}
