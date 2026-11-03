//! 自解压模块入口：解压自身携带的数据，并在完成或失败时给出提示。
//!
//! 交互界面使用原生 Win32 控件并参考 360 压缩的自解压窗口布局。GPUI/
//! DirectX 自解压 stub 会被 360 的 QVM 启发式判为木马，而原生 stub 已由
//! 用户实测不触发该误报；同时窗口按客户区高度布局，避免高 DPI 下按钮被裁掉。

#![windows_subsystem = "windows"]
// Win32 回调本身必须声明为 unsafe extern；内部 FFI 调用集中在这个文件，
// 统一由 Windows API 的安全约定约束。
#![allow(unsafe_op_in_unsafe_fn)]

use std::fs::File;
use std::path::Path;

use lumark_sfx::{SfxError, SfxFooter, extract_payload_with_password, read_footer};
use zeroize::Zeroizing;

mod lang;
#[cfg(windows)]
mod progress;
#[cfg(windows)]
mod setup;
#[cfg(windows)]
mod win;

/// 运行自解压流程；失败时用消息框提示用户。
fn main() {
    #[cfg(windows)]
    win::enable_dpi_awareness();
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

    #[cfg(windows)]
    {
        let mut error: Option<String> = None;
        loop {
            let Some(result) = setup::show(
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
                error = Some(
                    lang::t("请选择解压目录", "Please choose an extraction folder").to_string(),
                );
                continue;
            }
            if target.exists() && !overwrite && !directory_is_empty(&target) {
                match setup::confirm_overwrite(&target) {
                    setup::OverwriteChoice::Overwrite => overwrite = true,
                    setup::OverwriteChoice::Skip => {}
                    setup::OverwriteChoice::Cancel => {
                        error = Some(
                            lang::t(
                                "已取消覆盖确认，请选择其他目录或重新解压",
                                "Overwrite was cancelled; choose another folder or extract again",
                            )
                            .to_string(),
                        );
                        continue;
                    }
                }
            }

            match progress::extract_with_progress(
                &exe,
                footer,
                &target,
                password.as_ref().map(|value| value.as_str()),
                overwrite,
            ) {
                Ok(_) => {
                    show_message(
                        &format!(
                            "{}\n{}",
                            lang::t("已解压到：", "Extracted to:"),
                            target.display()
                        ),
                        false,
                    );
                    reveal(&target);
                    return Ok(());
                }
                Err(SfxError::WrongPassword) => {
                    error = Some(
                        lang::t("密码错误，请重新输入", "Wrong password; please try again")
                            .to_string(),
                    );
                    password = None;
                }
                Err(SfxError::Cancelled) => return Ok(()),
                Err(error_value) => {
                    error = Some(localized_error(&error_value));
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (target, password, overwrite, footer);
        Err(lang::t(
            "自解压图形界面仅支持 Windows",
            "The self-extractor UI is only available on Windows",
        )
        .to_string())
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
        SfxError::Cancelled => text("已取消解压", "Extraction cancelled").to_string(),
        SfxError::Other(message) => message.clone(),
    }
}

/// 重新打开自身文件并解压；静默模式不创建任何界面。
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

    let text = crate::win::wide(message);
    let title = crate::win::wide(lang::t("Lumark 自解压包", "Lumark Self-Extractor"));
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
