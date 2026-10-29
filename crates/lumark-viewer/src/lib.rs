//! 内置查看器的纯核心：来源字节、编码、文本索引、十六进制和 PE。
//!
//! 这个 crate 不依赖 GPUI，也不持有主题颜色；UI 层负责把这里返回的结构
//! 映射成 `cx.theme()` 中的颜色和官方组件。

pub mod encoding;
pub mod hex;
pub mod pe;
pub mod save;
pub mod source;
pub mod text;

pub use encoding::{DecodedText, EncodingKind};
pub use hex::{HEX_BYTES_PER_ROW, HexDocument, HexRow};
pub use pe::{PeError, PeImage, PeInfo, PeSection};
pub use save::{SaveError, SaveOutcome, save_text};
pub use source::{ContentKind, SourceBytes, SourceError, classify_content, open_source};
pub use text::{Eol, LineIndex, TextDocument, TextError};
