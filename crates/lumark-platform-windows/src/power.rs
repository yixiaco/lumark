//! 压缩完成后的系统关机计划。
//!
//! 通过系统自带的 `shutdown.exe` 安排延迟关机：系统会显示倒计时通知，
//! 用户可以执行 `shutdown /a` 取消。相比直接调用关机 API，这种方式不需要
//! 应用自己获取关机权限，也不会在权限不足时静默失败。

use std::path::PathBuf;
use std::process::Command;

/// 延迟关机的倒计时秒数，给用户留出取消时间。
pub const SHUTDOWN_DELAY_SECONDS: u32 = 60;

/// 倒计时期间显示在系统通知里的说明文字。
const SHUTDOWN_MESSAGE: &str =
    "Lumark 已完成压缩任务，系统将在倒计时结束后关机。可用 shutdown /a 取消。";

/// 组装关机命令参数。
///
/// 单独暴露成函数便于测试，也避免命令行参数散落在调用处；倒计时下限为
/// 30 秒，避免调用方传入过小值导致用户来不及取消。
pub fn shutdown_arguments(delay_seconds: u32) -> Vec<String> {
    vec![
        "/s".to_string(),
        "/t".to_string(),
        delay_seconds.max(30).to_string(),
        "/d".to_string(),
        "p:0:0".to_string(),
        "/c".to_string(),
        SHUTDOWN_MESSAGE.to_string(),
    ]
}

/// 返回系统关机程序的绝对路径，避免从 `PATH` 解析到同名程序。
fn shutdown_exe() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("shutdown.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("shutdown.exe"))
}

/// 安排一次延迟关机；失败时返回可直接展示给用户的错误文本。
pub fn schedule_shutdown(delay_seconds: u32) -> Result<(), String> {
    let status = Command::new(shutdown_exe())
        .args(shutdown_arguments(delay_seconds))
        .status()
        .map_err(|error| format!("无法启动系统关机程序：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "系统关机程序返回 {}，可能没有关机权限",
            status.code().unwrap_or(-1)
        ))
    }
}

/// 取消已经安排的关机计划。
///
/// 没有待执行的关机计划时系统命令会返回非零退出码，这里统一转成文本错误。
pub fn cancel_shutdown() -> Result<(), String> {
    let status = Command::new(shutdown_exe())
        .arg("/a")
        .status()
        .map_err(|error| format!("无法启动系统关机程序：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("当前没有待取消的关机计划".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参数应包含关机、倒计时、原因和通知文字，且倒计时不小于 30 秒。
    #[test]
    fn shutdown_arguments_are_complete() {
        let arguments = shutdown_arguments(5);
        assert_eq!(arguments[0], "/s");
        assert_eq!(arguments[1], "/t");
        assert_eq!(arguments[2], "30", "倒计时应抬到安全下限");
        assert_eq!(arguments[3], "/d");
        assert_eq!(arguments[4], "p:0:0");
        assert_eq!(arguments[5], "/c");
        assert!(arguments[6].contains("Lumark"));
    }

    /// 正常倒计时应原样保留。
    #[test]
    fn shutdown_arguments_keep_larger_delay() {
        let arguments = shutdown_arguments(SHUTDOWN_DELAY_SECONDS);
        assert_eq!(arguments[2], "60");
    }
}
