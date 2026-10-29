//! Microsoft Cabinet (`.cab`) archive reader.
//!
//! GeeZipX exposes CAB as a read-only archive format backed by the [`cab`]
//! crate. The current MVP supports listing, extraction, and integrity
//! verification for single-volume cabinet files; writing, multi-volume cabinet
//! sets, encryption, and password flows remain out of scope.
//!
//! # Design notes
//!
//! - **Read-only** 鈥?GeeZipX does not expose CAB creation in the product; the
//!   upstream writer API is only used in unit tests to generate portable
//!   fixtures.
//! - **Magic + extension detection** 鈥?CAB archives are detected via the `MSCF`
//!   signature and `.cab` extension.
//! - **Path-based** 鈥?The reader stores the cabinet path and re-opens the file
//!   on each operation because the upstream [`cab::Cabinet`] consumes a
//!   `Read + Seek` source.
//! - **Windows path normalization** 鈥?Stored file names may use backslashes.
//!   GeeZipX normalizes them to forward slashes for listing and extraction so
//!   shared Zip Slip protection catches `..\\evil.txt`, `C:\\foo`, and UNC
//!   paths consistently across platforms.

use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use crate::archive::{ArchiveReader, ArchiveWriter, Entry};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// Read-only Microsoft Cabinet reader.
pub struct CabReader {
    path: PathBuf,
    format: ArchiveFormat,
}

impl fmt::Debug for CabReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CabReader")
            .field("format", &self.format)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl CabReader {
    /// Create a new CAB reader for the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            format: ArchiveFormat::Cab,
        }
    }

    fn open_cabinet(&self) -> GeeZipResult<cab::Cabinet<File>> {
        let file = File::open(&self.path)
            .map_err(|e| GeeZipError::io(e, format!("opening '{}'", self.path.display())))?;
        cab::Cabinet::new(file).map_err(convert_cab_error)
    }

    fn collect_entries(&self) -> GeeZipResult<Vec<Entry>> {
        let cabinet = self.open_cabinet()?;
        let mut entries = Vec::new();
        for folder in cabinet.folder_entries() {
            for file in folder.file_entries() {
                entries.push(cab_file_to_entry(file)?);
            }
        }
        Ok(entries)
    }

    fn find_original_name(cabinet: &cab::Cabinet<File>, normalized_path: &str) -> Option<String> {
        for folder in cabinet.folder_entries() {
            for file in folder.file_entries() {
                if normalize_cab_entry_path(file.name()) == normalized_path {
                    return Some(file.name().to_owned());
                }
            }
        }
        None
    }
}

impl ArchiveReader for CabReader {
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

        let mut cabinet = self.open_cabinet()?;
        let original_name = Self::find_original_name(&cabinet, &entry.path).ok_or_else(|| {
            GeeZipError::EntryNotFound {
                name: entry.path.clone(),
            }
        })?;
        let mut reader = cabinet
            .read_file(&original_name)
            .map_err(|e| convert_cab_extract_error(e, &entry.path))?;
        std::io::copy(&mut reader, writer)
            .map_err(|e| GeeZipError::io(e, format!("writing CAB entry '{}'", entry.path)))
    }
}

fn cab_file_to_entry(file: &cab::FileEntry) -> GeeZipResult<Entry> {
    let path = normalize_cab_entry_path(file.name());
    if path.is_empty() {
        return Err(GeeZipError::format(
            "CAB entry is missing a pathname",
            ArchiveFormat::Cab,
        ));
    }

    let modified = file.datetime().map(|dt| {
        crate::archive::datetime_to_timestamp(
            dt.year() as u64,
            dt.month() as u64,
            dt.day() as u64,
            dt.hour() as u64,
            dt.minute() as u64,
            dt.second() as u64,
        )
    });

    Ok(Entry {
        path,
        size: file.uncompressed_size() as u64,
        compressed_size: 0,
        crc32: None,
        modified,
        is_dir: false,
    })
}

fn normalize_cab_entry_path(name: &str) -> String {
    name.replace('\\', "/")
}

fn convert_cab_error(err: std::io::Error) -> GeeZipError {
    match err.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
            GeeZipError::format(format!("invalid CAB archive: {err}"), ArchiveFormat::Cab)
        }
        _ => GeeZipError::io(err, "reading CAB archive"),
    }
}

