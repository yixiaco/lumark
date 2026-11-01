//! 内存内容的内置查看器入口。
//!
//! 提取到的 AES 密钥等短数据只以内存形式传给查看器，不写入临时文件，
//! 避免敏感密钥在磁盘上留下副本。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::px;

use crate::app::LumarkApp;
use crate::i18n::tr;

use super::*;

impl TextViewerWindowView {
    /// 用内存内容构造查看器：`text` 供文本视图，`bytes` 供十六进制视图。
    ///
    /// 两者可以是同一密钥的十六进制文本和原始 32 字节；不落盘、不提供保存，
    /// 保持只读查看的语义。
    fn from_memory(name: String, path_text: String, text: Vec<u8>, bytes: Vec<u8>) -> Self {
        let size = bytes.len() as u64;
        let document = TextDocument::from_bytes(text).ok();
        let line_sizes = Rc::new(vec![
            gpui_kit::size(px(1.), px(18.));
            document
                .as_ref()
                .map(|document| document.line_count())
                .unwrap_or(0)
        ]);
        let hex = HexDocument::from_bytes(bytes);
        let hex_sizes = Rc::new(vec![gpui_kit::size(px(1.), px(18.)); hex.row_count()]);
        Self {
            name,
            path: PathBuf::new(),
            path_text,
            size,
            saved_modified: None,
            can_save: false,
            archive_target: None,
            mode: if document.is_some() {
                ViewerMode::Text
            } else {
                ViewerMode::Hex
            },
            document,
            editor: None,
            line_sizes,
            text_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            hex_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            hex: Some(hex),
            hex_sizes,
            pe: None,
            error: None,
            status: String::new(),
        }
    }
}

impl LumarkApp {
    /// 在内置文本/十六进制查看器中打开内存内容，不写入磁盘。
    pub(crate) fn open_memory_viewer(
        &mut self,
        name: String,
        path_text: String,
        text: Vec<u8>,
        bytes: Vec<u8>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let options = super::viewer_window_options(parent);
        let viewer_name = name.clone();
        let result = gpui_kit::open_window(options, cx, move |_window, cx| {
            cx.new(|_| {
                TextViewerWindowView::from_memory(
                    viewer_name.clone(),
                    path_text.clone(),
                    text.clone(),
                    bytes.clone(),
                )
            })
        });
        match result {
            Ok(_) => {
                self.status = format!(
                    "{}：{name}",
                    tr("已在内置查看器中打开", "Opened in the viewer")
                )
            }
            Err(error) => {
                self.status = format!(
                    "{}{error}",
                    tr("无法打开查看器：", "Cannot open the viewer: ")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内存查看器应同时提供文本和十六进制视图，且默认可直接只读浏览。
    #[test]
    fn memory_viewer_exposes_text_and_hex_views() {
        let text = b"00112233445566778899AABBCCDDEEFF00112233445566778899AABBCCDDEEFF\n".to_vec();
        let bytes = (0u8..32).collect::<Vec<u8>>();
        let view = TextViewerWindowView::from_memory(
            "AES 密钥".to_string(),
            "内存内容".to_string(),
            text,
            bytes,
        );
        assert!(matches!(view.mode, ViewerMode::Text));
        assert!(view.document.is_some());
        assert!(view.hex.is_some());
        assert!(!view.can_save);
    }
}
