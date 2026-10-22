//! 归档格式适配器和安全解压基础能力。

pub mod edit;
pub mod error;
pub mod format;
pub mod path;
pub mod service;

pub use edit::{EditOptions, EditProgress, EditReport, EditRoot};
pub use error::ArchiveError;
pub use format::{
    ArchiveEntry, ArchiveFormat, ArchiveInfo, ArchiveTimeSource, CompressionOptions,
    CompressionPreset, CreateReport, EncryptionInfo, ExtractReport, ExtractionBudget,
    ExtractionOptions, FileTimeFilter, OverwritePolicy, PathMode, SevenZCompression, UpdateMode,
    ZipCompression, ZipEncryption,
};
pub use path::{ArchivePath, ensure_no_symlink_components, safe_join};
pub use service::{ArchiveService, CreateProgress, ExtractProgress, SFX_FOOTER_LEN, SFX_MAGIC};
