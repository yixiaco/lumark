//! 文本编码检测、解码和保存时的还原。

use encoding_rs::GB18030;

/// 查看器支持的文本编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingKind {
    /// UTF-8，可能带 BOM。
    Utf8,
    /// UTF-16 小端，可能带 BOM。
    Utf16Le,
    /// UTF-16 大端，可能带 BOM。
    Utf16Be,
    /// GB18030（GBK/GB2312 的超集）。
    Gb18030,
}

impl EncodingKind {
    /// 用于状态栏和编码选择的名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Gb18030 => "GB18030",
        }
    }

    /// 检测 BOM；没有 BOM 时按 UTF-8 → GB18030 的顺序探测。
    pub fn detect(bytes: &[u8]) -> Self {
        if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            return Self::Utf8;
        }
        if bytes.starts_with(&[0xff, 0xfe]) {
            return Self::Utf16Le;
        }
        if bytes.starts_with(&[0xfe, 0xff]) {
            return Self::Utf16Be;
        }
        let probe = &bytes[..bytes.len().min(8192)];
        if std::str::from_utf8(probe).is_ok() {
            return Self::Utf8;
        }
        let (_, _, had_errors) = GB18030.decode(probe);
        if !had_errors {
            return Self::Gb18030;
        }
        Self::Utf8
    }

    /// 返回编码的 BOM 字节长度。
    pub fn bom_len(self, bytes: &[u8]) -> usize {
        match self {
            Self::Utf8 if bytes.starts_with(&[0xef, 0xbb, 0xbf]) => 3,
            Self::Utf16Le if bytes.starts_with(&[0xff, 0xfe]) => 2,
            Self::Utf16Be if bytes.starts_with(&[0xfe, 0xff]) => 2,
            _ => 0,
        }
    }

    /// 解码完整文本。
    pub fn decode(self, bytes: &[u8]) -> DecodedText {
        let bom_len = self.bom_len(bytes);
        let body = &bytes[bom_len..];
        let (text, had_errors) = match self {
            Self::Utf8 => {
                let (text, had_errors) = decode_utf8(body);
                (text, had_errors)
            }
            Self::Utf16Le => decode_utf16(body, true),
            Self::Utf16Be => decode_utf16(body, false),
            Self::Gb18030 => {
                let (decoded, _, had_errors) = GB18030.decode(body);
                (decoded.into_owned(), had_errors)
            }
        };
        DecodedText {
            text,
            encoding: self,
            bom_len,
            had_errors,
        }
    }

    /// 解码一行字节，返回有损文本。
    pub fn decode_line(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Utf16Le => decode_utf16(bytes, true).0,
            Self::Utf16Be => decode_utf16(bytes, false).0,
            Self::Gb18030 => GB18030.decode(bytes).0.into_owned(),
        }
    }

    /// 按编码写出字节；`bom` 决定是否写 BOM。
    pub fn encode(self, text: &str, bom: bool) -> Vec<u8> {
        match self {
            Self::Utf8 => {
                let mut out = Vec::with_capacity(text.len() + 3);
                if bom {
                    out.extend_from_slice(&[0xef, 0xbb, 0xbf]);
                }
                out.extend_from_slice(text.as_bytes());
                out
            }
            Self::Utf16Le => {
                let mut out = Vec::with_capacity(text.len() * 2 + 2);
                if bom {
                    out.extend_from_slice(&[0xff, 0xfe]);
                }
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
                out
            }
            Self::Utf16Be => {
                let mut out = Vec::with_capacity(text.len() * 2 + 2);
                if bom {
                    out.extend_from_slice(&[0xfe, 0xff]);
                }
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
                out
            }
            Self::Gb18030 => {
                let (encoded, _, had_errors) = GB18030.encode(text);
                if had_errors {
                    // 编码失败时仍返回可写字节；保存层会在结果中报告替换字符。
                    encoded.into_owned()
                } else {
                    encoded.into_owned()
                }
            }
        }
    }
}

/// 解码结果。
#[derive(Debug, Clone)]
pub struct DecodedText {
    /// 解码后的文本。
    pub text: String,
    /// 实际使用的编码。
    pub encoding: EncodingKind,
    /// BOM 字节长度。
    pub bom_len: usize,
    /// 是否出现无法解码的字节。
    pub had_errors: bool,
}

fn decode_utf8(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), false),
        Err(_) => (String::from_utf8_lossy(bytes).into_owned(), true),
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> (String, bool) {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let (chunks, remainder) = bytes.as_chunks::<2>();
    for chunk in chunks {
        units.push(if little_endian {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], chunk[1]])
        });
    }
    let had_errors = !remainder.is_empty();
    (String::from_utf16_lossy(&units), had_errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_utf8_bom() {
        assert_eq!(
            EncodingKind::detect(b"\xef\xbb\xbfhello"),
            EncodingKind::Utf8
        );
        assert_eq!(EncodingKind::Utf8.bom_len(b"\xef\xbb\xbfhello"), 3);
    }

    #[test]
    fn utf16_roundtrip() {
        let encoded = EncodingKind::Utf16Le.encode("你好 Lumark", true);
        let decoded = EncodingKind::Utf16Le.decode(&encoded);
        assert_eq!(decoded.text, "你好 Lumark");
        assert_eq!(decoded.bom_len, 2);
    }

    #[test]
    fn gb18030_decodes_chinese() {
        let (encoded, _, _) = GB18030.encode("中文文本");
        let decoded = EncodingKind::Gb18030.decode(&encoded);
        assert_eq!(decoded.text, "中文文本");
        assert!(!decoded.had_errors);
    }
}
