//! 自解压模块入口：解压自身携带的数据，并在完成或失败时给出提示。
//!
//! 作为 Windows GUI 子系统程序运行，不弹出控制台窗口。非静默模式先显示
//! 目录和密码窗口，允许用户选择解压位置；失败信息通过消息框展示，成功后
//! 用资源管理器打开解压目录。

#![windows_subsystem = "windows"]
// Win32 回调本身必须声明为 unsafe extern；内部 FFI 调用集中在这个文件，
// 统一由 Windows API 的安全约定约束。
#![allow(unsafe_op_in_unsafe_fn)]

use std::fs::File;
use std::path::{Path, PathBuf};

use lumark_sfx::{SfxError, SfxFooter, extract_payload_with_password, read_footer};
use zeroize::Zeroizing;

mod lang;

/// 运行自解压流程；失败时用消息框提示用户。
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if let Some(language) = lang::language_override(&arguments) {
        // SAFETY: 进程启动早期、尚未创建工作线程时设置环境变量。
        unsafe {
            std::env::set_var("LUMARK_LANG", language);
        }
    }
    if let Err(error) = run() {
        show_message(&error, true);
    }
}

/// 读取自身页脚并解压数据段，最后打开目标目录。
fn run() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| {
        format!(
            "{}：{error}",
            lang::t("无法定位自身文件", "Cannot locate executable")
        )
    })?;
    let mut file = File::open(&exe).map_err(|error| {
        format!(
            "{}：{error}",
            lang::t("无法打开自身文件", "Cannot open executable")
        )
    })?;
    let footer = read_footer(&mut file)
        .map_err(|error| {
            format!(
                "{}：{error}",
                lang::t("无法读取自解压页脚", "Cannot read self-extractor footer")
            )
        })?
        .ok_or_else(|| {
            lang::t(
                "这不是有效的 Lumark 自解压文件",
                "This is not a valid Lumark self-extracting file",
            )
            .to_string()
        })?;
    drop(file);

    let plan = lumark_sfx::parse_args(std::env::args().skip(1).collect::<Vec<_>>());
    let default_target = lumark_sfx::default_target(&exe);
    let mut target = plan.target.clone().unwrap_or(default_target);
    let mut password = plan.password.clone().or_else(password_from_environment);
    let mut overwrite = plan.overwrite;

    if plan.silent {
        if footer.encrypted && password.is_none() {
            return Err(lang::t(
                "静默模式下的自解压包需要 /P=密码或 LUMARK_SFX_PASSWORD 环境变量",
                "Silent self-extraction requires /P=password or the LUMARK_SFX_PASSWORD environment variable",
            )
            .to_string());
        }
        extract(
            &exe,
            footer,
            &target,
            password.as_ref().map(|value| value.as_str()),
            overwrite,
        )
        .map_err(|error| localized_error(&error))?;
        return Ok(());
    }

    let mut error: Option<String> = None;
    loop {
        let Some(result) = show_dialog(
            &target,
            footer,
            password.as_ref().map(|value| value.as_str()),
            error.as_deref(),
        ) else {
            return Ok(());
        };
        target = result.target;
        password = result.password;

        if target.as_os_str().is_empty() {
            error =
                Some(lang::t("请选择解压目录", "Please choose an extraction folder").to_string());
            continue;
        }
        if target.exists() && !overwrite && !directory_is_empty(&target) {
            if !confirm_overwrite(&target) {
                error = Some(lang::t("目标目录已有文件，请选择其他目录或确认覆盖", "The target folder is not empty; choose another folder or confirm overwrite").to_string());
                continue;
            }
            overwrite = true;
        }

        match extract(
            &exe,
            footer,
            &target,
            password.as_ref().map(|value| value.as_str()),
            overwrite,
        ) {
            Ok(_) => {
                reveal(&target);
                show_message(
                    &format!(
                        "{}\n{}",
                        lang::t("已解压到：", "Extracted to:"),
                        target.display()
                    ),
                    false,
                );
                return Ok(());
            }
            Err(SfxError::WrongPassword) => {
                error = Some(
                    lang::t("密码错误，请重新输入", "Wrong password; please try again").to_string(),
                );
                password = None;
            }
            Err(error_value) => {
                error = Some(localized_error(&error_value));
            }
        }
    }
}

