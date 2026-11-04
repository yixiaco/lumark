//! 原生解压进度窗口：后台线程解压，窗口定时读取进度并支持取消/转入后台。

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use lumark_sfx::{
    SfxError, SfxExtractOptions, SfxFooter, SfxProgress, extract_payload_with_options,
};
use zeroize::Zeroizing;

use crate::win::{self, format_bytes, format_duration, wide};

/// 进度窗口的窗口类名。
#[cfg(windows)]
const CLASS_NAME: &str = "LumarkSfxProgress";
/// 定时刷新进度的计时器 ID。
#[cfg(windows)]
const TIMER_ID: usize = 1;
/// 后台按钮 ID。
#[cfg(windows)]
const ID_BACKGROUND: usize = 201;
/// 取消按钮沿用标准 IDCANCEL，Esc 和标题栏关闭都走同一路径。
#[cfg(windows)]
const ID_CANCEL: usize = 2;

/// 后台解压上报的一次进度快照。
#[cfg(windows)]
struct Snapshot {
    current: String,
    bytes_done: u64,
    bytes_total: u64,
    entries_done: usize,
    entries_total: usize,
    elapsed: f64,
    speed: f64,
    eta: Option<f64>,
}

/// 后台线程发给界面的消息。
#[cfg(windows)]
enum WorkerEvent {
    Progress(Snapshot),
    Done(usize),
    Failed(SfxError),
}

/// 进度窗口的控件和状态。
#[cfg(windows)]
struct ProgressState {
    rx: Receiver<WorkerEvent>,
    cancel: Arc<AtomicBool>,
    result: Option<Result<usize, SfxError>>,
    normal_font: windows_sys::Win32::Graphics::Gdi::HFONT,
    bold_font: windows_sys::Win32::Graphics::Gdi::HFONT,
    progress: windows_sys::Win32::Foundation::HWND,
    elapsed: windows_sys::Win32::Foundation::HWND,
    remaining: windows_sys::Win32::Foundation::HWND,
    files: windows_sys::Win32::Foundation::HWND,
    total: windows_sys::Win32::Foundation::HWND,
    speed: windows_sys::Win32::Foundation::HWND,
    processed: windows_sys::Win32::Foundation::HWND,
    current: windows_sys::Win32::Foundation::HWND,
    status: windows_sys::Win32::Foundation::HWND,
    cancel_button: windows_sys::Win32::Foundation::HWND,
    cancelling: bool,
}

/// 在进度窗口中解压 EXE 载荷；窗口创建失败时退回无进度同步解压。
pub(crate) fn extract_with_progress(
    exe: &Path,
    footer: SfxFooter,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
) -> Result<usize, SfxError> {
    #[cfg(windows)]
    {
        windows_extract_with_progress(exe, footer, target, password, overwrite)
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, footer, target, password, overwrite);
        Err(SfxError::Other("当前平台不支持自解压图形界面".to_string()))
    }
}

