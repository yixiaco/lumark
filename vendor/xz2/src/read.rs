//! 解码器兼容层。

use std::io::{self, Read};

use lzma_rust2::{LzmaReader, XzReader};

use crate::stream::Stream;

/// 兼容 `xz2::read::XzDecoder` 的解码器。
pub struct XzDecoder<R: Read> {
    inner: DecoderInner<R>,
}

enum DecoderInner<R: Read> {
    Xz(XzReader<R>),
    Lzma(LzmaReader<R>),
    Failed(String),
}

impl<R: Read> XzDecoder<R> {
    /// 读取单个或多个 XZ 流。
    pub fn new(reader: R) -> Self {
        Self {
            inner: DecoderInner::Xz(XzReader::new(reader, true)),
        }
    }

    /// 读取多个串联 XZ 流。
    pub fn new_multi_decoder(reader: R) -> Self {
        Self::new(reader)
    }

    /// 按 `Stream` 选择 XZ 或 LZMA 解码。
    pub fn new_stream(mut reader: R, stream: Stream) -> Self {
        use crate::stream::StreamKind;
        let inner = match stream.kind {
            StreamKind::LzmaDecoder => {
                let mut header = [0u8; 13];
                if reader.read_exact(&mut header).is_err() {
                    return Self {
                        inner: DecoderInner::Xz(XzReader::new(reader, true)),
                    };
                }
                let props = header[0];
                let dict_size = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
                let size = u64::from_le_bytes([
                    header[5], header[6], header[7], header[8],
                    header[9], header[10], header[11], header[12],
                ]);
                match LzmaReader::new_with_props(reader, size, props, dict_size, None) {
                    Ok(decoder) => DecoderInner::Lzma(decoder),
                    Err(error) => DecoderInner::Failed(error.to_string()),
                }
            }
            _ => DecoderInner::Xz(XzReader::new(reader, true)),
        };
        Self { inner }
    }
}

impl<R: Read> Read for XzDecoder<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            DecoderInner::Xz(decoder) => decoder.read(buffer),
            DecoderInner::Lzma(decoder) => decoder.read(buffer),
            DecoderInner::Failed(message) => Err(io::Error::other(message.clone())),
        }
    }
}
