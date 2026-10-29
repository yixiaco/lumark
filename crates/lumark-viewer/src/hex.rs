//! 十六进制视图的行模型。

use crate::source::{SourceBytes, open_source};

/// 每行显示的字节数，与参考截图一致。
pub const HEX_BYTES_PER_ROW: usize = 16;

/// 十六进制视图的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexRow {
    /// 行首文件偏移。
    pub offset: u64,
    /// 本行实际字节数（末行可能不足 16）。
    pub bytes: Vec<u8>,
    /// 补齐到 16 列后的十六进制文本。
    pub hex: String,
    /// 每字节对应的可打印 ASCII，不可打印为 `.`。
    pub ascii: String,
}

impl HexRow {
    /// 构造一行；`bytes` 最多 16 字节。
    pub fn new(offset: u64, bytes: &[u8]) -> Self {
        let mut groups = Vec::with_capacity(HEX_BYTES_PER_ROW);
        let mut ascii = String::with_capacity(HEX_BYTES_PER_ROW);
        for index in 0..HEX_BYTES_PER_ROW {
            if let Some(byte) = bytes.get(index) {
                groups.push(format!("{byte:02x}"));
                ascii.push(if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                });
            } else {
                groups.push("  ".to_string());
                ascii.push(' ');
            }
        }
        Self {
            offset,
            bytes: bytes.to_vec(),
            hex: groups.join(" "),
            ascii,
        }
    }
}

/// 只读十六进制文档；按需生成行，不复制整个文件。
pub struct HexDocument {
    source: SourceBytes,
}

impl HexDocument {
    /// 打开文件。
    pub fn open(path: &std::path::Path) -> Result<Self, crate::source::SourceError> {
        Ok(Self {
            source: open_source(path)?,
        })
    }

    /// 从内存创建。
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            source: SourceBytes::Owned(bytes),
        }
    }

    /// 总字节数。
    pub fn byte_len(&self) -> u64 {
        self.source.len() as u64
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.source.len().div_ceil(HEX_BYTES_PER_ROW)
    }

    /// 取一行。
    pub fn row(&self, index: usize) -> Option<HexRow> {
        let start = index.checked_mul(HEX_BYTES_PER_ROW)?;
        if start >= self.source.len() {
            return None;
        }
        let end = (start + HEX_BYTES_PER_ROW).min(self.source.len());
        Some(HexRow::new(
            start as u64,
            &self.source.as_slice()[start..end],
        ))
    }

    /// 偏移按 8 位或 16 位格式显示。
    pub fn offset_text(offset: u64, wide: bool) -> String {
        if wide {
            format!("{offset:016x}")
        } else {
            format!("{offset:08x}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_sixteen_bytes() {
        let document = HexDocument::from_bytes((0..=32).collect());
        assert_eq!(document.row_count(), 3);
        let row = document.row(1).unwrap();
        assert_eq!(row.offset, 16);
        assert_eq!(row.bytes.len(), 16);
        assert!(row.hex.starts_with("10 11 12"));
    }

    #[test]
    fn short_last_row_is_padded() {
        let document = HexDocument::from_bytes(b"abc".to_vec());
        let row = document.row(0).unwrap();
        assert_eq!(row.bytes.len(), 3);
        assert_eq!(row.ascii, "abc             ");
        assert_eq!(row.hex.split_whitespace().count(), 3);
        assert_eq!(row.hex.len(), 47);
    }
}
