//! 流配置兼容类型。

use crate::{Error, Result};

/// 流类型。
#[derive(Debug, Clone, Copy)]
pub(crate) enum StreamKind {
    XzDecoder,
    LzmaEncoder,
    LzmaDecoder,
}

/// 兼容 `xz2::stream::Stream` 的流描述。
#[derive(Debug, Clone, Copy)]
pub struct Stream {
    pub(crate) kind: StreamKind,
    pub(crate) options: Option<LzmaOptions>,
}

/// 兼容 `xz2::stream::LzmaOptions`。
#[derive(Debug, Clone, Copy)]
pub struct LzmaOptions {
    pub(crate) preset: u32,
}

impl LzmaOptions {
    /// 按预设等级创建 LZMA 选项。
    pub fn new_preset(preset: u32) -> Result<Self> {
        Ok(Self { preset })
    }
}

impl Stream {
    /// 创建 LZMA 编码流。
    pub fn new_lzma_encoder(options: &LzmaOptions) -> Result<Self> {
        Ok(Self {
            kind: StreamKind::LzmaEncoder,
            options: Some(*options),
        })
    }

    /// 创建 LZMA 解码流。
    pub fn new_lzma_decoder(_memlimit: u64) -> Result<Self> {
        Ok(Self {
            kind: StreamKind::LzmaDecoder,
            options: None,
        })
    }

    /// 创建 XZ 解码流。
    pub fn new_stream_decoder() -> Result<Self> {
        Ok(Self {
            kind: StreamKind::XzDecoder,
            options: None,
        })
    }
}

impl From<Stream> for Error {
    fn from(_: Stream) -> Self {
        Error::new("invalid stream")
    }
}
