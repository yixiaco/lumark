//! Unreal Engine IoStore（`.utoc` + `.ucas`）只读解析。
//!
//! 与 PAK 不同，IoStore 以压缩块为单位存储包数据，并通过 TOC 目录索引
//! 把包路径映射到块。本模块只读，不提供打包、写入或签名验证。

mod container_header;
mod reader;
mod toc;

pub use container_header::{IoContainerHeader, IoContainerPackage};
pub use reader::{IoStoreContainer, IoStoreFile};
pub(crate) use reader::{list, probe_encryption, verify_password};
