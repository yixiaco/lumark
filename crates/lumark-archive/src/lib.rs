//! 归档格式适配器和安全解压基础能力。

mod alz;
mod asar;
pub mod disguise;
pub mod edit;
pub mod error;
pub mod format;
mod lzh;
mod lzh_stream;
mod msi;
mod pak;
pub mod path;
pub mod repair;
mod rpm;
pub mod service;
mod unarc;
mod wim;
mod xar;
mod zpaq;

pub use disguise::{
    DISGUISE_MAGIC, DISGUISE_TRAILER_LEN, DisguiseFileOptions, DisguiseInfo, DisguiseMask,
    DisguiseOptions, DisguisePayloadKind, DisguiseReport, MAX_DISGUISE_NAME_BYTES, RestoreReport,
    clear_materialized_cache,
};
pub use edit::{EditOptions, EditProgress, EditReport, EditRoot};
pub use error::ArchiveError;
pub use format::{
    ArchiveEntry, ArchiveFormat, ArchiveInfo, ArchiveTimeSource, CompressionOptions,
    CompressionPreset, CreateReport, EncryptionInfo, ExtractReport, ExtractionBudget,
    ExtractionOptions, FileTimeFilter, OverwritePolicy, PathMode, SevenZCompression, UpdateMode,
    ZipCompression, ZipEncryption, ZpaqCompression,
};
pub use pak::iostore::{IoStoreContainer, IoStoreFile};
pub use pak::oodle::{OodleStatus, configure_oodle_library, oodle_backend_setting, oodle_status};
pub use pak::oodle_download::{
    OodleDownloadOptions, OodleDownloadResult, download_and_cache_oodle, extract_oodle_library,
    install_oodle_zip,
};
pub use pak::unreal::{PakAssetEntry, PakAssetReader};
pub use path::{ArchivePath, ensure_no_symlink_components, safe_join};
pub use service::{
    ArchiveService, CreateProgress, ExtractProgress, SFX_FOOTER_LEN, SFX_MAGIC, SFX_MAGIC_V2,
};
pub use zpaq::{configure_zpaq_worker, run_zpaq_worker};
