//! PAK 家族只读读取器。
//!
//! `.pak` 扩展名对应多种互不兼容的格式，按二进制头区分后分别由
//! `quake`、`chromium` 和 `unreal` 子模块解析。

pub(crate) mod chromium;
pub mod cityhash;
pub(crate) mod codec;
pub(crate) mod iostore;
pub(crate) mod oodle;
pub mod oodle_download;
pub(crate) mod quake;
pub(crate) mod unreal;