/// 从环境变量读取密码，避免把密码暴露在命令行中。
fn password_from_environment() -> Option<Zeroizing<String>> {
    password_from_value(std::env::var("LUMARK_SFX_PASSWORD").ok())
}

/// 把可选环境变量值转换为非空密码。
fn password_from_value(value: Option<String>) -> Option<Zeroizing<String>> {
    value
        .filter(|password| !password.is_empty())
        .map(Zeroizing::new)
}

/// 把 SFX 错误转换为当前界面语言。
fn localized_error(error: &SfxError) -> String {
    localized_error_language(error, lang::is_english())
}

/// 按指定语言转换 SFX 错误。
fn localized_error_language(error: &SfxError, english: bool) -> String {
    let text = |zh: &'static str, en: &'static str| if english { en } else { zh };
    match error {
        SfxError::PasswordRequired => {
            text("该自解压包需要密码", "This package requires a password").to_string()
        }
        SfxError::WrongPassword => {
            text("密码错误，请重新输入", "Wrong password; please try again").to_string()
        }
        SfxError::Other(message) => message.clone(),
    }
}

/// 重新打开自身文件并解压；每次密码重试都使用新的数据段读取器。
fn extract(
    exe: &Path,
    footer: SfxFooter,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
) -> Result<usize, SfxError> {
    let file = File::open(exe).map_err(|error| {
        SfxError::Other(format!(
            "{}：{error}",
            lang::t("无法打开自身文件", "Cannot open executable")
        ))
    })?;
    extract_payload_with_password(file, footer, target, password, overwrite)
}

/// 在资源管理器中打开解压目录。
fn reveal(target: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(target)
        .spawn();
}

/// 判断目录是否已有内容；读取失败时按非空处理，避免静默覆盖。
fn directory_is_empty(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

/// 显示消息框；`error` 为真时使用错误图标。
#[cfg(windows)]
fn show_message(message: &str, error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
    };

    let text = wide(message);
    let title = wide(lang::t("Lumark 自解压", "Lumark Self-Extractor"));
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    // 消息框只展示静态文本，句柄传空表示不绑定父窗口。
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | icon,
        );
    }
}

/// 非 Windows 平台直接输出到标准错误，方便开发时排查。
#[cfg(not(windows))]
fn show_message(message: &str, _error: bool) {
    eprintln!("{message}");
}

/// 覆盖确认；返回 `true` 表示继续解压并允许覆盖。
#[cfg(windows)]
fn confirm_overwrite(target: &Path) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONWARNING, MB_YESNO, MessageBoxW,
    };

    let text = wide(&format!(
        "{}\n{}\n\n{}",
        lang::t("目标目录已有文件：", "The target folder is not empty:"),
        target.display(),
        lang::t("是否覆盖同名文件？", "Overwrite files with the same name?")
    ));
    let title = wide(lang::t("Lumark 自解压", "Lumark Self-Extractor"));
    let result = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING,
        )
    };
    result == IDYES
}

/// 非 Windows 平台缺少系统对话框，默认不覆盖。
#[cfg(not(windows))]
fn confirm_overwrite(_target: &Path) -> bool {
    false
}

/// 对话框返回的解压参数。
#[cfg(windows)]
struct DialogResult {
    target: PathBuf,
    password: Option<Zeroizing<String>>,
}

/// 目录/密码窗口的控件和临时状态。
#[cfg(windows)]
struct DialogState {
    encrypted: bool,
    result: Option<DialogResult>,
    target_edit: windows_sys::Win32::Foundation::HWND,
    password_edit: windows_sys::Win32::Foundation::HWND,
    error_hwnd: windows_sys::Win32::Foundation::HWND,
}

