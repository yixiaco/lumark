use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use lumark_archive::{
    ArchiveError, ArchiveFormat, ArchiveService, CompressionOptions, EditOptions, EditRoot,
    ExtractionBudget, ExtractionOptions, OverwritePolicy, UpdateMode, safe_join,
};
use tempfile::tempdir;

#[path = "roundtrip/common.rs"]
mod common;
#[path = "roundtrip/legacy.rs"]
mod legacy;
#[path = "roundtrip/misc.rs"]
mod misc;
#[path = "roundtrip/seven_zip.rs"]
mod seven_zip;
#[path = "roundtrip/tar_stream.rs"]
mod tar_stream;
#[path = "roundtrip/zip.rs"]
mod zip;
