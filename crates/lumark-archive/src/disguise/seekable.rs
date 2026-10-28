//! 可 Seek 的伪装写入器，供 7Z/LZH 等需要回填头部的格式使用。

use std::io::{self, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{DisguiseMask, DisguisePayloadKind, DisguiseWriter};
use crate::error::ArchiveError;

/// 在 `DisguiseWriter` 上提供随机访问写入视图。
///
/// 顺序写入直接追加；定位到已写区域时调用 `patch_payload_at` 重新混淆并覆盖，
/// 因此 7Z 的签名头和 LZH 的条目头都可以在完整载荷写完后回填。
pub struct SeekableDisguiseWriter<W: Write + Seek> {
    /// 真正的伪装写入器。
    inner: DisguiseWriter<W>,
    /// 当前逻辑载荷位置。
    position: u64,
}

impl<W: Write + Seek> SeekableDisguiseWriter<W> {
    /// 创建可 Seek 的伪装写入器。
    pub fn new(
        inner: W,
        mask: DisguiseMask,
        original_name: &str,
        kind: DisguisePayloadKind,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self, ArchiveError> {
        Ok(Self {
            inner: DisguiseWriter::new(inner, mask, original_name, kind, cancel)?,
            position: 0,
        })
    }

    /// 写出伪装尾部并返回底层写入器。
    pub fn finish(self) -> Result<W, ArchiveError> {
        self.inner.finish()
    }
}

impl<W: Write + Seek> Write for SeekableDisguiseWriter<W> {
    /// 在当前位置追加；落在已有载荷内时按偏移回填。
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let len = self.inner.payload_len();
        if self.position > len {
            let gap = self.position - len;
            let zeros = vec![0u8; gap.min(1024 * 1024) as usize];
            let mut remaining = gap;
            while remaining > 0 {
                let take = remaining.min(zeros.len() as u64) as usize;
                self.inner.write_all(&zeros[..take])?;
                remaining -= take as u64;
            }
            self.position = len + gap;
        }
        if self.position < len {
            let take = buffer.len().min((len - self.position) as usize);
            self.inner
                .patch_payload_at(self.position, &buffer[..take])?;
            self.position += take as u64;
            if take == buffer.len() {
                return Ok(buffer.len());
            }
            self.inner.write_all(&buffer[take..])?;
            self.position += (buffer.len() - take) as u64;
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
    /// 在逻辑载荷范围内定位；向后定位会用零填充缺口。
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let len = self.inner.payload_len();
        let target = match position {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::Current(delta) => self.position as i128 + delta as i128,
            SeekFrom::End(delta) => len as i128 + delta as i128,
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "伪装载荷定位越界",
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}
