//! 内嵌 JIDAC 引擎调用、stdio 句柄修复和错误映射。
//!
//! `zpaq_rs` 通过临时重定向进程 stdout/stderr 来捕获命令行输出；release 版
//! 图形界面从资源管理器启动时没有控制台，CRT 的标准流可能没有有效句柄，
//! 导致 `fileno()/dup()` 失败。这里在首次调用前用 `freopen` 把无效标准流
//! 重新绑定到 NUL，避免整个 ZPAQ 后端在无控制台环境下不可用。

use zpaq_rs::ZpaqCommandOutput;

use crate::error::ArchiveError;

// UCRT 的 `stdout`/`stderr` 宏背后返回 `FILE*` 的函数。
#[cfg(all(windows, target_env = "msvc"))]
unsafe extern "C" {
    /// UCRT 标准流查询函数。
    fn __acrt_iob_func(index: u32) -> *mut libc::FILE;
}

/// 判断引擎错误是否属于密码缺失或密码错误。
fn is_password_error(message: &str) -> bool {
    message.contains("password incorrect")
}

/// 去掉引擎错误里重复的库名前缀，保留可读的原始信息。
fn error_message(error: zpaq_rs::ZpaqError) -> String {
    let message = error.to_string();
    message
        .strip_prefix("libzpaq: ")
        .unwrap_or(&message)
        .trim()
        .to_string()
}

/// 把引擎错误转换为归档层错误，并根据调用方是否提供密码区分密码状态。
pub(super) fn map_engine_error(error: zpaq_rs::ZpaqError, password_supplied: bool) -> ArchiveError {
    let message = error_message(error);
    if is_password_error(&message) {
        return if password_supplied {
            ArchiveError::WrongPassword
        } else {
            ArchiveError::PasswordRequired
        };
    }
    ArchiveError::Corrupt(format!("ZPAQ 操作失败：{message}"))
}

/// 执行一次内嵌 zpaq 命令，并在失败时给出归档层错误。
///
/// `password_supplied` 只影响错误分类：引擎把“缺少密码”和“密码错误”
/// 都报告为同一条信息，调用方需要告诉这里当前是否已经带上 `-key`。
pub(super) fn run(
    args: &[&str],
    password_supplied: bool,
) -> Result<ZpaqCommandOutput, ArchiveError> {
    ensure_jidac_stdio();
    zpaq_rs::zpaq_command(args).map_err(|error| map_engine_error(error, password_supplied))
}

/// 确保进程有可用的 CRT 标准输出和标准错误句柄。
///
/// 只在句柄无效时打开 `NUL` 并覆盖文件描述符 1/2；控制台环境下保留原
/// 句柄，调试输出不受影响。该修复只做一次。
fn ensure_jidac_stdio() {
    #[cfg(windows)]
    {
        use std::sync::Once;

        /// 标准流修复只执行一次。
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            // SAFETY: 这里只查询和重绑定 CRT 的标准流；
            // 打开 NUL 失败时保持原状，后续引擎调用会返回明确错误。
            unsafe {
                for fd in [1, 2] {
                    // GUI 进程没有控制台时 `_get_osfhandle` 可能返回 -1
                    // 或 -2；只要不是有效句柄就需要重新绑定。
                    if libc::get_osfhandle(fd) >= 0 {
                        continue;
                    }
                    // `freopen` 会同步更新 FILE* 与底层描述符；只做
                    // `_dup2` 时 `fileno(stdout)` 仍可能是无效值。
                    #[cfg(all(windows, target_env = "msvc"))]
                    {
                        let stream = __acrt_iob_func(fd as u32);
                        if !stream.is_null() {
                            let reopened = libc::freopen(c"NUL".as_ptr(), c"w".as_ptr(), stream);
                            if !reopened.is_null() && libc::get_osfhandle(fd) >= 0 {
                                continue;
                            }
                        }
                    }
                    let Ok(file) = std::fs::OpenOptions::new().write(true).open("NUL") else {
                        continue;
                    };
                    use std::os::windows::io::IntoRawHandle;

                    let handle = file.into_raw_handle();
                    let descriptor = libc::open_osfhandle(handle as isize, libc::O_WRONLY);
                    if descriptor < 0 {
                        let _ = windows_sys::Win32::Foundation::CloseHandle(handle as *mut _);
                        continue;
                    }
                    libc::dup2(descriptor, fd);
                    libc::close(descriptor);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确认缺少密码和密码错误会被分类成不同结果。
    #[test]
    fn maps_password_errors_by_password_supplied() {
        let error = || zpaq_rs::ZpaqError::Ffi("password incorrect".to_string());
        assert!(matches!(
            map_engine_error(error(), false),
            ArchiveError::PasswordRequired
        ));
        assert!(matches!(
            map_engine_error(error(), true),
            ArchiveError::WrongPassword
        ));
    }

    /// 确认普通引擎错误去掉库名前缀后仍保留原始描述。
    #[test]
    fn strips_library_prefix_from_other_errors() {
        let error = zpaq_rs::ZpaqError::Ffi("archive is empty".to_string());
        match map_engine_error(error, false) {
            ArchiveError::Corrupt(message) => {
                assert_eq!(message, "ZPAQ 操作失败：archive is empty")
            }
            other => panic!("错误类型不正确：{other:?}"),
        }
    }
}
