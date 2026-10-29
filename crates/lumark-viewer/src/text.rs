//! 文本行索引和按需解码。

use std::path::Path;

use memchr::memchr_iter;
use thiserror::Error;

use crate::encoding::EncodingKind;
use crate::source::{ContentKind, SourceBytes, SourceError, classify_content, open_source};

/// 文本索引或打开失败。
#[derive(Debug, Error)]
pub enum TextError {
    /// 来源读取失败。
    #[error(transparent)]
    Source(#[from] SourceError),
    /// 内容不是文本。
    #[error("内容不是文本")]
    NotText,
    /// 文件系统错误。
    #[error("文件系统错误：{0}")]
    Io(#[from] std::io::Error),
}

/// 文本换行风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Eol {
    /// `\n`。
    #[default]
    Lf,
    /// `\r\n`。
    Crlf,
    /// `\r`。
    Cr,
}

impl Eol {
    /// 在文本中探测换行风格。
    pub fn detect(text: &str) -> Self {
        if text.contains("\r\n") {
            Self::Crlf
        } else if text.contains('\n') {
            Self::Lf
        } else if text.contains('\r') {
            Self::Cr
        } else {
            Self::Lf
        }
    }

    /// 把文本标准化成目标换行风格。
    pub fn normalize(self, text: &str) -> String {
        let unified = text.replace("\r\n", "\n").replace('\r', "\n");
        match self {
            Self::Lf => unified,
            Self::Crlf => unified.replace('\n', "\r\n"),
            Self::Cr => unified.replace('\n', "\r"),
        }
    }
}

/// 行首偏移索引。
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<usize>,
    len: usize,
    bom_len: usize,
    max_line_bytes: usize,
}

impl LineIndex {
    /// 扫描字节建立行首索引。
    pub fn build(bytes: &[u8], bom_len: usize) -> Self {
        let body = &bytes[bom_len..];
        let mut starts = Vec::with_capacity(body.len() / 32 + 1);
        starts.push(0);
        let mut max_line_bytes = 0;
        let mut line_start = 0;
        for position in memchr_iter(b'\n', body) {
            let end =
                position.saturating_sub(if position > line_start && body[position - 1] == b'\r' {
                    1
                } else {
                    0
                });
            max_line_bytes = max_line_bytes.max(end.saturating_sub(line_start));
            starts.push(position + 1);
            line_start = position + 1;
        }
        max_line_bytes = max_line_bytes.max(body.len().saturating_sub(line_start));
        Self {
            starts,
            len: body.len(),
            bom_len,
            max_line_bytes,
        }
    }

    /// 行数；空文本也有一行。
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// 指定行的字节范围（不含换行）。
    pub fn line_range(&self, index: usize) -> Option<(usize, usize)> {
        let start = *self.starts.get(index)?;
        let end = self
            .starts
            .get(index + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(self.len);
        Some((start, end.max(start)))
    }

    /// 最长行字节数。
    pub fn max_line_bytes(&self) -> usize {
        self.max_line_bytes
    }

    /// BOM 长度。
    pub fn bom_len(&self) -> usize {
        self.bom_len
    }
}

/// 打开的大文本视图。
pub struct TextDocument {
    source: SourceBytes,
    index: LineIndex,
    encoding: EncodingKind,
    eol: Eol,
    had_errors: bool,
}

impl TextDocument {
    /// 打开文件并按内容检测编码、建立行索引。
    pub fn open(path: &Path) -> Result<Self, TextError> {
        let source = open_source(path)?;
        let kind = classify_content(source.as_slice(), path);
        if kind != ContentKind::Text {
            return Err(TextError::NotText);
        }
        let encoding = EncodingKind::detect(source.as_slice());
        let bom_len = encoding.bom_len(source.as_slice());
        let index = LineIndex::build(source.as_slice(), bom_len);
        let sample = &source.as_slice()[bom_len..source.as_slice().len().min(bom_len + 8192)];
        let decoded = encoding.decode(sample);
        let eol = Eol::detect(&decoded.text);
        Ok(Self {
            source,
            index,
            encoding,
            eol,
            had_errors: decoded.had_errors,
        })
    }

    /// 从内存字节创建，主要用于测试和归档临时文件。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, TextError> {
        let source = SourceBytes::Owned(bytes);
        let kind = classify_content(source.as_slice(), Path::new(""));
        if kind != ContentKind::Text {
            return Err(TextError::NotText);
        }
        let encoding = EncodingKind::detect(source.as_slice());
        let bom_len = encoding.bom_len(source.as_slice());
        let index = LineIndex::build(source.as_slice(), bom_len);
        let sample = &source.as_slice()[bom_len..source.as_slice().len().min(bom_len + 8192)];
        let decoded = encoding.decode(sample);
        Ok(Self {
            source,
            index,
            encoding,
            eol: Eol::detect(&decoded.text),
            had_errors: decoded.had_errors,
        })
    }

    /// 行数。
    pub fn line_count(&self) -> usize {
        self.index.line_count()
    }

    /// 完整文本；大文件会一次性解码，供编辑器使用。
    pub fn text(&self) -> String {
        self.encoding.decode(self.source.as_slice()).text
    }

    /// 按行首偏移取一行文本；超过 8 KiB 的行截断。
    pub fn line(&self, index: usize) -> Option<String> {
        let (start, end) = self.index.line_range(index)?;
        let body = &self.source.as_slice()[self.index.bom_len()..];
        let line = &body[start.min(body.len())..end.min(body.len())];
        let capped = &line[..line.len().min(8192)];
        Some(
            self.encoding
                .decode_line(capped)
                .trim_end_matches('\r')
                .to_string(),
        )
    }

    /// 字节长度。
    pub fn byte_len(&self) -> u64 {
        self.source.len() as u64
    }

    /// 文本编码。
    pub fn encoding(&self) -> EncodingKind {
        self.encoding
    }

    /// 换行风格。
    pub fn eol(&self) -> Eol {
        self.eol
    }

    /// 最长行字节数。
    pub fn max_line_bytes(&self) -> usize {
        self.index.max_line_bytes()
    }

    /// 解码时是否出现过替换字符。
    pub fn had_errors(&self) -> bool {
        self.had_errors
    }

    /// 是否适合使用完整编辑器（10 MiB 且 20 万行以内）。
    pub fn editable(&self) -> bool {
        self.byte_len() <= 10 * 1024 * 1024 && self.line_count() <= 200_000
    }

    /// 来源字节只读引用。
    pub fn bytes(&self) -> &[u8] {
        self.source.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_empty_text_as_one_line() {
        let document = TextDocument::from_bytes(Vec::new()).unwrap();
        assert_eq!(document.line_count(), 1);
        assert_eq!(document.line(0).as_deref(), Some(""));
    }

    #[test]
    fn indexes_crlf_lines() {
        let document = TextDocument::from_bytes(b"one\r\ntwo\r\n".to_vec()).unwrap();
        assert_eq!(document.line_count(), 3);
        assert_eq!(document.line(0).as_deref(), Some("one"));
        assert_eq!(document.line(1).as_deref(), Some("two"));
        assert_eq!(document.line(2).as_deref(), Some(""));
        assert_eq!(document.eol(), Eol::Crlf);
    }

    #[test]
    fn gb18030_lines_decode() {
        let (bytes, _, _) = encoding_rs::GB18030.encode("第一行\n第二行");
        let document = TextDocument::from_bytes(bytes.into_owned()).unwrap();
        assert_eq!(document.line(0).as_deref(), Some("第一行"));
        assert_eq!(document.line(1).as_deref(), Some("第二行"));
    }
}
