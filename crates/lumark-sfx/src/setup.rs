//! 原生自解压设置窗口，布局参考 360 压缩的自解压包对话框。
//!
//! 选择原生 Win32 控件而不是 GPUI：360 的 QVM 启发式会把带 DirectX/GPUI
//! 的自解压 stub 判为木马，而原生 stub 已由用户实测不触发该误报。

use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use lumark_sfx::SfxFooter;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::HFONT;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::HICON;

use crate::lang;
#[cfg(windows)]
use crate::win;

/// 设置窗口返回的解压参数。
#[cfg(windows)]
pub(crate) struct DialogResult {
    /// 解压目标目录。
    pub target: PathBuf,
    /// 用户输入的密码；未加密时为 `None`。
    pub password: Option<Zeroizing<String>>,
}

/// 设置窗口的控件和临时状态。
#[cfg(windows)]
struct DialogState {
    encrypted: bool,
    result: Option<DialogResult>,
    target_edit: HWND,
    password_edit: HWND,
    error_hwnd: HWND,
    normal_font: HFONT,
    body_font: HFONT,
    header_font: HFONT,
    banner_icon: HICON,
}

/// 显示目录和密码窗口；取消时返回 `None`。
#[cfg(windows)]
pub(crate) fn show(
    default_target: &Path,
    footer: SfxFooter,
    initial_password: Option<&str>,
    error: Option<&str>,
) -> Option<DialogResult> {
    use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, UpdateWindow};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, CreateWindowExW, DispatchMessageW, GetMessageW, GetSystemMetrics,
        IsDialogMessageW, LoadImageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW,
        SetWindowLongPtrW, ShowWindow, TranslateMessage, WNDCLASSW, WS_CAPTION, WS_CHILD,
        WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
        WS_VISIBLE,
    };

    /// 窗口类只注册一次。
    static REGISTER: std::sync::Once = std::sync::Once::new();
    /// 设置窗口的窗口类名。
    const CLASS_NAME: &str = "LumarkSfxDialog";
    /// 浏览目录按钮的控件 ID。
    const ID_BROWSE: usize = 101;
    /// 密码输入框的控件 ID。
    const ID_PASSWORD: usize = 102;
    /// 开始解压按钮的控件 ID。
    const ID_EXTRACT: usize = 103;
    /// 取消按钮使用标准 IDCANCEL，Esc 和标题栏关闭都走同一路径。
    const ID_CANCEL: usize = 2;
    /// 解压目录输入框的控件 ID。
    const ID_TARGET: usize = 105;
    /// 错误提示文本的控件 ID。
    const ID_ERROR: usize = 106;
    /// 默认按钮消息，和对话框管理器使用同一个值。
    const DM_SETDEFID: u32 = 0x0401;

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, PostQuitMessage,
            WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_PAINT,
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
                            win::set_text(state.target_edit, &path.display().to_string());
                        }
                    }
                    ID_EXTRACT => {
                        let target = win::text_of(state.target_edit);
                        if target.trim().is_empty() {
                            win::set_text(
                                state.error_hwnd,
                                lang::t("请选择解压目录", "Choose extraction folder"),
                            );
                            return 0;
                        }
                        let password = if state.encrypted {
                            let value = win::text_of(state.password_edit);
                            if value.is_empty() {
                                win::set_text(
                                    state.error_hwnd,
                                    lang::t("请输入解压密码", "Enter the extraction password"),
                                );
                                SetFocus(state.password_edit);
                                return 0;
                            }
                            Some(Zeroizing::new(value))
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
            WM_PAINT => {
                if !state_ptr.is_null() {
                    paint_banner(hwnd, &*state_ptr);
                }
                0
            }
            WM_ERASEBKGND => 1,
            WM_CTLCOLORSTATIC => {
                use windows_sys::Win32::Graphics::Gdi::{
                    GetStockObject, SetBkMode, SetTextColor, TRANSPARENT, WHITE_BRUSH,
                };
                let hdc = wparam as _;
                SetBkMode(hdc, TRANSPARENT as i32);
                if state_ptr.is_null()
                    || lparam as HWND != (*state_ptr).error_hwnd
                    || (*state_ptr).error_hwnd.is_null()
                {
                    SetTextColor(hdc, 0x00141414);
                } else {
                    SetTextColor(hdc, 0x002020C0);
                }
                GetStockObject(WHITE_BRUSH) as _
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        if hinstance.is_null() {
            return None;
        }
        let icon =
            windows_sys::Win32::UI::WindowsAndMessaging::LoadIconW(hinstance, win::icon_resource());
        REGISTER.call_once(|| {
            let class_name = win::wide(CLASS_NAME);
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
                hIcon: icon,
                hCursor: cursor,
                hbrBackground: (COLOR_WINDOW + 1) as _,
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&window_class);
        });

        let class_name = win::wide(CLASS_NAME);
        let title = win::wide(lang::t("Lumark 自解压包", "Lumark Self-Extractor"));
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let ex_style = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
        let client_width = win::scale(720);
        let client_height = win::scale(540);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: client_width,
            bottom: client_height,
        };
        AdjustWindowRectEx(&mut rect, style, 0, ex_style);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let x = ((screen_width - width) / 2).max(0);
        let y = ((screen_height - height) / 2).max(0);
        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            width,
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
            normal_font: win::make_font(9, 400),
            body_font: win::make_font(10, 400),
            header_font: win::make_font(16, 600),
            banner_icon: LoadImageW(
                hinstance,
                win::icon_resource(),
                windows_sys::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
                win::scale(64),
                win::scale(64),
                windows_sys::Win32::UI::WindowsAndMessaging::LR_DEFAULTCOLOR,
            ) as HICON,
        });
        SetWindowLongPtrW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            &mut *state as *mut DialogState as win::LongPtr,
        );

        let normal = if state.normal_font.is_null() {
            windows_sys::Win32::Graphics::Gdi::GetStockObject(
                windows_sys::Win32::Graphics::Gdi::DEFAULT_GUI_FONT,
            ) as HFONT
        } else {
            state.normal_font
        };
        let body = if state.body_font.is_null() {
            normal
        } else {
            state.body_font
        };
        let header = if state.header_font.is_null() {
            body
        } else {
            state.header_font
        };
        let content_x = win::scale(174);
        let content_width = win::scale(522);
        let label_height = win::scale(22);
        let edit_height = win::scale(28);
        let make = |text: &str,
                    class: &str,
                    style: i32,
                    x: i32,
                    y: i32,
                    width: i32,
                    height: i32,
                    id: usize,
                    font: HFONT|
         -> HWND {
            let is_edit = class.eq_ignore_ascii_case("EDIT");
            let ex_style = if is_edit {
                windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_CLIENTEDGE
            } else {
                0
            };
            let class = win::wide(class);
            let text = win::wide(text);
            let control = CreateWindowExW(
                ex_style,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | style as u32,
                x,
                y,
                width,
                height,
                hwnd,
                id as _,
                hinstance,
                std::ptr::null(),
            );
            win::set_font(control, font);
            control
        };
        make(
            lang::t("Lumark 自解压包", "Lumark Self-Extractor"),
            "STATIC",
            0,
            content_x,
            win::scale(24),
            content_width,
            win::scale(34),
            0,
            header,
        );
        make(
            lang::t(
                "单击“解压”按钮开始解压。\r\n\r\n使用“浏览”按钮从目录树中选择目标文件夹，也可以手动输入。\r\n\r\n如果指定的目标文件夹不存在，在文件解压前将自动创建。",
                "Click Extract to start.\r\n\r\nUse Browse to choose the target folder from the directory tree, or type it manually.\r\n\r\nIf the target folder does not exist, it will be created before extraction.",
            ),
            "STATIC",
            0,
            content_x,
            win::scale(68),
            content_width,
            win::scale(96),
            0,
            body,
        );
        let target_y = if footer.encrypted {
            win::scale(262)
        } else {
            win::scale(300)
        };
        make(
            lang::t("解压缩目标文件夹 (X)：", "Extract to folder (X):"),
            "STATIC",
            0,
            content_x,
            target_y,
            content_width,
            label_height,
            0,
            normal,
        );
        let edit_y = target_y + win::scale(22);
        let browse_width = win::scale(96);
        let edit_width = content_width - browse_width - win::scale(10);
        state.target_edit = make(
            &default_target.display().to_string(),
            "EDIT",
            windows_sys::Win32::UI::WindowsAndMessaging::ES_AUTOHSCROLL,
            content_x,
            edit_y,
            edit_width,
            edit_height,
            ID_TARGET,
            normal,
        );
        make(
            lang::t("浏览…", "Browse..."),
            "BUTTON",
            0,
            content_x + edit_width + win::scale(10),
            edit_y,
            browse_width,
            edit_height,
            ID_BROWSE,
            normal,
        );
        if footer.encrypted {
            let password_y = win::scale(340);
            make(
                lang::t("解压密码 (P)：", "Password (P):"),
                "STATIC",
                0,
                content_x,
                password_y,
                content_width,
                label_height,
                0,
                normal,
            );
            state.password_edit = make(
                initial_password.unwrap_or_default(),
                "EDIT",
                windows_sys::Win32::UI::WindowsAndMessaging::ES_PASSWORD
                    | windows_sys::Win32::UI::WindowsAndMessaging::ES_AUTOHSCROLL,
                content_x,
                password_y + win::scale(22),
                content_width,
                edit_height,
                ID_PASSWORD,
                normal,
            );
        }
        let error_y = win::scale(408);
        state.error_hwnd = make(
            error.unwrap_or_default(),
            "STATIC",
            0,
            content_x,
            error_y,
            content_width,
            win::scale(44),
            ID_ERROR,
            normal,
        );
        // 底部按钮按客户区高度定位；外框尺寸不再参与控件坐标。
        let button_y = client_height - win::scale(50);
        let button_width = win::scale(100);
        let button_height = win::scale(32);
        make(
            lang::t("解压", "Extract"),
            "BUTTON",
            windows_sys::Win32::UI::WindowsAndMessaging::BS_DEFPUSHBUTTON,
            client_width - win::scale(234),
            button_y,
            button_width,
            button_height,
            ID_EXTRACT,
            normal,
        );
        make(
            lang::t("取消", "Cancel"),
            "BUTTON",
            0,
            client_width - win::scale(124),
            button_y,
            button_width,
            button_height,
            ID_CANCEL,
            normal,
        );
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, DM_SETDEFID, ID_EXTRACT, 0);
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

        let result = state.result.take();
        win::delete_font(state.normal_font);
        win::delete_font(state.body_font);
        win::delete_font(state.header_font);
        if !state.banner_icon.is_null() {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(state.banner_icon);
        }
        let _ = icon;
        result
    }
}

