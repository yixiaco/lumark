//! 运行时诊断：把未捕获的 panic 记录到本地日志。
//!
//! Windows GUI 进程没有控制台，Rust 默认的 panic 输出会被系统直接丢弃；如果
//! panic 又发生在 Win32 回调里，栈展开无法跨越 FFI 边界，进程会立刻中止，用户
//! 既看不到错误提示，也留不下任何现场。这里在启动时安装 panic hook，把线程、
//! 位置、消息和回溯追加到日志文件，方便事后定位这类“窗口突然消失”的故障。

use std::backtrace::Backtrace;
use std::io::Write;
use std::path::PathBuf;

/// 日志文件名，固定放在本地应用数据的 `Lumark\logs` 目录下。
const LOG_NAME: &str = "lumark-panic.log";
/// 单个日志文件的上限；超过后滚动成 `.old`，避免长期占用磁盘。
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// 返回当前用户的日志目录，与 IPC 请求目录使用同一个基目录。
fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Lumark")
        .join("logs")
}

/// 安装 panic hook：保留默认输出，再把一条带现场信息的记录写入日志。
///
/// 诊断写入失败时必须保持静默，否则会在崩溃路径上再引发一次 panic。
pub(crate) fn install_panic_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        record(info);
    }));
}

/// 把一次 panic 追加到日志，包含线程名、源码位置、消息和回溯。
fn record(info: &std::panic::PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("<未命名线程>");
    let location = info.location().map_or_else(
        || "<未知位置>".to_string(),
        |location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        },
    );
    let message = payload_message(info.payload());
    let entry = format!(
        "[{}] 进程 {} 线程 {thread} 在 {location} 崩溃\n{message}\n回溯：\n{}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        std::process::id(),
        Backtrace::force_capture(),
    );
    let _ = append(&entry);
}

/// 把一段文本追加到日志文件，必要时先把超限的旧日志滚动成 `.old`。
fn append(entry: &str) -> std::io::Result<()> {
    let directory = log_dir();
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(LOG_NAME);
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, directory.join(format!("{LOG_NAME}.old")));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(entry.as_bytes())?;
    file.flush()
}

/// 把 panic 载荷还原成可读文本，非字符串载荷给出占位说明。
fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "<非字符串 panic 载荷>".to_string()
}

#[cfg(test)]
mod tests {
    use super::payload_message;

    /// 两种字符串载荷都要能还原成原始文本。
    #[test]
    fn payload_message_reads_string_payloads() {
        let borrowed: &(dyn std::any::Any + Send) = &"借用字符串";
        assert_eq!(payload_message(borrowed), "借用字符串");
        let owned: &(dyn std::any::Any + Send) = &String::from("拥有字符串");
        assert_eq!(payload_message(owned), "拥有字符串");
    }
}