/// Windows 实现：创建窗口、启动后台线程并跑消息循环。
#[cfg(windows)]
fn windows_extract_with_progress(
    exe: &Path,
    footer: SfxFooter,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
) -> Result<usize, SfxError> {
    use windows_sys::Win32::UI::Controls::{
        ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx,
    };

    // SAFETY: 公共控件初始化没有额外前置条件，失败时进度条会退化为空控件。
    unsafe {
        let init = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS,
        };
        InitCommonControlsEx(&init);
    }

    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let state = Box::new(ProgressState {
        rx,
        cancel: cancel.clone(),
        result: None,
        normal_font: std::ptr::null_mut(),
        bold_font: std::ptr::null_mut(),
        progress: std::ptr::null_mut(),
        elapsed: std::ptr::null_mut(),
        remaining: std::ptr::null_mut(),
        files: std::ptr::null_mut(),
        total: std::ptr::null_mut(),
        speed: std::ptr::null_mut(),
        processed: std::ptr::null_mut(),
        current: std::ptr::null_mut(),
        status: std::ptr::null_mut(),
        cancel_button: std::ptr::null_mut(),
        cancelling: false,
    });

    let exe = exe.to_path_buf();
    let target = target.to_path_buf();
    let password = password.map(|value| Zeroizing::new(value.to_string()));
    let state_ptr = Box::into_raw(state);
    let hwnd = unsafe { create_window(state_ptr) };
    if hwnd.is_null() {
        // 窗口创建失败时把状态重新收回，直接同步解压，保证功能不丢。
        let state = unsafe { Box::from_raw(state_ptr) };
        drop(state);
        return extract_sync(
            &exe,
            footer,
            &target,
            password.as_ref().map(|value| value.as_str()),
            overwrite,
        );
    }

    let worker_tx = tx;
    let worker_cancel = cancel.clone();
    std::thread::spawn(move || {
        run_worker(
            &exe,
            footer,
            &target,
            password.as_ref().map(|value| value.as_str()),
            overwrite,
            worker_cancel,
            worker_tx,
        );
    });

    let mut message = unsafe { std::mem::zeroed() };
    while unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetMessageW(
            &mut message,
            std::ptr::null_mut(),
            0,
            0,
        )
    } > 0
    {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(&message);
            windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&message);
        }
    }

    let state = unsafe { Box::from_raw(state_ptr) };
    win::delete_font(state.normal_font);
    win::delete_font(state.bold_font);
    state
        .result
        .unwrap_or_else(|| Err(SfxError::Other("解压未完成".to_string())))
}

/// 无进度窗口时的同步解压兜底。
#[cfg(windows)]
fn extract_sync(
    exe: &Path,
    footer: SfxFooter,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
) -> Result<usize, SfxError> {
    let file =
        File::open(exe).map_err(|error| SfxError::Other(format!("无法打开自身文件：{error}")))?;
    extract_payload_with_options(
        file,
        footer,
        target,
        SfxExtractOptions {
            password,
            overwrite,
            cancel: None,
            progress: None,
        },
    )
}

/// 后台线程执行解压并把节流后的进度快照发回 UI。
#[cfg(windows)]
fn run_worker(
    exe: &Path,
    footer: SfxFooter,
    target: &Path,
    password: Option<&str>,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<WorkerEvent>,
) {
    let file = match File::open(exe) {
        Ok(file) => file,
        Err(error) => {
            let _ = tx.send(WorkerEvent::Failed(SfxError::Other(format!(
                "无法打开自身文件：{error}"
            ))));
            return;
        }
    };
    let start = Instant::now();
    let mut last_emit = start;
    let mut last_time = start;
    let mut last_bytes = 0u64;
    let mut smoothed_speed = 0.0f64;
    let progress_tx = tx.clone();
    let cancel_for_callback = cancel.clone();
    let mut callback = |snapshot: SfxProgress| {
        if cancel_for_callback.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        if now.duration_since(last_emit) < Duration::from_millis(80)
            && snapshot.bytes_done < snapshot.bytes_total
        {
            return;
        }
        let delta_time = now.duration_since(last_time).as_secs_f64();
        if delta_time > 0.0 {
            let instant_speed = snapshot.bytes_done.saturating_sub(last_bytes) as f64 / delta_time;
            smoothed_speed = if smoothed_speed <= 0.0 {
                instant_speed
            } else {
                smoothed_speed * 0.72 + instant_speed * 0.28
            };
            last_time = now;
            last_bytes = snapshot.bytes_done;
        }
        let eta = if smoothed_speed > 1.0 && snapshot.bytes_total > snapshot.bytes_done {
            Some((snapshot.bytes_total - snapshot.bytes_done) as f64 / smoothed_speed)
        } else {
            None
        };
        let _ = progress_tx.send(WorkerEvent::Progress(Snapshot {
            current: snapshot.current,
            bytes_done: snapshot.bytes_done,
            bytes_total: snapshot.bytes_total,
            entries_done: snapshot.entries_done,
            entries_total: snapshot.entries_total,
            elapsed: start.elapsed().as_secs_f64(),
            speed: smoothed_speed,
            eta,
        }));
        last_emit = now;
    };
    let result = extract_payload_with_options(
        file,
        footer,
        target,
        SfxExtractOptions {
            password,
            overwrite,
            cancel: Some(&cancel),
            progress: Some(&mut callback),
        },
    );
    drop(callback);
    let event = match result {
        Ok(files) => WorkerEvent::Done(files),
        Err(error) => WorkerEvent::Failed(error),
    };
    let _ = tx.send(event);
}

