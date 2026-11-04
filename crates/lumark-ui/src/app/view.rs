//! 主窗口视图入口，按框架、页面、弹窗和组件拆分。

mod browser;
pub(crate) use browser::{ArchiveDragItem, DetailsPreview, path_bar_drop_handler};
pub(crate) mod asset_viewer_window;
#[allow(unused_imports)]
pub(crate) use asset_viewer_window::ViewerScene;
mod batch_export_window;
mod chrome;
mod components;
mod compress_window;
mod dialogs;
pub(crate) mod disguise_window;
pub(crate) mod export_window;
pub(crate) mod extract_window;
mod game_assets;
mod game_sidebar;
pub(crate) mod image_viewer_window;
pub(crate) mod media_player_window;
pub(crate) mod oodle_download_window;
mod pages;
pub(crate) mod password_windows;
pub(crate) mod properties_viewer_window;
mod settings_window;
mod settings_window_notify;
mod settings_window_pages;
mod settings_window_selects;
mod settings_window_tabs;
pub(crate) mod text_viewer_window;
pub(crate) mod windows;