/// 显示目录和密码窗口；取消时返回 `None`。
#[cfg(windows)]
fn show_dialog(
    default_target: &Path,
    footer: SfxFooter,
    initial_password: Option<&str>,
    error: Option<&str>,
) -> Option<DialogResult> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, IsDialogMessageW, RegisterClassW, SW_SHOW,
        SendMessageW, SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_SETFONT, WNDCLASSW,
        WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
        WS_VISIBLE,
    };

    /// 窗口类只注册一次。
    static REGISTER: std::sync::Once = std::sync::Once::new();
    /// 自解压目录/密码窗口的窗口类名。
    const CLASS_NAME: &str = "LumarkSfxDialog";
    /// 浏览目录按钮的控件 ID。
    const ID_BROWSE: usize = 101;
    /// 密码输入框的控件 ID。
    const ID_PASSWORD: usize = 102;
    /// 开始解压按钮的控件 ID。
    const ID_EXTRACT: usize = 103;
    /// 取消按钮的控件 ID。
    const ID_CANCEL: usize = 104;
    /// 解压目录输入框的控件 ID。
    const ID_TARGET: usize = 105;
    /// 错误提示文本的控件 ID。
    const ID_ERROR: usize = 106;

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DefWindowProcW, DestroyWindow, GetWindowLongPtrW, PostQuitMessage, SetWindowTextW,
            WM_CLOSE, WM_COMMAND, WM_DESTROY,
        };

        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
        match message {
            WM_COMMAND => {
                if state_ptr.is_null() {
                    return DefWindowProcW(hwnd, message, wparam, lparam);
                }
                let id = wparam & 0xffff;
                let state = &mut *state_ptr;
                match id {
                    ID_BROWSE => {
                        if let Some(path) = browse_for_folder(hwnd) {
                            let text = wide(&path.display().to_string());
                            SetWindowTextW(state.target_edit, text.as_ptr());
                        }
                    }
                    ID_EXTRACT => {
                        let target = get_text(state.target_edit);
                        if target.trim().is_empty() {
                            let text = wide(lang::t("请选择解压目录", "Choose extraction folder"));
                            SetWindowTextW(state.error_hwnd, text.as_ptr());
                            return 0;
                        }
                        let password = if state.encrypted {
                            Some(Zeroizing::new(get_text(state.password_edit)))
                        } else {
                            None
                        };
                        state.result = Some(DialogResult {
                            target: PathBuf::from(target),
                            password,
                        });
                        DestroyWindow(hwnd);
                    }
                    ID_CANCEL => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe {
        REGISTER.call_once(|| {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class_name = wide(CLASS_NAME);
            let cursor = windows_sys::Win32::UI::WindowsAndMessaging::LoadCursorW(
                std::ptr::null_mut(),
                windows_sys::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            );
            let window_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: std::ptr::null_mut(),
                hCursor: cursor,
                hbrBackground: (COLOR_WINDOW + 1) as _,
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&window_class);
        });

        let hinstance = GetModuleHandleW(std::ptr::null());
        if hinstance.is_null() {
            return None;
        }
        let class_name = wide(CLASS_NAME);
        let title = wide(lang::t("Lumark 自解压", "Lumark Self-Extractor"));
        let height = if footer.encrypted { 250 } else { 210 };
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            100,
            100,
            470,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return None;
        }

        let mut state = Box::new(DialogState {
            encrypted: footer.encrypted,
            result: None,
            target_edit: std::ptr::null_mut(),
            password_edit: std::ptr::null_mut(),
            error_hwnd: std::ptr::null_mut(),
        });
        let state_ptr = &mut *state as *mut DialogState;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

        let font = GetStockObject(DEFAULT_GUI_FONT);
        let make = |text: &str,
                    class: &str,
                    style: i32,
                    x: i32,
                    y: i32,
                    width: i32,
                    control_height: i32,
                    id: usize|
         -> HWND {
            let class = wide(class);
            let text = wide(text);
            let child = CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | style as u32,
                x,
                y,
                width,
                control_height,
                hwnd,
                id as _,
                hinstance,
                std::ptr::null(),
            );
            if !child.is_null() && !font.is_null() {
                SendMessageW(child, WM_SETFONT, font as WPARAM, 1);
            }
            child
        };

        make(
            lang::t("解压到：", "Extract to:"),
            "STATIC",
            0,
            16,
            14,
            420,
            20,
            0,
        );
        state.target_edit = make(
            &default_target.display().to_string(),
            "EDIT",
            windows_sys::Win32::UI::WindowsAndMessaging::ES_AUTOHSCROLL,
            16,
            36,
            340,
            24,
            ID_TARGET,
        );
        make(
            lang::t("浏览…", "Browse..."),
            "BUTTON",
            0,
            364,
            36,
            80,
            24,
            ID_BROWSE,
        );
        if footer.encrypted {
            make(
                lang::t("密码：", "Password:"),
                "STATIC",
                0,
                16,
                68,
                420,
                20,
                0,
            );
            state.password_edit = make(
                initial_password.unwrap_or_default(),
                "EDIT",
                windows_sys::Win32::UI::WindowsAndMessaging::ES_PASSWORD
                    | windows_sys::Win32::UI::WindowsAndMessaging::ES_AUTOHSCROLL,
                16,
                88,
                428,
                24,
                ID_PASSWORD,
            );
        }
        let error_hwnd = make(
            error.unwrap_or_default(),
            "STATIC",
            0,
            16,
            122,
            428,
            40,
            ID_ERROR,
        );
        state.error_hwnd = error_hwnd;
        make(
            lang::t("解压", "Extract"),
            "BUTTON",
            0,
            300,
            height - 44,
            70,
            26,
            ID_EXTRACT,
        );
        make(
            lang::t("取消", "Cancel"),
            "BUTTON",
            0,
            378,
            height - 44,
            70,
            26,
            ID_CANCEL,
        );

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        if !state.target_edit.is_null() {
            SetFocus(state.target_edit);
        }

        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        state.result.take()
    }
}

