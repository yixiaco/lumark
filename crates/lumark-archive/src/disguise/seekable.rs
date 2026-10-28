//! 可 Seek 的伪装写入器，供 7Z 等先写占位头的格式使用。

use std::io::{self, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{DisguiseMask, DisguisePayloadKind, DisguiseWriter};
use crate::error::ArchiveError;

/// 在 `DisguiseWriter` 上提供最小的可 Seek 视图，供 7Z 写入器使用。
///
/// 7Z 在构造时定位到 32 字节签名头之后，完成时再回到 0 写入最终签名头。
/// 这里把前 32 字节暂存在内存，载荷位置从 32 开始；`finish` 时再回填。
pub struct SeekableDisguiseWriter<W: Write + Seek> {
    /// 真正的伪装写入器。
    inner: DisguiseWriter<W>,
    /// 当前逻辑载荷位置。
    position: u64,
    /// 7Z 签名头暂存。
    header: [u8; 32],
    /// 已暂存的签名字节数。
    header_len: usize,
}

impl<W: Write + Seek> SeekableDisguiseWriter<W> {
    /// 创建可 Seek 的伪装写入器，并预留 7Z 的 32 字节签名头。
    pub fn new(
        inner: W,
        mask: DisguiseMask,
        original_name: &str,
        kind: DisguisePayloadKind,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self, ArchiveError> {
        let mut writer = DisguiseWriter::new(inner, mask, original_name, kind, cancel)?;
        writer.write_all(&[0u8; 32])?;
        Ok(Self {
            inner: writer,
            position: 32,
            header: [0u8; 32],
            header_len: 0,
        })
    }

    /// 回填签名头并写出伪装尾部。
    pub fn finish(mut self) -> Result<W, ArchiveError> {
        if self.header_len == 32 {
            self.inner
                .patch_payload_prefix(&self.header)
                .map_err(ArchiveError::Io)?;
        }
        self.inner.finish()
    }
}

impl<W: Write + Seek> Write for SeekableDisguiseWriter<W> {
    /// 暂存签名头，其余字节直接写入载荷。
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.position < 32 {
            let take = (32 - self.position as usize).min(buffer.len());
            self.header[self.position as usize..self.position as usize + take]
                .copy_from_slice(&buffer[..take]);
            self.position += take as u64;
            self.header_len = self.header_len.max(self.position as usize);
            if take < buffer.len() {
                self.inner.write_all(&buffer[take..])?;
                self.position += (buffer.len() - take) as u64;
            }
            return Ok(buffer.len());
        }
        self.inner.write_all(buffer)?;
        self.position += buffer.len() as u64;
        Ok(buffer.len())
    }

    /// 刷新底层写入器。
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write + Seek> Seek for SeekableDisguiseWriter<W> {
    /// 仅支持 7Z 使用的 0/32 位置和当前偏移查询。
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(value) => value,
            SeekFrom::Current(delta) => self.position.saturating_add_signed(delta),
            SeekFrom::End(delta) => self.position.saturating_add_signed(delta),
        };
        self.position = target;
        Ok(self.position)
    }
}