/// 创建进度窗口和全部控件。
#[cfg(windows)]
unsafe fn create_window(state_ptr: *mut ProgressState) -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, CreateWindowExW, GetSystemMetrics, GetWindowLongPtrW, HMENU,
        RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SetWindowLongPtrW, ShowWindow,
        WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_MINIMIZEBOX, WS_OVERLAPPED,
        WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    };

    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = wide(CLASS_NAME);
        let cursor = windows_sys::Win32::UI::WindowsAndMessaging::LoadCursorW(
            std::ptr::null_mut(),
            windows_sys::Win32::UI::WindowsAndMessaging::IDC_ARROW,
        );
        let icon =
            windows_sys::Win32::UI::WindowsAndMessaging::LoadIconW(hinstance, win::icon_resource());
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

    let hinstance = GetModuleHandleW(std::ptr::null());
    if hinstance.is_null() {
        return std::ptr::null_mut();
    }
    let class_name = wide(CLASS_NAME);
    let title = wide(crate::lang::t("Lumark 解压中", "Lumark Extracting"));
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let ex_style = WS_EX_DLGMODALFRAME;
    let client_width = win::scale(640);
    let client_height = win::scale(360);
    let mut rect = windows_sys::Win32::Foundation::RECT {
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
        return hwnd;
    }
    SetWindowLongPtrW(
        hwnd,
        windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        state_ptr as win::LongPtr,
    );

    let state = &mut *state_ptr;
    state.normal_font = win::make_font(9, 400);
    state.bold_font = win::make_font(9, 600);
    let default_font = GetStockObject(DEFAULT_GUI_FONT) as _;
    let text_font = if state.normal_font.is_null() {
        default_font
    } else {
        state.normal_font
    };
    let make = |text: &str,
                class: &str,
                style: i32,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                id: usize|
     -> windows_sys::Win32::Foundation::HWND {
        let class = wide(class);
        let text = wide(text);
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
            id as HMENU,
            hinstance,
            std::ptr::null(),
        );
        win::set_font(control, text_font);
        control
    };
    let label = |text: &str, x: i32, y: i32| {
        make(text, "STATIC", 0, x, y, win::scale(100), win::scale(20), 0)
    };
    let make_value =
        |x: i32, y: i32, width: i32| make("--", "STATIC", 0, x, y, width, win::scale(20), 0);
    let left = win::scale(24);
    let right = win::scale(340);
    let value_left = win::scale(110);
    let value_right = win::scale(430);
    label(
        crate::lang::t("已用时间：", "Elapsed:"),
        left,
        win::scale(24),
    );
    label(
        crate::lang::t("剩余时间：", "Remaining:"),
        left,
        win::scale(52),
    );
    label(
        crate::lang::t("已解压文件数：", "Files:"),
        left,
        win::scale(80),
    );
    label(
        crate::lang::t("总长度：", "Total size:"),
        right,
        win::scale(24),
    );
    label(crate::lang::t("速率：", "Speed:"), right, win::scale(52));
    label(
        crate::lang::t("已处理：", "Processed:"),
        right,
        win::scale(80),
    );
    state.elapsed = make_value(value_left, win::scale(24), win::scale(170));
    state.remaining = make_value(value_left, win::scale(52), win::scale(170));
    state.files = make_value(value_left, win::scale(80), win::scale(170));
    state.total = make_value(value_right, win::scale(24), win::scale(170));
    state.speed = make_value(value_right, win::scale(52), win::scale(170));
    state.processed = make_value(value_right, win::scale(80), win::scale(170));
    let bold = if state.bold_font.is_null() {
        text_font
    } else {
        state.bold_font
    };
    for control in [
        state.elapsed,
        state.remaining,
        state.files,
        state.total,
        state.speed,
        state.processed,
    ] {
        win::set_font(control, bold);
    }
    state.progress = make(
        "",
        "msctls_progress32",
        0x0000_0001, // PBS_SMOOTH，避免默认分段进度条
        left,
        win::scale(116),
        win::scale(592),
        win::scale(20),
        0,
    );
    state.current = make(
        "",
        "STATIC",
        0,
        left,
        win::scale(146),
        win::scale(592),
        win::scale(20),
        0,
    );
    state.status = make(
        crate::lang::t("正在解压…", "Extracting..."),
        "STATIC",
        0,
        left,
        win::scale(176),
        win::scale(592),
        win::scale(24),
        0,
    );
    // 底部按钮放在客户区底部，避免标题栏/边框吃掉按钮高度。
    let bottom = client_height - win::scale(48);
    make(
        crate::lang::t("后台(B)", "Background (B)"),
        "BUTTON",
        0,
        client_width - win::scale(236),
        bottom,
        win::scale(100),
        win::scale(30),
        ID_BACKGROUND,
    );
    state.cancel_button = make(
        crate::lang::t("取消", "Cancel"),
        "BUTTON",
        0,
        client_width - win::scale(124),
        bottom,
        win::scale(100),
        win::scale(30),
        ID_CANCEL,
    );
    // 标题栏使用自解压模块自带的 Lumark 图标。
    let icon =
        windows_sys::Win32::UI::WindowsAndMessaging::LoadIconW(hinstance, win::icon_resource());
    if !icon.is_null() {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::WM_SETICON,
            0,
            icon as _,
        );
    }
    let _ = GetWindowLongPtrW(hwnd, 0);
    ShowWindow(hwnd, SW_SHOW);
    windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(hwnd, TIMER_ID, 80, None);
    hwnd
}

