//! 编码器兼容层。

use std::io::{self, Write};

use lzma_rust2::{LzmaOptions as RawLzmaOptions, LzmaWriter, XzOptions, XzWriter};

use crate::stream::{LzmaOptions, Stream};

/// 兼容 `xz2::write::XzEncoder` 的编码器。
pub struct XzEncoder<W: Write> {
    inner: EncoderInner<W>,
}

#[allow(clippy::large_enum_variant)]
enum EncoderInner<W: Write> {
    Xz(XzWriter<W>),
    Lzma(LzmaWriter<W>),
}

impl<W: Write> XzEncoder<W> {
    /// 创建 XZ 编码器。
    pub fn new(writer: W, level: u32) -> Self {
        let inner = XzWriter::new(writer, XzOptions::with_preset(level))
            .expect("lzma-rust2 XZ encoder initialization");
        Self {
            inner: EncoderInner::Xz(inner),
        }
    }

    /// 创建 LZMA 编码器。
    pub fn new_stream(writer: W, stream: Stream) -> Self {
        let options = stream.options.unwrap_or(LzmaOptions { preset: 6 });
        let raw = RawLzmaOptions::with_preset(options.preset);
        let inner = LzmaWriter::new_use_header(writer, &raw, None)
            .expect("lzma-rust2 LZMA encoder initialization");
        Self {
            inner: EncoderInner::Lzma(inner),
        }
    }

    /// 完成编码并返回底层写入器。
    pub fn finish(self) -> io::Result<W> {
        match self.inner {
            EncoderInner::Xz(writer) => writer.finish().map_err(io::Error::other),
            EncoderInner::Lzma(writer) => writer.finish().map_err(io::Error::other),
        }
    }
}

impl<W: Write> Write for XzEncoder<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            EncoderInner::Xz(writer) => writer.write(buffer),
            EncoderInner::Lzma(writer) => writer.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            EncoderInner::Xz(writer) => writer.flush(),
            EncoderInner::Lzma(writer) => writer.flush(),
        }
    }
}
