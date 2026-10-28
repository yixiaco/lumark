//! 命名管道单实例请求传输。
//!
//! 第一个进程启动管道服务线程，后续进程把命令行参数编码成二进制消息写入
//! 管道；服务端把请求放入进程内队列，由界面轮询消费。相比临时文件轮询，
//! 管道消息不需要在磁盘上落盘，也不依赖目录权限。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_CONNECTED, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT, WaitNamedPipeW,
};

use crate::PlatformError;

/// 固定管道名前缀。
const PIPE_PREFIX: &str = r"\\.\pipe\";
/// 默认单实例管道名。
const DEFAULT_PIPE: &str = "Lumark.SingleInstance";
/// 单条消息最大字节数。
const MAX_MESSAGE: usize = 1024 * 1024;

/// 进程内待处理请求队列。
fn queue() -> &'static Mutex<Vec<Vec<String>>> {
    static QUEUE: OnceLock<Mutex<Vec<Vec<String>>>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// 启动默认管道服务；重复调用无副作用。
pub fn start_server() {
    let _ = start_server_with(DEFAULT_PIPE, |arguments| {
        if let Ok(mut queue) = queue().lock() {
            queue.push(arguments);
        }
    });
}

/// 取出当前累积的全部管道请求。
pub fn take_requests() -> Vec<Vec<String>> {
    queue()
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

/// 向默认管道发送请求。
pub fn send_request(arguments: &[String]) -> Result<(), PlatformError> {
    send_request_to(DEFAULT_PIPE, arguments)
}

/// 启动指定名称的管道服务，并把请求交给 `sink`。
pub fn start_server_with(
    name: &str,
    sink: impl Fn(Vec<String>) + Send + 'static,
) -> Result<(), PlatformError> {
    let pipe_name = full_pipe_name(name);
    std::thread::Builder::new()
        .name("lumark-pipe-server".to_string())
        .spawn(move || {
            if let Err(error) = serve(&pipe_name, sink) {
                eprintln!("命名管道服务退出：{error}");
            }
        })
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    Ok(())
}

/// 向指定管道写入一条请求。
pub fn send_request_to(name: &str, arguments: &[String]) -> Result<(), PlatformError> {
    let pipe_name = full_pipe_name(name);
    let payload = encode(arguments)?;
    let wide = wide(&pipe_name);
    unsafe {
        for _ in 0..5 {
            if WaitNamedPipeW(wide.as_ptr(), 500) != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let handle = CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(PlatformError::Windows(std::io::Error::last_os_error()));
        }
        let result = write_all(handle, &payload);
        CloseHandle(handle);
        result
    }
}

/// 管道服务循环：建立实例、等待连接、读取消息、断开后重建。
fn serve(pipe_name: &str, sink: impl Fn(Vec<String>)) -> Result<(), PlatformError> {
    let wide = wide(pipe_name);
    loop {
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                0,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(PlatformError::Windows(std::io::Error::last_os_error()));
        }
        let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
        if connected == 0 {
            let code = unsafe { GetLastError() };
            if code != ERROR_PIPE_CONNECTED {
                unsafe { CloseHandle(handle) };
                continue;
            }
        }
        if let Ok(arguments) = read_message(handle) {
            sink(arguments);
        }
        unsafe {
            DisconnectNamedPipe(handle);
            CloseHandle(handle);
        }
    }
}

/// 读取一条长度前缀消息。
fn read_message(handle: HANDLE) -> Result<Vec<String>, PlatformError> {
    let mut count_bytes = [0u8; 4];
    read_exact(handle, &mut count_bytes)?;
    let count = u32::from_be_bytes(count_bytes) as usize;
    if count > 1024 {
        return Err(PlatformError::Windows(std::io::Error::other(
            "管道请求参数过多",
        )));
    }
    let mut arguments = Vec::with_capacity(count);
    for _ in 0..count {
        let mut length_bytes = [0u8; 4];
        read_exact(handle, &mut length_bytes)?;
        let length = u32::from_be_bytes(length_bytes) as usize;
        if length > MAX_MESSAGE {
            return Err(PlatformError::Windows(std::io::Error::other(
                "管道请求参数过大",
            )));
        }
        let mut bytes = vec![0u8; length];
        read_exact(handle, &mut bytes)?;
        arguments.push(String::from_utf8_lossy(&bytes).to_string());
    }
    Ok(arguments)
}

/// 编码参数列表：参数个数 + 每个参数的长度和 UTF-8 字节。
fn encode(arguments: &[String]) -> Result<Vec<u8>, PlatformError> {
    let mut output = Vec::new();
    output.extend_from_slice(&(arguments.len() as u32).to_be_bytes());
    for argument in arguments {
        let bytes = argument.as_bytes();
        if bytes.len() > MAX_MESSAGE {
            return Err(PlatformError::Windows(std::io::Error::other(
                "命令行参数过长",
            )));
        }
        output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        output.extend_from_slice(bytes);
    }
    Ok(output)
}

/// 在管道句柄上完整写入。
fn write_all(handle: HANDLE, mut bytes: &[u8]) -> Result<(), PlatformError> {
    while !bytes.is_empty() {
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || written == 0 {
            return Err(PlatformError::Windows(std::io::Error::last_os_error()));
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

/// 在管道句柄上完整读取。
fn read_exact(handle: HANDLE, mut bytes: &mut [u8]) -> Result<(), PlatformError> {
    while !bytes.is_empty() {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || read == 0 {
            return Err(PlatformError::Windows(std::io::Error::last_os_error()));
        }
        bytes = &mut bytes[read as usize..];
    }
    Ok(())
}

/// 拼接完整管道路径。
fn full_pipe_name(name: &str) -> String {
    format!("{PIPE_PREFIX}{name}")
}

/// 转换为 NUL 结尾宽字符串。
fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// 验证管道请求编码、服务端接收和参数保真。
    #[test]
    fn pipe_round_trips_arguments() {
        let name = format!("LumarkTest-{}", std::process::id());
        let (sender, receiver) = mpsc::channel();
        start_server_with(&name, move |arguments| {
            let _ = sender.send(arguments);
        })
        .unwrap();
        let arguments = vec!["--open".to_string(), "C:\\a b\\x.zip".to_string()];
        let mut last_error = String::new();
        for _ in 0..20 {
            match send_request_to(&name, &arguments) {
                Ok(()) => break,
                Err(error) => {
                    last_error = error.to_string();
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        let received = receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| panic!("管道未收到请求：{last_error}"));
        assert_eq!(received, arguments);
    }
}
