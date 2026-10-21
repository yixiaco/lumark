//! 从 Lumark 窗口向资源管理器发起原生文件拖放。
//!
//! GPUI 当前只在部分平台实现出站拖放，Windows 后端没有提供直接入口。
//! 这里用 Shell 的 `IShellItemArray` 构造 `IDataObject`，再由 `SHDoDragDrop`
//! 驱动系统拖放循环；临时文件由调用方提前解压并保证存在。

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, S_OK,
};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, IDropSource, IDropSource_Impl};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, ILCreateFromPathW, ILFree, IShellItemArray, SHCreateShellItemArrayFromIDLists,
    SHDoDragDrop,
};
use windows::core::{BOOL, HRESULT, PCWSTR, implement};

use crate::PlatformError;

/// 用指定窗口作为拖放源，把真实文件路径拖到资源管理器或其他应用。
///
/// 返回是否发生了实际投放；用户提前松开左键时返回 `false`。
pub fn start_file_drag(hwnd: isize, paths: &[PathBuf]) -> Result<bool, PlatformError> {
    // 解压临时文件可能耗时；如果用户已经松开左键，不再启动系统拖放循环。
    if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
        return Ok(false);
    }
    let data_object = create_data_object(paths)?;
    let source: IDropSource = FileDropSource.into();
    let effect = unsafe {
        SHDoDragDrop(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            &data_object,
            &source,
            DROPEFFECT_COPY,
        )
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?
    };
    Ok(effect.0 != 0)
}

/// 由真实文件路径构造 Shell 数据对象，包含资源管理器需要的 `CF_HDROP`。
fn create_data_object(paths: &[PathBuf]) -> Result<IDataObject, PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::EmptyPaths);
    }
    let pidls = paths
        .iter()
        .map(|path| unsafe { create_pidl(path) })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| PlatformError::Windows(std::io::Error::other("无法创建拖放路径")))?;

    let result = (|| -> windows::core::Result<IDataObject> {
        let array: IShellItemArray = unsafe { SHCreateShellItemArrayFromIDLists(&pidls)? };
        unsafe { array.BindToHandler(None, &BHID_DataObject) }
    })();
    for pidl in pidls {
        unsafe { ILFree(Some(pidl)) };
    }
    result.map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))
}

/// 把路径转换为 Shell PIDL；调用方负责释放。
unsafe fn create_pidl(path: &Path) -> Option<*const ITEMIDLIST> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let pidl = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
    (!pidl.is_null()).then_some(pidl.cast_const())
}

/// 只处理按钮状态的最小拖放源；不接管绘制，交给系统默认拖放光标。
#[implement(IDropSource)]
struct FileDropSource;

impl IDropSource_Impl for FileDropSource_Impl {
    /// 松开左键时完成拖放，按 Esc 时取消。
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    /// 使用系统默认拖放光标。
    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{CF_HDROP, OleInitialize, OleUninitialize};

    /// 验证 Shell 数据对象能回答资源管理器对 `CF_HDROP` 的格式查询。
    #[test]
    fn shell_data_object_exposes_hdrop() {
        let initialized = unsafe { OleInitialize(None) }.is_ok();
        let path =
            std::env::temp_dir().join(format!("lumark-drag-test-{}.txt", std::process::id()));
        std::fs::write(&path, b"drag").unwrap();
        let data_object = create_data_object(std::slice::from_ref(&path)).unwrap();
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        assert_eq!(unsafe { data_object.QueryGetData(&format) }, S_OK);
        let _ = std::fs::remove_file(path);
        if initialized {
            unsafe { OleUninitialize() };
        }
    }
}
