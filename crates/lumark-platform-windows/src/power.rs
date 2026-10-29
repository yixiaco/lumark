//! 压缩完成后的系统关机计划。
//!
//! 通过系统自带的 `shutdown.exe` 安排延迟关机：系统会显示倒计时通知，
//! 用户可以执行 `shutdown /a` 取消。相比直接调用关机 API，这种方式不需要
//! 应用自己获取关机权限，也不会在权限不足时静默失败。

use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use std::cell::Cell;

/// 任务运行期间阻止系统进入睡眠的 RAII 守卫。
///
/// 只在当前线程生效；同一线程的多个任务用引用计数合并，最后一个守卫
/// 释放时恢复系统默认行为。守卫失败时保持 inactive，不阻塞任务本身。
#[must_use = "守卫在 Drop 时恢复系统电源状态，应立即绑定到任务生命周期"]
pub struct SleepGuard {
    _private: (),
}

#[cfg(windows)]
thread_local! {
    /// 当前线程持有的睡眠守卫数量。
    static SLEEP_GUARD_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// 当前线程是否成功向系统注册了阻止睡眠请求。
    static SLEEP_GUARD_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// 请求当前线程运行期间阻止系统睡眠。
///
/// 使用 `ES_CONTINUOUS | ES_SYSTEM_REQUIRED`，不请求显示器常亮；显示器仍可
/// 按系统策略关闭。系统拒绝请求时返回 inactive 守卫，任务继续执行。
#[cfg(windows)]
pub fn prevent_system_sleep() -> SleepGuard {
    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };

    let depth = SLEEP_GUARD_DEPTH.with(Cell::get);
    if depth == 0 {
        // SAFETY: SetThreadExecutionState 没有指针参数，只影响当前线程。
        let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        SLEEP_GUARD_ACTIVE.with(|active| active.set(previous != 0));
    }
    SLEEP_GUARD_DEPTH.with(|counter| counter.set(depth.saturating_add(1)));
    SleepGuard { _private: () }
}

/// 非 Windows 平台没有需要阻止的系统睡眠，保持调用方代码一致。
#[cfg(not(windows))]
pub fn prevent_system_sleep() -> SleepGuard {
    SleepGuard { _private: () }
}

#[cfg(windows)]
impl Drop for SleepGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Power::{ES_CONTINUOUS, SetThreadExecutionState};

        let depth = SLEEP_GUARD_DEPTH.with(Cell::get);
        let next = depth.saturating_sub(1);
        SLEEP_GUARD_DEPTH.with(|counter| counter.set(next));
        if next == 0 && SLEEP_GUARD_ACTIVE.with(Cell::get) {
            // SAFETY: 与 prevent_system_sleep 相同，只清除当前线程的持续请求。
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
            SLEEP_GUARD_ACTIVE.with(|active| active.set(false));
        }
    }
}

#[cfg(not(windows))]
impl Drop for SleepGuard {
    fn drop(&mut self) {}
}

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

    /// 睡眠守卫按线程引用计数，嵌套释放不会提前恢复系统状态。
    #[cfg(windows)]
    #[test]
    fn sleep_guard_nests_and_releases() {
        assert_eq!(SLEEP_GUARD_DEPTH.with(Cell::get), 0);
        {
            let _first = prevent_system_sleep();
            assert_eq!(SLEEP_GUARD_DEPTH.with(Cell::get), 1);
            {
                let _second = prevent_system_sleep();
                assert_eq!(SLEEP_GUARD_DEPTH.with(Cell::get), 2);
            }
            assert_eq!(SLEEP_GUARD_DEPTH.with(Cell::get), 1);
        }
        assert_eq!(SLEEP_GUARD_DEPTH.with(Cell::get), 0);
        assert!(!SLEEP_GUARD_ACTIVE.with(Cell::get));
    }
}