/// 进度窗口消息处理。
#[cfg(windows)]
unsafe extern "system" fn window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, IDYES, KillTimer, MB_ICONQUESTION,
        MB_YESNO, MessageBoxW, PostQuitMessage, SW_MINIMIZE, ShowWindow, WM_CLOSE, WM_COMMAND,
        WM_CTLCOLORSTATIC, WM_DESTROY, WM_TIMER,
    };

    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ProgressState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let state = &mut *state_ptr;
    match message {
        WM_COMMAND => {
            let id = wparam & 0xffff;
            if id == ID_CANCEL {
                request_cancel(hwnd, state);
            } else if id == ID_BACKGROUND {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
            0
        }
        WM_CLOSE => {
            // 运行中关窗等同取消：先确认，避免静默丢弃后台解压线程。
            if state.result.is_none() && !state.cancelling {
                let title = wide(crate::lang::t("Lumark 解压中", "Lumark Extracting"));
                let text = wide(crate::lang::t(
                    "正在解压，确定要取消吗？",
                    "Extraction is running. Cancel it?",
                ));
                let answer = MessageBoxW(
                    hwnd,
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_YESNO | MB_ICONQUESTION,
                );
                if answer == IDYES {
                    request_cancel(hwnd, state);
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER_ID => {
            drain_events(hwnd, state);
            0
        }
        WM_CTLCOLORSTATIC => {
            use windows_sys::Win32::Graphics::Gdi::{SetBkMode, SetTextColor, TRANSPARENT};
            let hdc = wparam as _;
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00121212);
            windows_sys::Win32::Graphics::Gdi::GetStockObject(
                windows_sys::Win32::Graphics::Gdi::WHITE_BRUSH,
            ) as _
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_ID);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// 请求取消，禁用按钮并更新状态。
#[cfg(windows)]
unsafe fn request_cancel(hwnd: windows_sys::Win32::Foundation::HWND, state: &mut ProgressState) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW;
    if state.cancelling {
        return;
    }
    state.cancelling = true;
    state.cancel.store(true, Ordering::SeqCst);
    EnableWindow(state.cancel_button, 0);
    let text = wide(crate::lang::t("正在取消…", "Cancelling..."));
    SetWindowTextW(state.status, text.as_ptr());
    let _ = hwnd;
}

/// 从通道读取事件并更新控件；收到终止事件后关闭窗口。
#[cfg(windows)]
unsafe fn drain_events(hwnd: windows_sys::Win32::Foundation::HWND, state: &mut ProgressState) {
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

    loop {
        match state.rx.try_recv() {
            Ok(WorkerEvent::Progress(snapshot)) => update_view(state, snapshot),
            Ok(WorkerEvent::Done(files)) => {
                state.result = Some(Ok(files));
                DestroyWindow(hwnd);
                return;
            }
            Ok(WorkerEvent::Failed(error)) => {
                state.result = Some(Err(error));
                DestroyWindow(hwnd);
                return;
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                if state.result.is_none() {
                    state.result = Some(Err(SfxError::Other("解压线程已结束".to_string())));
                }
                DestroyWindow(hwnd);
                return;
            }
        }
    }
}

/// 把一份进度快照写入标签和进度条。
#[cfg(windows)]
unsafe fn update_view(state: &mut ProgressState, snapshot: Snapshot) {
    use windows_sys::Win32::UI::Controls::{PBM_SETPOS, PBM_SETRANGE32};
    let remaining = snapshot
        .eta
        .map(format_duration)
        .unwrap_or_else(|| "--:--:--".to_string());
    win::set_text(state.elapsed, &format_duration(snapshot.elapsed));
    win::set_text(state.remaining, &remaining);
    let files_text = if snapshot.entries_total > 0 {
        format!("{} / {}", snapshot.entries_done, snapshot.entries_total)
    } else {
        snapshot.entries_done.to_string()
    };
    win::set_text(state.files, &files_text);
    win::set_text(
        state.total,
        &if snapshot.bytes_total > 0 {
            format_bytes(snapshot.bytes_total)
        } else {
            "--".to_string()
        },
    );
    win::set_text(
        state.speed,
        &if snapshot.speed > 0.0 {
            format!("{}/s", format_bytes(snapshot.speed as u64))
        } else {
            "--".to_string()
        },
    );
    let percent = (snapshot.bytes_done.saturating_mul(100))
        .checked_div(snapshot.bytes_total)
        .unwrap_or(0)
        .min(100);
    let processed = if snapshot.bytes_total > 0 {
        format!(
            "{} / {} ({}%)",
            format_bytes(snapshot.bytes_done),
            format_bytes(snapshot.bytes_total),
            percent
        )
    } else {
        format_bytes(snapshot.bytes_done)
    };
    win::set_text(state.processed, &processed);
    if !snapshot.current.is_empty() {
        win::set_text(
            state.current,
            &format!(
                "{} {}",
                crate::lang::t("正在解压：", "Extracting:"),
                snapshot.current
            ),
        );
    }
    windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
        state.progress,
        PBM_SETRANGE32,
        0,
        1000,
    );
    windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
        state.progress,
        PBM_SETPOS,
        (percent * 10) as usize,
        0,
    );
    if !state.cancelling {
        win::set_text(state.status, crate::lang::t("正在解压…", "Extracting..."));
    }
}
