//! Lumark 的 GPUI Kit 展示层。

rust_i18n::i18n!("locales", fallback = "en");

pub mod app;
mod i18n;

pub use app::{LumarkApp, run};
