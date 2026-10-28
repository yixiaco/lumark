//! Lumark 的 Windows 11 资源管理器上下文菜单 COM 服务器。
//!
//! 该 DLL 由稀疏 MSIX 包中的 `windows.comServer` 注册，提供 `IExplorerCommand`，
//! 点击菜单后启动已安装的 Lumark 并传入 `--compress` 和当前选中路径。

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_NOTIMPL, E_OUTOFMEMORY, S_FALSE};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::UI::Shell::{
    ECF_DEFAULT, ECS_ENABLED, IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, SIGDN_FILESYSPATH,
};
use windows::core::{GUID, HRESULT, IUnknown, Interface, PWSTR, Ref, Result, implement};
use windows_core::BOOL;

/// 上下文菜单命令的 CLSID，必须与稀疏包清单中的 `com:Class/@Id` 一致。
pub const COMMAND_CLSID: GUID = GUID::from_u128(0x8f1d2c3a_4b5e_4f60_9a7b_1c2d3e4f5a6b);

/// 极简 `IExplorerCommand` 实现。
#[implement(IExplorerCommand)]
struct LumarkCommand;

impl IExplorerCommand_Impl for LumarkCommand_Impl {
    /// 菜单标题。
    fn GetTitle(&self, _items: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        wide_task_mem(command_title())
    }

    /// 菜单图标由可执行文件提供。
    fn GetIcon(&self, _items: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        Ok(PWSTR::null())
    }

    /// 不提供额外提示。
    fn GetToolTip(&self, _items: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        Ok(PWSTR::null())
    }

    /// 不参与命令规范化。
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::zeroed())
    }

    /// 始终启用；实际参数校验留给主程序。
    fn GetState(&self, _items: Ref<'_, IShellItemArray>, _slow: BOOL) -> Result<u32> {
        Ok(ECS_ENABLED.0 as u32)
    }

    /// 把选中路径交给已安装的 Lumark。
    fn Invoke(&self, items: Ref<'_, IShellItemArray>, _bind: Ref<'_, IBindCtx>) -> Result<()> {
        let Some(items) = items.as_ref() else {
            return Ok(());
        };
        let count = unsafe { items.GetCount()? };
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            let item = unsafe { items.GetItemAt(index)? };
            let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? };
            if name.is_null() {
                continue;
            }
            let value = unsafe { name.to_string()? };
            unsafe { CoTaskMemFree(Some(name.0 as *const _)) };
            paths.push(PathBuf::from(value));
        }
        Command::new(lumark_executable())
            .arg("--compress")
            .args(paths)
            .spawn()
            .map_err(|error| {
                windows::core::Error::from_hresult(HRESULT::from_win32(
                    error.raw_os_error().unwrap_or(2) as u32,
                ))
            })?;
        Ok(())
    }

    /// 使用默认菜单项行为。
    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    /// 当前没有子命令。
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }
}

/// 返回当前用户界面语言的菜单标题。
fn command_title() -> &'static str {
    title_for_language(unsafe { GetUserDefaultUILanguage() })
}

/// 按 Windows 主语言 ID 选择菜单标题。
fn title_for_language(language: u16) -> &'static str {
    if language & 0x3ff == 0x04 {
        "用 Lumark 压缩"
    } else {
        "Compress with Lumark"
    }
}

/// 返回已安装的 Lumark 可执行文件路径。
fn lumark_executable() -> PathBuf {
    if let Some(path) = std::env::var_os("LUMARK_EXE") {
        return PathBuf::from(path);
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(key) {
            let candidate = PathBuf::from(root).join("Lumark").join("Lumark.exe");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(parent) = current.parent()
    {
        let candidate = parent.join("Lumark.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("Lumark.exe")
}

/// 使用 COM 任务分配器复制 UTF-16 字符串。
fn wide_task_mem(value: &str) -> Result<PWSTR> {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let memory = unsafe { CoTaskMemAlloc(bytes) } as *mut u16;
    if memory.is_null() {
        return Err(windows::core::Error::from_hresult(E_OUTOFMEMORY));
    }
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), memory, wide.len()) };
    Ok(PWSTR(memory))
}

/// COM 类工厂，负责创建 `LumarkCommand`。
#[implement(IClassFactory)]
struct CommandFactory;

impl IClassFactory_Impl for CommandFactory_Impl {
    /// 创建命令对象。
    fn CreateInstance(
        &self,
        _outer: Ref<'_, IUnknown>,
        riid: *const GUID,
        object: *mut *mut std::ffi::c_void,
    ) -> Result<()> {
        let command: IExplorerCommand = LumarkCommand.into();
        let unknown: IUnknown = command.cast()?;
        unsafe { unknown.query(riid, object) }.ok()
    }

    /// DLL 生命周期由 COM 管理。
    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// 标准的 COM 类工厂入口。
///
/// # Safety
///
/// 调用方必须传入有效的 CLSID、IID 和输出指针；这些指针由 COM 运行时管理，
/// 本函数只在指针非空时读取和写入。
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    interface: *const GUID,
    object: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if class.is_null() || interface.is_null() || object.is_null() {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    if unsafe { *class } != COMMAND_CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = CommandFactory.into();
    let unknown: IUnknown = match factory.cast() {
        Ok(unknown) => unknown,
        Err(error) => return error.code(),
    };
    unsafe { unknown.query(interface, object) }
}

/// 当前 DLL 可以被卸载。
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 COM 类工厂可以创建 IExplorerCommand。
    #[test]
    fn class_factory_creates_command() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
            let mut factory = std::ptr::null_mut();
            let result = DllGetClassObject(&COMMAND_CLSID, &IClassFactory::IID, &mut factory);
            assert!(result.is_ok(), "DllGetClassObject failed: {result:?}");
            assert!(!factory.is_null());
            let unknown = IUnknown::from_raw(factory);
            drop(unknown);
            windows::Win32::System::Com::CoUninitialize();
        }
    }

    /// 验证菜单标题跟随 Windows 主语言。
    #[test]
    fn title_follows_ui_language() {
        assert_eq!(title_for_language(0x0804), "用 Lumark 压缩");
        assert_eq!(title_for_language(0x0409), "Compress with Lumark");
    }

    /// CLSID 必须和清单保持稳定。
    #[test]
    fn clsid_is_stable() {
        assert_eq!(
            format!("{:?}", COMMAND_CLSID),
            "8F1D2C3A-4B5E-4F60-9A7B-1C2D3E4F5A6B"
        );
    }
}
