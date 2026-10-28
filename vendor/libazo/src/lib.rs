//! AZO 解压占位实现。
//!
//! Lumark 的 EGG 支持通过 `unegg` 读取 Store/Deflate/Bzip2/LZMA 条目；AZO
//! 是 ALZip 的私有编解码器，当前没有可用依赖，因此这里显式返回不支持，
//! 避免把缺失实现误报成数据损坏。

use std::io::{Read, Write};

/// 解密回调类型，与 `unegg` 调用约定保持一致。
pub type DecryptFn<'a> = &'a mut dyn FnMut(&mut [u8]);

/// AZO 解码错误。
#[derive(Debug)]
pub struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AZO 解压暂不支持")
    }
}

impl std::error::Error for Error {}

/// 占位解压函数；AZO 条目返回明确的不支持错误。
pub fn extract_azo<R: Read, W: Write>(
    _reader: &mut R,
    _writer: &mut W,
    _compressed_size: u64,
    _max_output: Option<u64>,
    _decrypt: Option<DecryptFn<'_>>,
) -> Result<u32, Error> {
    Err(Error)
}
