//! 带版本管理、不含敏感信息的应用设置。

pub mod model;
pub mod store;

pub use model::{
    ColumnKind, ColumnWidths, CompressionPreset, DisguiseMaskPreference, OverwritePolicy, Settings,
    ThemePreference,
};
pub use store::SettingsStore;
