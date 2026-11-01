//! 查看器的复制行为。
//!
//! 文本视图复制正文，十六进制视图复制带偏移和 ASCII 的转储；复制内容只读，
//! 不会改动文件或归档。

use crate::app::format_size;
use crate::i18n::tr;
use gpui_kit::ClipboardItem;

use super::*;

/// 单次整体复制的最大原始字节数，避免超大文件占满剪贴板和内存。
const COPY_LIMIT_BYTES: u64 = 2 * 1024 * 1024;

impl TextViewerWindowView {
    /// 复制当前视图内容到剪贴板；PE 视图没有可复制的文本。
    pub(super) fn copy_current(&mut self, cx: &mut Context<Self>) {
        let bytes = match self.mode {
            ViewerMode::Text => self
                .document
                .as_ref()
                .map(|document| document.byte_len())
                .unwrap_or(0),
            ViewerMode::Hex => self.hex.as_ref().map(HexDocument::byte_len).unwrap_or(0),
            ViewerMode::Pe => return,
        };
        if bytes > COPY_LIMIT_BYTES {
            self.status = format!(
                "{}（{}）",
                tr(
                    "内容过大，无法整体复制",
                    "Content is too large to copy as a whole"
                ),
                format_size(COPY_LIMIT_BYTES)
            );
            cx.notify();
            return;
        }
        let text = match self.mode {
            ViewerMode::Text => self
                .editor
                .as_ref()
                .map(|editor| editor.read(cx).text().to_string())
                .or_else(|| self.document.as_ref().map(TextDocument::text))
                .unwrap_or_default(),
            ViewerMode::Hex => self.hex.as_ref().map(hex_dump).unwrap_or_default(),
            ViewerMode::Pe => return,
        };
        if text.is_empty() {
            self.status = tr("没有可复制的内容", "Nothing to copy").to_string();
        } else {
            self.status = format!(
                "{} · {}",
                tr("已复制", "Copied"),
                format_size(text.len() as u64)
            );
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.notify();
    }
}

/// 把十六进制文档转换成带偏移、字节和 ASCII 的文本转储。
fn hex_dump(hex: &HexDocument) -> String {
    let wide = hex.byte_len() > u32::MAX as u64;
    let mut output = String::new();
    for index in 0..hex.row_count() {
        let Some(row) = hex.row(index) else {
            continue;
        };
        output.push_str(&HexDocument::offset_text(row.offset, wide));
        output.push_str("  ");
        output.push_str(&row.hex);
        output.push_str("  ");
        output.push_str(&row.ascii);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 十六进制转储包含偏移、字节列和 ASCII 列，便于直接粘贴到文本工具。
    #[test]
    fn hex_dump_contains_offset_bytes_and_ascii() {
        let document = HexDocument::from_bytes(b"hello world!".to_vec());
        let dump = hex_dump(&document);
        assert_eq!(dump.lines().count(), 1);
        let line = dump.lines().next().unwrap_or_default();
        assert!(line.starts_with("00000000  68 65 6c 6c 6f"));
        assert!(line.contains("hello world!"), "dump={dump:?}");
    }
}
