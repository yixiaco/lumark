#![allow(clippy::all)]

//! 从 MIT 许可的 GeeZipX 0.7.4 裁剪出的传统格式只读适配层。
//!
//! 只保留 Lumark 实际使用的十个适配器；上游 ZIP/7Z/RAR/ZPAQ/WIM 实现和
//! 写归档功能不再编译，避免重复引入 ZIP/7z 依赖。

pub mod archive;
pub mod config;
pub mod detect;
pub mod error;
pub mod test;

pub use archive::{ArchiveReader, Entry};
pub use detect::ArchiveFormat;
pub use error::{GeeZipError, GeeZipResult};