/// 非 Windows 平台没有原生设置窗口。
#[cfg(not(windows))]
pub(crate) struct DialogResult;

/// 非 Windows 平台直接返回取消，由调用方输出错误。
#[cfg(not(windows))]
pub(crate) fn show(
    _default_target: &Path,
    _footer: SfxFooter,
    _initial_password: Option<&str>,
    _error: Option<&str>,
) -> Option<DialogResult> {
    None
}

/// 绘制左侧品牌栏、分隔线，并在高 DPI 下按比例缩放。
#[cfg(windows)]
unsafe fn paint_banner(hwnd: HWND, state: &DialogState) {
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DrawTextW, EndPaint,
        FillRect, HGDIOBJ, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx, GetClientRect};

    let mut paint = PAINTSTRUCT {
        hdc: std::ptr::null_mut(),
        fErase: 0,
        rcPaint: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        fRestore: 0,
        fIncUpdate: 0,
        rgbReserved: [0; 32],
    };
    let hdc = BeginPaint(hwnd, &mut paint);
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut client);
    let banner_width = win::scale(150);
    let background = CreateSolidBrush(0x00F4EDE4);
    FillRect(
        hdc,
        &RECT {
            left: 0,
            top: 0,
            right: banner_width,
            bottom: client.bottom,
        },
        background,
    );
    windows_sys::Win32::Graphics::Gdi::DeleteObject(background as HGDIOBJ);
    let separator = CreateSolidBrush(0x00E2D8CE);
    FillRect(
        hdc,
        &RECT {
            left: banner_width,
            top: 0,
            right: banner_width + 1,
            bottom: client.bottom,
        },
        separator,
    );
    FillRect(
        hdc,
        &RECT {
            left: 0,
            top: client.bottom - win::scale(64),
            right: client.right,
            bottom: client.bottom - win::scale(63),
        },
        separator,
    );
    // 说明文字与目标目录之间加一条细分隔线，贴近参考布局的分组感。
    FillRect(
        hdc,
        &RECT {
            left: win::scale(174),
            top: win::scale(244),
            right: client.right - win::scale(24),
            bottom: win::scale(245),
        },
        separator,
    );
    windows_sys::Win32::Graphics::Gdi::DeleteObject(separator as HGDIOBJ);
    if !state.banner_icon.is_null() {
        DrawIconEx(
            hdc,
            win::scale(43),
            win::scale(36),
            state.banner_icon,
            win::scale(64),
            win::scale(64),
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        );
    }
    SetBkMode(hdc, TRANSPARENT as i32);
    let old_font = SelectObject(hdc, state.header_font as HGDIOBJ);
    SetTextColor(hdc, 0x007A3F17);
    let mut text = win::wide("Lumark");
    let mut rect = RECT {
        left: win::scale(24),
        top: win::scale(116),
        right: banner_width,
        bottom: win::scale(150),
    };
    DrawTextW(
        hdc,
        text.as_mut_ptr(),
        -1,
        &mut rect,
        DT_LEFT | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(hdc, state.body_font as HGDIOBJ);
    SetTextColor(hdc, 0x00906E55);
    text = win::wide(lang::t("自解压", "Self-Extract"));
    rect.top = win::scale(146);
    rect.bottom = win::scale(174);
    DrawTextW(
        hdc,
        text.as_mut_ptr(),
        -1,
        &mut rect,
        DT_LEFT | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(hdc, old_font);
    EndPaint(hwnd, &paint);
}

/// 选择目录；取消时返回 `None`。
#[cfg(windows)]
fn browse_for_folder(owner: HWND) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    };

    let mut display = [0u16; 260];
    let title = win::wide(lang::t("选择解压目录", "Choose extraction folder"));
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
    Some(PathBuf::from(win::from_wide(&path)))
}

