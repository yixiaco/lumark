//! 自解压模块入口：静默解压，或打开 GPUI 交互窗口。
//!
//! 静默模式（`/S`）不初始化 GPUI，失败只写标准错误并返回非零退出码；
//! 交互模式在 Windows 上由 `ui` 模块提供目录、密码、覆盖确认和进度界面，
//! 无法读取页脚等启动错误也使用同一套界面基座展示。

#![windows_subsystem = "windows"]

use std::fs::File;
use std::path::PathBuf;

use lumark_sfx::{SfxError, SfxFooter, SfxPlan, extract_payload_with_password, read_footer};
use zeroize::Zeroizing;

mod lang;
#[cfg(windows)]
mod ui;

/// 交互窗口或静默解压所需的已解析启动状态。
struct Prepared {
    exe: PathBuf,
    footer: SfxFooter,
    plan: SfxPlan,
    default_target: PathBuf,
    password: Option<Zeroizing<String>>,
}

/// 运行自解压流程；静默与交互模式共用页脚和参数准备。
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if let Some(language) = lang::language_override(&arguments) {
        // SAFETY: 进程启动早期、尚未创建工作线程时设置环境变量。
        unsafe {
            std::env::set_var("LUMARK_LANG", language);
        }
    }
    let plan = lumark_sfx::parse_args(arguments.iter().skip(1).cloned());
    let silent = plan.silent;
    match prepare(plan) {
        Ok(prepared) => {
            if silent {
                if let Err(error) = run_silent(&prepared) {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            } else {
                run_interactive(prepared);
            }
        }
        Err(error) => {
            if silent {
                eprintln!("{error}");
                std::process::exit(1);
            }
            report_startup_error(error);
        }
    }
}

/// 读取自身页脚、参数计划、默认目录和初始密码。
fn prepare(plan: SfxPlan) -> Result<Prepared, String> {
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
    let default_target = lumark_sfx::default_target(&exe);
    let password = plan.password.clone().or_else(password_from_environment);
    Ok(Prepared {
        exe,
        footer,
        plan,
        default_target,
        password,
    })
}

/// 从环境变量读取密码，避免把密码暴露在命令行中。
fn password_from_environment() -> Option<Zeroizing<String>> {
    std::env::var("LUMARK_SFX_PASSWORD")
        .ok()
        .filter(|password| !password.is_empty())
        .map(Zeroizing::new)
}

/// 静默解压：不打开任何窗口，成功后直接退出。
fn run_silent(prepared: &Prepared) -> Result<(), String> {
    if prepared.footer.encrypted && prepared.password.is_none() {
        return Err(lang::t(
            "静默模式下的自解压包需要 /P=密码或 LUMARK_SFX_PASSWORD 环境变量",
            "Silent self-extraction requires /P=password or the LUMARK_SFX_PASSWORD environment variable",
        )
        .to_string());
    }
    let target = prepared
        .plan
        .target
        .clone()
        .unwrap_or_else(|| prepared.default_target.clone());
    let file = File::open(&prepared.exe).map_err(|error| {
        format!(
            "{}：{error}",
            lang::t("无法打开自身文件", "Cannot open executable")
        )
    })?;
    let password = prepared.password.as_deref().map(String::as_str);
    extract_payload_with_password(
        file,
        prepared.footer,
        &target,
        password,
        prepared.plan.overwrite,
    )
    .map_err(localized_error)?;
    Ok(())
}

/// 交互模式：把已解析状态交给 GPUI 窗口。
fn run_interactive(prepared: Prepared) {
    #[cfg(windows)]
    {
        ui::run(
            prepared.exe,
            prepared.footer,
            prepared.plan,
            prepared.default_target,
            prepared.password,
        );
    }
    #[cfg(not(windows))]
    {
        let _ = prepared;
        eprintln!(
            "{}",
            lang::t(
                "当前平台没有可用的自解压界面",
                "No self-extractor UI is available on this platform"
            )
        );
        std::process::exit(1);
    }
}

/// 启动阶段错误：交互模式用 GPUI 错误窗口，静默模式只写标准错误。
fn report_startup_error(error: String) -> ! {
    #[cfg(windows)]
    ui::run_error(error);
    #[cfg(not(windows))]
    eprintln!("{error}");
    std::process::exit(1);
}

/// 把 SFX 错误转换为当前界面语言。
fn localized_error(error: SfxError) -> String {
    match error {
        SfxError::PasswordRequired => {
            lang::t("该自解压包需要密码", "This package requires a password").to_string()
        }
        SfxError::WrongPassword => {
            lang::t("密码错误，请重新输入", "Wrong password; please try again").to_string()
        }
        SfxError::Cancelled => lang::t("已取消解压", "Extraction cancelled").to_string(),
        SfxError::Other(message) => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证静默解压错误文本跟随当前语言（CI 英语环境同样成立）。
    #[test]
    fn silent_error_follows_language() {
        let english = lang::is_english();
        assert_eq!(
            localized_error(SfxError::WrongPassword),
            if english {
                "Wrong password; please try again"
            } else {
                "密码错误，请重新输入"
            }
        );
        assert_eq!(
            localized_error(SfxError::Cancelled),
            if english {
                "Extraction cancelled"
            } else {
                "已取消解压"
            }
        );
    }
}
