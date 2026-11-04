//! 文件系统和归档浏览模型。

mod asset_type;
pub mod model;
pub mod selection;
mod sidecar;

pub use asset_type::{AssetCategory, classify as classify_asset_path};
pub use model::{
    BrowserEntry, BrowserLocation, BrowserModel, GameArchiveRow, SearchMode, SearchQuery,
};
pub use selection::SelectionModel;
pub use sidecar::{UE_SIDECAR_EXTENSIONS, is_ue_sidecar, merge_ue_sidecars};