/// 非 Windows 平台不提供目录选择窗口。
#[cfg(not(windows))]
fn show_dialog(
    _default_target: &Path,
    _footer: SfxFooter,
    _initial_password: Option<&str>,
    _error: Option<&str>,
) -> Option<DialogResult> {
    None
}

/// 非 Windows 平台的占位结果，保持编译单元完整。
#[cfg(not(windows))]
struct DialogResult {
    target: PathBuf,
    password: Option<Zeroizing<String>>,
}

/// 选择目录；取消时返回 `None`。
#[cfg(windows)]
fn browse_for_folder(owner: windows_sys::Win32::Foundation::HWND) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    };

    let mut display = [0u16; 260];
    let title = wide(lang::t("选择解压目录", "Choose extraction folder"));
    let browse = BROWSEINFOW {
        hwndOwner: owner,
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: display.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    let pidl = unsafe { SHBrowseForFolderW(&browse) };
    if pidl.is_null() {
        return None;
    }
    let mut path = [0u16; 260];
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    unsafe { CoTaskMemFree(pidl as *const _) };
    if ok == 0 {
        return None;
    }
    Some(PathBuf::from(from_wide(&path)))
}

/// 读取编辑框文本。
#[cfg(windows)]
fn get_text(hwnd: windows_sys::Win32::Foundation::HWND) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    if hwnd.is_null() {
        return String::new();
    }
    let length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let read = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    buffer.truncate(read.max(0) as usize);
    String::from_utf16_lossy(&buffer)
}

/// 把 Rust 字符串转换为以 NUL 结尾的 UTF-16。
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 把以 NUL 结尾的 UTF-16 缓冲区转换为 Rust 字符串。
#[cfg(windows)]
fn from_wide(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|item| *item == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA;

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证环境变量密码只接受非空值。
    #[test]
    fn environment_password_ignores_empty_values() {
        assert!(password_from_value(None).is_none());
        assert!(password_from_value(Some(String::new())).is_none());
        assert_eq!(
            password_from_value(Some("secret".to_string()))
                .as_deref()
                .map(String::as_str),
            Some("secret")
        );
    }

    /// 验证错误文本按语言切换。
    #[test]
    fn errors_follow_language() {
        assert_eq!(
            localized_error_language(&SfxError::PasswordRequired, true),
            "This package requires a password"
        );
        assert_eq!(
            localized_error_language(&SfxError::WrongPassword, false),
            "密码错误，请重新输入"
        );
    }
}
