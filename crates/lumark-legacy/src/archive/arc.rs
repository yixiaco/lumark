//! ARC/PAK archive reader (`.arc`, `.pak`).
//!
//! GeeZipX exposes ARC as a read-only archive format backed by the
//! `unarc-rs` crate.  The reader supports listing and extraction;
//! XOR-based encryption is handled transparently by `unarc-rs`.
//!
//! # Design notes
//!
//! - **Read-only** 鈥?ARC/PAK creation is out of scope.
//! - **Path-based** 鈥?The reader stores the archive path and re-opens the
//!   file on each operation because [`unarc_rs::unified::UnifiedArchive`]
//!   consumes and owns its `Read + Seek` source.
//! - **No stable magic** 鈥?Detection relies on the `.arc` / `.pak` extension.

use std::fmt;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::PathBuf;

use unarc_rs::unified::{ArchiveEntry as UnarcEntry, ArchiveFormat as UnarcFormat, UnifiedArchive};

use crate::archive::{ArchiveReader, Entry};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// Read-only ARC/PAK archive reader.
pub struct ArcReader {
    path: PathBuf,
    format: ArchiveFormat,
}

impl fmt::Debug for ArcReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArcReader")
            .field("format", &self.format)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl ArcReader {
    /// Create a new ARC reader for the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            format: ArchiveFormat::Arc,
        }
    }

    fn open_archive(&self) -> GeeZipResult<UnifiedArchive<BufReader<File>>> {
        let file = File::open(&self.path)
            .map_err(|e| GeeZipError::io(e, format!("opening '{}'", self.path.display())))?;
        UnifiedArchive::open_with_format(BufReader::new(file), UnarcFormat::Arc).map_err(|e| {
            GeeZipError::format(
                format!("invalid ARC archive '{}': {e}", self.path.display()),
                ArchiveFormat::Arc,
            )
        })
    }

    fn collect_entries(&self) -> GeeZipResult<Vec<Entry>> {
        let mut archive = self.open_archive()?;
        let mut entries = Vec::new();
        while let Some(unarc_entry) = archive
            .next_entry()
            .map_err(|e| GeeZipError::format(format!("ARC read error: {e}"), ArchiveFormat::Arc))?
        {
            entries.push(convert_entry(&unarc_entry));
        }
        Ok(entries)
    }
}

impl ArchiveReader for ArcReader {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        self.collect_entries()
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        if entry.is_dir {
            return Ok(0);
        }

        let mut archive = self.open_archive()?;
        while let Some(unarc_entry) = archive
            .next_entry()
            .map_err(|e| GeeZipError::format(format!("ARC read error: {e}"), ArchiveFormat::Arc))?
        {
            if unarc_entry.name() == entry.path {
                let data = archive.read(&unarc_entry).map_err(|e| {
                    GeeZipError::format(
                        format!("failed to extract ARC entry '{}': {e}", entry.path),
                        ArchiveFormat::Arc,
                    )
                })?;
                let len = data.len() as u64;
                writer.write_all(&data).map_err(|e| {
                    GeeZipError::io(e, format!("writing ARC entry '{}'", entry.path))
                })?;
                return Ok(len);
            }
            let _ = archive.skip(&unarc_entry);
        }
        Err(GeeZipError::EntryNotFound {
            name: entry.path.clone(),
        })
    }
}

fn convert_entry(unarc_entry: &UnarcEntry) -> Entry {
    let modified = unarc_entry.modified_time().map(|dt| {
        crate::archive::datetime_to_timestamp(
            dt.year() as u64,
            dt.month() as u64,
            dt.day() as u64,
            dt.hour() as u64,
            dt.minute() as u64,
            dt.second() as u64,
        )
    });
    Entry {
        path: unarc_entry.name().to_owned(),
        size: unarc_entry.original_size(),
        compressed_size: unarc_entry.compressed_size(),
        crc32: Some(unarc_entry.crc() as u32),
        modified,
        is_dir: unarc_entry.original_size() == 0 && unarc_entry.name().ends_with('/'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_malformed_fails_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("broken.arc");
        std::fs::write(&archive, b"MALFORMED_ARC_FILE").unwrap();
        let mut reader = ArcReader::new(&archive);

        let err = reader.entries().unwrap_err();
        assert!(matches!(
            err,
            GeeZipError::Format {
                format: ArchiveFormat::Arc,
                ..
            }
        ));
        // The error format may vary 鈥?just ensure it's a clean Format error.
        let msg = err.to_string();
        assert!(
            msg.contains("arc") || msg.contains("format error"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn arc_trait_object_is_supported() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("trait.arc");
        std::fs::write(&archive, b"FAKE").unwrap();
        let reader: Box<dyn ArchiveReader> = Box::new(ArcReader::new(&archive));
        assert_eq!(reader.format(), ArchiveFormat::Arc);
    }

    #[test]
    fn arc_missing_entry_is_handled() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("empty.arc");
        std::fs::write(&archive, b"FAKE").unwrap();
        let mut reader = ArcReader::new(&archive);
        let mut sink = Vec::new();

        let result = reader.extract(
            &Entry {
                path: "missing.txt".into(),
                size: 0,
                compressed_size: 0,
                crc32: None,
                modified: None,
                is_dir: false,
            },
            &mut sink,
        );
        assert!(result.is_err());
    }
}