/// 目标目录非空时的覆盖选择。
#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverwriteChoice {
    /// 覆盖同名文件。
    Overwrite,
    /// 保留同名文件，只解压不存在的条目。
    Skip,
    /// 返回设置窗口重新选择目录。
    Cancel,
}

/// 覆盖确认窗口的状态。
#[cfg(windows)]
struct OverwriteState {
    choice: OverwriteChoice,
    normal_font: HFONT,
    bold_font: HFONT,
}

/// 显示原生覆盖确认窗口；窗口创建失败时保守返回取消。
#[cfg(windows)]
pub(crate) fn confirm_overwrite(target: &Path) -> OverwriteChoice {
    use windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, CreateWindowExW, DispatchMessageW, GetMessageW, GetSystemMetrics,
        IsDialogMessageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SetWindowLongPtrW,
        ShowWindow, TranslateMessage, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT,
        WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    };

    const CLASS_NAME: &str = "LumarkSfxOverwrite";
    const ID_OVERWRITE: usize = 301;
    const ID_SKIP: usize = 302;
    const ID_CANCEL: usize = 2;
    const DM_SETDEFID: u32 = 0x0401;
    static REGISTER: std::sync::Once = std::sync::Once::new();

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, PostQuitMessage,
            WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY,
        };
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverwriteState;
        match message {
            WM_COMMAND => {
                if state_ptr.is_null() {
                    return 0;
                }
                let state = &mut *state_ptr;
                match wparam & 0xffff {
                    ID_OVERWRITE => {
                        state.choice = OverwriteChoice::Overwrite;
                        DestroyWindow(hwnd);
                    }
                    ID_SKIP => {
                        state.choice = OverwriteChoice::Skip;
                        DestroyWindow(hwnd);
                    }
                    ID_CANCEL => {
                        state.choice = OverwriteChoice::Cancel;
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                if !state_ptr.is_null() {
                    (*state_ptr).choice = OverwriteChoice::Cancel;
                }
                DestroyWindow(hwnd);
                0
            }
            WM_CTLCOLORSTATIC => {
                use windows_sys::Win32::Graphics::Gdi::{
                    GetStockObject, SetBkMode, SetTextColor, TRANSPARENT, WHITE_BRUSH,
                };
                let hdc = wparam as _;
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, 0x00141414);
                GetStockObject(WHITE_BRUSH) as _
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        if hinstance.is_null() {
            return OverwriteChoice::Cancel;
        }
        let icon =
            windows_sys::Win32::UI::WindowsAndMessaging::LoadIconW(hinstance, win::icon_resource());
        REGISTER.call_once(|| {
            let class_name = win::wide(CLASS_NAME);
            let cursor = windows_sys::Win32::UI::WindowsAndMessaging::LoadCursorW(
                std::ptr::null_mut(),
                windows_sys::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            );
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: icon,
                hCursor: cursor,
                hbrBackground: (COLOR_WINDOW + 1) as _,
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&class);
        });
        let class_name = win::wide(CLASS_NAME);
        let title = win::wide(lang::t("确认文件替换", "Confirm file replacement"));
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let ex_style = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
        let client_width = win::scale(560);
        let client_height = win::scale(240);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: client_width,
            bottom: client_height,
        };
        AdjustWindowRectEx(&mut rect, style, 0, ex_style);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let x = ((GetSystemMetrics(SM_CXSCREEN) - width) / 2).max(0);
        let y = ((GetSystemMetrics(SM_CYSCREEN) - height) / 2).max(0);
        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return OverwriteChoice::Cancel;
        }
        let mut state = Box::new(OverwriteState {
            choice: OverwriteChoice::Cancel,
            normal_font: win::make_font(9, 400),
            bold_font: win::make_font(12, 600),
        });
        SetWindowLongPtrW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            &mut *state as *mut OverwriteState as win::LongPtr,
        );
        let normal = state.normal_font;
        let bold = state.bold_font;
        let make = |text: &str,
                    class: &str,
                    style: i32,
                    x: i32,
                    y: i32,
                    width: i32,
                    height: i32,
                    id: usize,
                    font: HFONT|
         -> HWND {
            let class = win::wide(class);
            let text = win::wide(text);
            let control = CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | style as u32,
                x,
                y,
                width,
                height,
                hwnd,
                id as _,
                hinstance,
                std::ptr::null(),
            );
            win::set_font(control, font);
            control
        };
        make(
            lang::t(
                "目标文件夹已有文件",
                "The target folder already contains files",
            ),
            "STATIC",
            0,
            win::scale(24),
            win::scale(20),
            win::scale(512),
            win::scale(28),
            0,
            bold,
        );
        make(
            &target.display().to_string(),
            "STATIC",
            0x0000_4000, // SS_ENDELLIPSIS，路径过长时显示省略号
            win::scale(24),
            win::scale(54),
            win::scale(512),
            win::scale(24),
            0,
            normal,
        );
        make(
            lang::t(
                "选择“覆盖全部”会替换同名文件；选择“跳过全部”只解压不存在的条目。",
                "Overwrite all replaces existing files; Skip all extracts only new entries.",
            ),
            "STATIC",
            0,
            win::scale(24),
            win::scale(90),
            win::scale(512),
            win::scale(48),
            0,
            normal,
        );
        let button_y = client_height - win::scale(52);
        make(
            lang::t("覆盖全部", "Overwrite all"),
            "BUTTON",
            windows_sys::Win32::UI::WindowsAndMessaging::BS_DEFPUSHBUTTON,
            client_width - win::scale(334),
            button_y,
            win::scale(100),
            win::scale(32),
            ID_OVERWRITE,
            normal,
        );
        make(
            lang::t("跳过全部", "Skip all"),
            "BUTTON",
            0,
            client_width - win::scale(224),
            button_y,
            win::scale(100),
            win::scale(32),
            ID_SKIP,
            normal,
        );
        make(
            lang::t("取消", "Cancel"),
            "BUTTON",
            0,
            client_width - win::scale(114),
            button_y,
            win::scale(90),
            win::scale(32),
            ID_CANCEL,
            normal,
        );
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            DM_SETDEFID,
            ID_OVERWRITE,
            0,
        );
        ShowWindow(hwnd, SW_SHOW);
        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let choice = state.choice;
        win::delete_font(state.normal_font);
        win::delete_font(state.bold_font);
        choice
    }
}
