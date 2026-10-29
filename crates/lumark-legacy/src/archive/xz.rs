//! XZ/LZMA 单流辅助函数，仅供 DEB 写入器复用。
//!
//! Lumark 的正常归档读写仍由 `lumark-archive` 的 `lzma-rust2` 实现负责；
//! 这里保留最小兼容层，避免把上游完整 XZ 模块带入本地适配层。

use std::io::{Read, Write};

use crate::error::GeeZipResult;

/// 使用 XZ 压缩数据流。
pub fn xz_compress<R: Read, W: Write>(reader: &mut R, writer: W) -> GeeZipResult<u64> {
    let mut encoder = xz2::write::XzEncoder::new(writer, 6);
    let bytes = std::io::copy(reader, &mut encoder)?;
    encoder.finish()?;
    Ok(bytes)
}

/// 解压 XZ 数据流。
pub fn xz_decompress<R: Read, W: Write>(reader: R, writer: &mut W) -> GeeZipResult<u64> {
    let mut decoder = xz2::read::XzDecoder::new_multi_decoder(reader);
    let bytes = std::io::copy(&mut decoder, writer)?;
    Ok(bytes)
}

/// 使用 LZMA 压缩数据流。
pub fn lzma_compress<R: Read, W: Write>(reader: &mut R, writer: W) -> GeeZipResult<u64> {
    let mut encoder = xz2::write::XzEncoder::new(writer, 6);
    let bytes = std::io::copy(reader, &mut encoder)?;
    encoder.finish()?;
    Ok(bytes)
}

/// 解压 LZMA/XZ 数据流。
pub fn lzma_decompress<R: Read, W: Write>(reader: R, writer: &mut W) -> GeeZipResult<u64> {
    let mut decoder = xz2::read::XzDecoder::new_multi_decoder(reader);
    let bytes = std::io::copy(&mut decoder, writer)?;
    Ok(bytes)
}