fn convert_cab_extract_error(err: std::io::Error, entry_name: &str) -> GeeZipError {
    match err.kind() {
        std::io::ErrorKind::NotFound => GeeZipError::EntryNotFound {
            name: entry_name.to_owned(),
        },
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => GeeZipError::format(
            format!("failed to extract CAB entry '{entry_name}': {err}"),
            ArchiveFormat::Cab,
        ),
        _ => GeeZipError::io(err, format!("extracting CAB entry '{entry_name}'")),
    }
}

// ---------------------------------------------------------------------------
// CabWriter
// ---------------------------------------------------------------------------

/// CAB archive writer.
///
/// Creates `.cab` (Microsoft Cabinet) archives using the `cab` crate.
/// Supports MSZIP (Deflate) compression.
///
/// Because the `cab` crate requires all files to be registered before
/// writing begins, `add_entry_from_reader` buffers all file data in
/// memory.  This is acceptable for CAB files which are typically small
/// (installers, driver packages).
pub struct CabWriter<W: Write + Seek + Send> {
    writer: Option<W>,
    files: Vec<(String, Vec<u8>)>,
    dirs: Vec<String>,
    format: ArchiveFormat,
}

impl<W: Write + Seek + Send> fmt::Debug for CabWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CabWriter")
            .field("format", &self.format)
            .field("file_count", &self.files.len())
            .field("dir_count", &self.dirs.len())
            .finish_non_exhaustive()
    }
}

impl<W: Write + Seek + Send> CabWriter<W> {
    /// Create a new CAB writer targeting the given output.
    pub fn new(writer: W) -> Self {
        CabWriter {
            writer: Some(writer),
            files: Vec::new(),
            dirs: Vec::new(),
            format: ArchiveFormat::Cab,
        }
    }
}

impl<W: Write + Seek + Send> ArchiveWriter for CabWriter<W> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn add_entry_from_reader(&mut self, path: &Path, reader: &mut dyn Read) -> GeeZipResult<()> {
        let name = path
            .to_str()
            .ok_or_else(|| GeeZipError::Format {
                message: format!("non-UTF-8 path: {}", path.display()),
                format: ArchiveFormat::Cab,
            })?
            .to_owned();
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(|e| GeeZipError::io(e, format!("reading data for CAB entry '{name}'")))?;
        self.files.push((name, data));
        Ok(())
    }

    fn add_directory(&mut self, path: &Path) -> GeeZipResult<()> {
        let name = path
            .to_str()
            .ok_or_else(|| GeeZipError::Format {
                message: format!("non-UTF-8 path: {}", path.display()),
                format: ArchiveFormat::Cab,
            })?
            .to_owned();
        self.dirs.push(name);
        Ok(())
    }

    fn finish(self: Box<Self>) -> GeeZipResult<u64> {
        let writer = self.writer.ok_or_else(|| GeeZipError::Format {
            message: "CAB writer not initialised (already consumed)".into(),
            format: ArchiveFormat::Cab,
        })?;

        // Empty archives: return 0 bytes (cab crate panics on empty folders).
        if self.files.is_empty() {
            return Ok(0);
        }
        let mut builder = cab::CabinetBuilder::new();

        // Use MSZIP if there are files; None for dirs-only archives.
        let compression = if !self.files.is_empty() {
            cab::CompressionType::MsZip
        } else {
            cab::CompressionType::None
        };

        {
            let folder = builder.add_folder(compression);
            for (path, _) in &self.files {
                folder.add_file(path);
            }
        }

        let mut cab_writer = builder.build(writer).map_err(|e| GeeZipError::Format {
            message: format!("building CAB: {e}"),
            format: ArchiveFormat::Cab,
        })?;

        // Write file data in registration order.
        let mut idx = 0;
        while let Some(mut file_writer) =
            cab_writer.next_file().map_err(|e| GeeZipError::Format {
                message: format!("writing CAB file: {e}"),
                format: ArchiveFormat::Cab,
            })?
        {
            if idx >= self.files.len() {
                return Err(GeeZipError::Format {
                    message: "CAB writer returned more file slots than registered".into(),
                    format: ArchiveFormat::Cab,
                });
            }
            file_writer
                .write_all(&self.files[idx].1)
                .map_err(|e| GeeZipError::io(e, "writing CAB file data"))?;
            idx += 1;
        }

        let mut writer = cab_writer.finish().map_err(|e| GeeZipError::Format {
            message: format!("finalising CAB: {e}"),
            format: ArchiveFormat::Cab,
        })?;
        let bytes = writer
            .stream_position()
            .map_err(|e| GeeZipError::io(e, "determining CAB size"))?;

        Ok(bytes)
    }
}
