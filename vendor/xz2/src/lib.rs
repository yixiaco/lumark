//! `xz2` API 兼容层，实际编解码由纯 Rust `lzma-rust2` 提供。
//!
//! Lumark 只使用 `geezipx-core` 依赖到的 XZ/LZMA 子集；这里提供同名类型和
//! 构造器，避免引入第二套 native `lzma` 链接。

pub mod read;
pub mod stream;
pub mod write;

use std::io;

/// 兼容层错误。
#[derive(Debug)]
pub struct Error(String);

impl Error {
    /// 构造错误。
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<Error> for io::Error {
    fn from(error: Error) -> Self {
        io::Error::other(error.0)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

/// 兼容旧 API 的结果别名。
pub type Result<T> = std::result::Result<T, Error>;
