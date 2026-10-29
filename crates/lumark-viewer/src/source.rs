//! 查看器来源字节和内容类型判定。

use std::path::Path;
use std::sync::Arc;

use memmap2::Mmap;
use thiserror::Error;

/// 来源文件无法读取或映射。
#[derive(Debug, Error)]
pub enum SourceError {
    /// 文件系统错误。
    #[error("无法读取文件：{0}")]
    Io(#[from] std::io::Error),
}

/// 查看器持有的来源字节。
pub enum SourceBytes {
    /// 小文件或空文件。
    Owned(Vec<u8>),
    /// 通过只读内存映射持有的大文件。
    Mapped(Arc<Mmap>),
}

impl SourceBytes {
    /// 返回完整字节切片。
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Mapped(mmap) => mmap,
        }
    }

    /// 来源字节数。
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// 来源是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for SourceBytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceBytes")
            .field("len", &self.len())
            .finish()
    }
}

/// 文本、二进制或 PE 内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// 文本内容。
    Text,
    /// 非 PE 的二进制内容。
    Binary,
    /// PE32/PE32+ 可执行文件。
    Pe,
}

/// 打开来源文件；大于 1 MiB 的文件优先使用只读 mmap。
pub fn open_source(path: &Path) -> Result<SourceBytes, SourceError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() == 0 {
        return Ok(SourceBytes::Owned(Vec::new()));
    }
    let file = std::fs::File::open(path)?;
    if metadata.len() >= 1024 * 1024
        && let Ok(mmap) = unsafe { Mmap::map(&file) }
    {
        return Ok(SourceBytes::Mapped(Arc::new(mmap)));
    }
    Ok(SourceBytes::Owned(std::fs::read(path)?))
}

/// 检查 MZ/PE 签名。
pub fn is_pe(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let offset = u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
    offset
        .checked_add(4)
        .is_some_and(|end| end <= bytes.len() && &bytes[offset..end] == b"PE\0\0")
}

/// 只按内容判定文本、二进制或 PE。
///
/// 这是查看器的核心约定：文件大小不参与视图类型判定；大文本仍然是文本，
/// 只是渲染层会改用虚拟列表。`path` 只用于无 BOM 时的编码候选扩展名，
/// 不读取文件。
pub fn classify_content(bytes: &[u8], path: &Path) -> ContentKind {
    if bytes.is_empty() {
        return ContentKind::Text;
    }
    if is_pe(bytes) {
        return ContentKind::Pe;
    }
    let probe = &bytes[..bytes.len().min(8192)];
    if probe.starts_with(&[0xff, 0xfe]) || probe.starts_with(&[0xfe, 0xff]) {
        return ContentKind::Text;
    }
    if probe.starts_with(&[0xef, 0xbb, 0xbf]) {
        return ContentKind::Text;
    }
    if probe.contains(&0) {
        return ContentKind::Binary;
    }
    if std::str::from_utf8(probe).is_ok() {
        return ContentKind::Text;
    }
    // 非 UTF-8 时尝试 GB18030；中文旧文件常见，且解码失败会留下替换字符。
    let (_decoded, _, had_errors) = encoding_rs::GB18030.decode(probe);
    if !had_errors {
        return ContentKind::Text;
    }
    let control = probe
        .iter()
        .filter(|byte| **byte < 0x09 || (**byte > 0x0d && **byte < 0x20))
        .count();
    if control * 20 > probe.len()
        || path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("dll")
        })
    {
        ContentKind::Binary
    } else {
        ContentKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_text() {
        assert_eq!(classify_content(&[], Path::new("empty")), ContentKind::Text);
    }

    #[test]
    fn nul_is_binary() {
        assert_eq!(
            classify_content(b"hello\0world", Path::new("bin")),
            ContentKind::Binary
        );
    }

    #[test]
    fn utf8_is_text() {
        assert_eq!(
            classify_content("你好，Lumark".as_bytes(), Path::new("a.txt")),
            ContentKind::Text
        );
    }

    #[test]
    fn pe_magic_is_pe() {
        let mut bytes = vec![0u8; 0x100];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        assert_eq!(
            classify_content(&bytes, Path::new("a.exe")),
            ContentKind::Pe
        );
    }
}
