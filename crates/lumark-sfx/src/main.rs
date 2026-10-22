//! 自解压模块入口：解压自身携带的数据，并在完成或失败时给出提示。
//!
//! 作为 Windows GUI 子系统程序运行，不弹出控制台窗口；失败信息通过消息框
//! 展示，成功后用资源管理器打开解压目录。

#![windows_subsystem = "windows"]

use std::fs::File;

/// 运行自解压流程；失败时用消息框提示用户。
fn main() {
    if let Err(error) = run() {
        show_message(&error, true);
    }
}

/// 读取自身页脚并解压数据段，最后打开目标目录。
fn run() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("无法定位自身文件：{error}"))?;
    let mut file = File::open(&exe).map_err(|error| format!("无法打开自身文件：{error}"))?;
    let footer = lumark_sfx::read_footer(&mut file)
        .map_err(|error| format!("无法读取自解压页脚：{error}"))?
        .ok_or_else(|| "这不是有效的 Lumark 自解压文件".to_string())?;
    let target = lumark_sfx::default_target(&exe);
    let files = lumark_sfx::extract_payload(file, footer, &target)?;
    let _ = std::process::Command::new("explorer.exe")
        .arg(&target)
        .spawn();
    show_message(
        &format!("已解压 {files} 个文件到：\n{}", target.display()),
        false,
    );
    Ok(())
}

/// 显示消息框；`error` 为真时使用错误图标。
#[cfg(windows)]
fn show_message(message: &str, error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
    };

    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "Lumark 自解压"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
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
fn show_message(message: &str, error: bool) {
    if error {
        eprintln!("{message}");
    } else {
        println!("{message}");
    }
}
