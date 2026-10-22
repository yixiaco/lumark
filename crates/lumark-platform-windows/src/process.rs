//! 进程优先级控制。
//!
//! 压缩是 CPU 密集型操作，长时间占满核心会让界面和其他程序卡顿。这里通过
//! 降低整个进程的调度优先级来让出 CPU，而不是做硬性百分比限速：硬限速会
//! 连界面线程一起限流，反而让暂停、取消和进度刷新变得迟钝。

use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, GetPriorityClass, NORMAL_PRIORITY_CLASS,
    SetPriorityClass,
};

/// 读取当前进程的优先级类别（Windows 常量值）。
pub fn priority_class() -> Result<u32, String> {
    // 伪句柄只代表当前进程，调用失败时返回 0。
    let class = unsafe { GetPriorityClass(GetCurrentProcess()) };
    if class == 0 {
        Err(format!(
            "无法读取进程优先级：{}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(class)
    }
}

/// 切换压缩期间的进程优先级：开启时降到普通以下，关闭时恢复普通。
///
/// 不改动实时/高优先级，避免在异常情况下把系统调度拖死；失败时返回可展示
/// 文本，由调用方决定提示方式。
pub fn set_low_priority(enabled: bool) -> Result<(), String> {
    let target = if enabled {
        BELOW_NORMAL_PRIORITY_CLASS
    } else {
        NORMAL_PRIORITY_CLASS
    };
    if unsafe { SetPriorityClass(GetCurrentProcess(), target) } == 0 {
        return Err(format!(
            "无法设置进程优先级：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 低优先级开关应能正确写入并读回，测试结束前恢复普通优先级。
    #[test]
    fn low_priority_round_trips() {
        set_low_priority(true).expect("设置低优先级");
        assert_eq!(
            priority_class().expect("读取优先级"),
            BELOW_NORMAL_PRIORITY_CLASS
        );
        set_low_priority(false).expect("恢复普通优先级");
        assert_eq!(priority_class().expect("读取优先级"), NORMAL_PRIORITY_CLASS);
    }
}
