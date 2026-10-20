//! Versioned, non-secret application settings.

pub mod model;
pub mod store;

pub use model::{CompressionPreset, OverwritePolicy, Settings, ThemePreference};
pub use store::SettingsStore;
