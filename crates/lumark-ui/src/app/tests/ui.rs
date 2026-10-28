//! 真实窗口 UI 测试入口，按功能拆分到子模块。

use super::super::*;
use gpui_kit::test::TestWindowExt;
use lumark_vault::{VaultCrypto, VaultSettings};
use tempfile::tempdir;

mod browser;
mod compress;
mod mail;
mod password;
mod tasks;
mod vault;
mod viewer;
