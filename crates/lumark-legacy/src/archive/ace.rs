//! ACE archive reader (`.ace`).
//!
//! GeeZipX exposes ACE as a read-only archive format backed by the
//! `unarc-rs` crate.  The reader supports listing and extraction;
//! Blowfish-based encryption is supported through `unarc-rs`.
//!
//! # Design notes
//!
//! - **Read-only** 鈥?ACE creation is out of scope.
//! - **Path-based** 鈥?The reader stores the archive path and re-opens the
//!   file on each operation because [`unarc_rs::unified::UnifiedArchive`]
//!   consumes and owns its `Read + Seek` source.
//! - **No stable magic** 鈥?Detection relies on the `.ace` extension.

use std::fmt;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::PathBuf;

use unarc_rs::unified::{ArchiveEntry as UnarcEntry, ArchiveFormat as UnarcFormat, UnifiedArchive};

use crate::archive::{ArchiveReader, Entry};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// Read-only ACE archive reader.
pub struct AceReader {
    path: PathBuf,
    format: ArchiveFormat,
}

impl fmt::Debug for AceReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AceReader")
            .field("format", &self.format)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl AceReader {
    /// Create a new ACE reader for the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            format: ArchiveFormat::Ace,
        }
    }

    fn open_archive(&self) -> GeeZipResult<UnifiedArchive<BufReader<File>>> {
        let file = File::open(&self.path)
            .map_err(|e| GeeZipError::io(e, format!("opening '{}'", self.path.display())))?;
        UnifiedArchive::open_with_format(BufReader::new(file), UnarcFormat::Ace).map_err(|e| {
            GeeZipError::format(
                format!("invalid ACE archive '{}': {e}", self.path.display()),
                ArchiveFormat::Ace,
            )
        })
    }

    fn collect_entries(&self) -> GeeZipResult<Vec<Entry>> {
        let mut archive = self.open_archive()?;
        let mut entries = Vec::new();
        while let Some(unarc_entry) = archive
            .next_entry()
            .map_err(|e| GeeZipError::format(format!("ACE read error: {e}"), ArchiveFormat::Ace))?
        {
            entries.push(convert_entry(&unarc_entry));
        }
        Ok(entries)
    }
}

impl ArchiveReader for AceReader {
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
            .map_err(|e| GeeZipError::format(format!("ACE read error: {e}"), ArchiveFormat::Ace))?
        {
            if unarc_entry.name() == entry.path {
                let data = archive.read(&unarc_entry).map_err(|e| {
                    GeeZipError::format(
                        format!("failed to extract ACE entry '{}': {e}", entry.path),
                        ArchiveFormat::Ace,
                    )
                })?;
                let len = data.len() as u64;
                writer.write_all(&data).map_err(|e| {
                    GeeZipError::io(e, format!("writing ACE entry '{}'", entry.path))
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
    fn ace_malformed_fails_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("broken.ace");
        std::fs::write(&archive, b"MALFORMED_ACE_FILE").unwrap();
        let mut reader = AceReader::new(&archive);

        let err = reader.entries().unwrap_err();
        assert!(matches!(
            err,
            GeeZipError::Format {
                format: ArchiveFormat::Ace,
                ..
            }
        ));
        assert!(err.to_string().contains("invalid ACE archive"));
    }

    #[test]
    fn ace_trait_object_is_supported() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("trait.ace");
        std::fs::write(&archive, b"FAKE").unwrap();
        let reader: Box<dyn ArchiveReader> = Box::new(AceReader::new(&archive));
        assert_eq!(reader.format(), ArchiveFormat::Ace);
    }

    #[test]
    fn ace_missing_entry_is_handled() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("empty.ace");
        std::fs::write(&archive, b"FAKE").unwrap();
        let mut reader = AceReader::new(&archive);
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
