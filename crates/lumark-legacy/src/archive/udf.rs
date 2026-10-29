//! UDF (Universal Disk Format) filesystem reader and writer (`.udf`).
//!
//! Reading uses `hadris_udf::UdfFs` for directory listing, with manual file
//! extraction because `hadris-udf` does not expose a public file-reading API.
//!
//! Writing produces UDF 2.01 images via `hadris_udf::write::UdfWriter`.
//! Entries are buffered in memory during `add_entry_from_reader` and written
//! in a single pass when `finish()` is called (non-streaming by UDF format
//! constraints).

// use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
// use std::sync::Arc;

use hadris_udf::UdfFs;
use hadris_udf::dir::UdfDirEntry;
use hadris_udf::write::{SimpleDir, SimpleFile, UdfWriteOptions, UdfWriter as HadrisUdfWriter};

use crate::archive::{ArchiveReader, ArchiveWriter, Entry, is_entry_path_dangerous};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// Maximum single file size for UDF (no practical limit).
const UDF_MAX_ENTRY_SIZE: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB per file

// ============================================================================
// Path validation
// ============================================================================

fn validate_udf_entry_path(path: &Path) -> GeeZipResult<String> {
    let raw = path.to_str().ok_or_else(|| {
        GeeZipError::format(
            format!("non-UTF-8 path: {}", path.display()),
            ArchiveFormat::Udf,
        )
    })?;

    let normalized = raw.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');

    if normalized.is_empty() || normalized == "." || normalized == ".." {
        return Err(GeeZipError::format(
            format!("invalid UDF entry path: {raw}"),
            ArchiveFormat::Udf,
        ));
    }

    if is_entry_path_dangerous(Path::new(normalized)) {
        return Err(GeeZipError::format(
            format!("invalid UDF entry path: {raw}"),
            ArchiveFormat::Udf,
        ));
    }

    Ok(normalized.to_string())
}

// ============================================================================
// Reader
// ============================================================================

/// UDF image reader.
pub struct UdfReader {
    /// Raw image data for parsing and extraction.
    data: Vec<u8>,
    /// Cached entries after parsing.
    entries_cache: Option<Vec<Entry>>,
    format: ArchiveFormat,
}

impl fmt::Debug for UdfReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdfReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl UdfReader {
    /// Create a UDF reader from a file path.
    pub fn new(path: impl AsRef<Path>) -> GeeZipResult<Self> {
        let data = fs::read(path.as_ref()).map_err(|e| {
            GeeZipError::io(
                e,
                format!("reading UDF image '{}'", path.as_ref().display()),
            )
        })?;
        Ok(Self {
            data,
            entries_cache: None,
            format: ArchiveFormat::Udf,
        })
    }

    fn ensure_parsed(&mut self) -> GeeZipResult<()> {
        if self.entries_cache.is_some() {
            return Ok(());
        }

        let cursor = Cursor::new(self.data.clone());
        let udf = UdfFs::open(cursor).map_err(|err| convert_udf_error(err, "opening UDF image"))?;

        let root = udf
            .root_dir()
            .map_err(|err| convert_udf_error(err, "reading UDF root directory"))?;

        let mut entries = Vec::new();
        collect_udf_entries(&root, "", &mut entries);
        self.entries_cache = Some(entries);
        Ok(())
    }

    /// Extract a file from the UDF image by navigating to its ICB.
    fn extract_file_internal(&self, entry_path: &str) -> GeeZipResult<Vec<u8>> {
        let mut reader = Cursor::new(self.data.clone());

        // Parse the UDF filesystem to find the file entry
        let udf = UdfFs::open(Cursor::new(self.data.clone()))
            .map_err(|err| convert_udf_error(err, "re-opening UDF image for extraction"))?;

        let root = udf
            .root_dir()
            .map_err(|err| convert_udf_error(err, "reading root dir for extraction"))?;

        // Find the directory entry for the requested path
        let dir_entry =
            find_udf_entry(&root, entry_path).ok_or_else(|| GeeZipError::EntryNotFound {
                name: entry_path.to_string(),
            })?;

        if dir_entry.is_directory {
            return Ok(Vec::new());
        }

        // Read the file entry from the ICB
        read_udf_file_data(&mut reader, &udf, dir_entry).map_err(|e| {
            GeeZipError::format(
                format!("extracting UDF file '{}': {}", entry_path, e),
                ArchiveFormat::Udf,
            )
        })
    }
}

/// Recursively collect entries from a UDF directory tree.
fn collect_udf_entries(dir: &hadris_udf::dir::UdfDir, parent: &str, out: &mut Vec<Entry>) {
    for entry in dir.entries() {
        // Skip parent and self references
        if entry.is_parent() || entry.name().is_empty() {
            continue;
        }

        let path = if parent.is_empty() {
            entry.name().to_string()
        } else {
            format!("{}/{}", parent, entry.name())
        };

        out.push(Entry {
            path: path.clone(),
            size: entry.size,
            compressed_size: entry.size,
            crc32: None,
            modified: None,
            is_dir: entry.is_directory,
        });

        // Recurse into subdirectories by re-reading from the filesystem
        // Note: This is limited because hadris-udf doesn't expose recursive
        // directory traversal. For the initial listing, we only get root entries.
        // Full recursive listing requires implementing directory reading.
    }
}

/// Find a UDF directory entry by path (simple single-level only due to API limits).
fn find_udf_entry<'a>(dir: &'a hadris_udf::dir::UdfDir, path: &str) -> Option<&'a UdfDirEntry> {
    // Handle multi-segment paths by walking directories
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }

    let filename = segments.last().unwrap();
    dir.entries().find(|e| e.name() == *filename)
}

/// Read file data from a UDF image by navigating to the file's ICB.
///
/// This manually parses the File Entry structure to find the allocation
/// descriptors and read the file data, because `hadris-udf` does not expose
/// a public file-reading API.
fn read_udf_file_data<R: Read + Seek>(
    reader: &mut R,
    udf: &UdfFs<Cursor<Vec<u8>>>,
    entry: &UdfDirEntry,
) -> GeeZipResult<Vec<u8>> {
    let info = udf.info();
    let block_size = info.block_size as u64;
    let partition_start = info.partition_start as u64;

    // Navigate to the ICB location
    let icb_block = entry.icb.logical_block_num as u64;
    let icb_sector = partition_start + icb_block;
    let icb_offset = icb_sector * block_size;

    reader
        .seek(SeekFrom::Start(icb_offset))
        .map_err(|e| GeeZipError::io(e, "seeking to UDF file ICB"))?;

    // Read the sector containing the File Entry
    let mut buffer = vec![0u8; block_size as usize];
    reader
        .read_exact(&mut buffer)
        .map_err(|e| GeeZipError::io(e, "reading UDF File Entry"))?;

    // Parse the Descriptor Tag (16 bytes)
    let tag_identifier = u16::from_le_bytes([buffer[0], buffer[1]]);
    if tag_identifier != 261 {
        // 261 = File Entry tag
        return Err(GeeZipError::format(
            format!("expected UDF File Entry tag (261), got {}", tag_identifier),
            ArchiveFormat::Udf,
        ));
    }

    // Parse the ICB Tag at offset 16
    let icb_tag_offset = 16;
    let _file_type = buffer[icb_tag_offset + 11];

    // Parse extended attributes length and allocation descriptors length
    // File Entry layout (after ICB Tag at offset 16 + 20 bytes = 36):
    // uid(4) gid(4) permissions(4) file_link_count(2) record_format(1)
    // record_display_attributes(1) record_length(4) info_length(8)
    // logical_blocks_recorded(8) access_time(12) modification_time(12)
    // attribute_time(12) checkpoint(4) extended_attribute_icb(16)
    // impl_use(32) unique_id(8) extended_attributes_length(4)
    // allocation_descriptors_length(4)
    let base_field_offset = icb_tag_offset + 20; // Start of UID
    let ea_length_offset = base_field_offset
        + 4  // uid
        + 4  // gid
        + 4  // permissions
        + 2  // file_link_count
        + 1  // record_format
        + 1  // record_display_attributes
        + 4  // record_length
        + 8  // info_length
        + 8  // logical_blocks_recorded
        + 12 // access_time
        + 12 // modification_time
        + 12 // attribute_time
        + 4  // checkpoint
        + 16 // extended_attribute_icb
        + 32 // impl_use
        + 8; // unique_id

    if ea_length_offset + 8 > buffer.len() {
        return Err(GeeZipError::format(
            "UDF File Entry too small".to_string(),
            ArchiveFormat::Udf,
        ));
    }

    let ea_length = u32::from_le_bytes([
        buffer[ea_length_offset],
        buffer[ea_length_offset + 1],
        buffer[ea_length_offset + 2],
        buffer[ea_length_offset + 3],
    ]) as usize;

    let ad_length = u32::from_le_bytes([
        buffer[ea_length_offset + 4],
        buffer[ea_length_offset + 5],
        buffer[ea_length_offset + 6],
        buffer[ea_length_offset + 7],
    ]) as usize;

    let ad_start = ea_length_offset + 8 + ea_length;
    let ad_end = ad_start + ad_length;

    if ad_start > buffer.len() || ad_end > buffer.len() {
        return Err(GeeZipError::format(
            "UDF allocation descriptors out of bounds".to_string(),
            ArchiveFormat::Udf,
        ));
    }

    let ad_data = &buffer[ad_start..ad_end];

    // Parse short allocation descriptors (8 bytes each)
    let mut file_data = Vec::new();
    for chunk in ad_data.chunks(8) {
        if chunk.len() < 8 {
            break;
        }
        let extent_length = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let extent_position = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);

        if extent_length == 0 {
            break;
        }

        let extent_sector = partition_start + extent_position as u64;
        let extent_offset = extent_sector * block_size;

        reader
            .seek(SeekFrom::Start(extent_offset))
            .map_err(|e| GeeZipError::io(e, "seeking to file extent"))?;

        let mut chunk_data = vec![0u8; extent_length as usize];
        reader
            .read_exact(&mut chunk_data)
            .map_err(|e| GeeZipError::io(e, "reading file extent"))?;

        file_data.extend_from_slice(&chunk_data);
    }

    Ok(file_data)
}

impl ArchiveReader for UdfReader {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        self.ensure_parsed()?;
        Ok(self
            .entries_cache
            .as_ref()
            .expect("entries should be cached")
            .clone())
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        self.ensure_parsed()?;

        if entry.is_dir {
            return Ok(0);
        }

        let file_data = self.extract_file_internal(&entry.path)?;
        let byte_count = file_data.len() as u64;

        writer
            .write_all(&file_data)
            .map_err(|e| GeeZipError::io(e, format!("writing UDF entry '{}'", entry.path)))?;
        writer
            .flush()
            .map_err(|e| GeeZipError::io(e, "flushing after UDF extraction"))?;

        Ok(byte_count)
    }
}

// ============================================================================
// Writer
// ============================================================================

/// UDF disc image writer.
///
/// Entries are buffered in memory during `add_entry_from_reader` calls and
/// written in a single pass when `finish()` is called.
pub struct UdfWriter<W: Write + Send> {
    inner: Option<W>,
    entries: Vec<BufferedUdfEntry>,
    format: ArchiveFormat,
}

#[derive(Clone)]
struct BufferedUdfEntry {
    path: String,
    data: Vec<u8>,
    is_dir: bool,
}

impl<W: Write + Send> fmt::Debug for UdfWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdfWriter")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Send> UdfWriter<W> {
    /// Create a new UDF writer targeting the given output.
    pub fn new(writer: W) -> Self {
        Self {
            inner: Some(writer),
            entries: Vec::new(),
            format: ArchiveFormat::Udf,
        }
    }
} // impl UdfWriter

/// Build a `SimpleDir` tree from buffered entries.
fn build_simple_dir_from_entries(entries: &[BufferedUdfEntry]) -> GeeZipResult<SimpleDir> {
    let mut root = SimpleDir::new("");

    for entry in entries {
        if entry.is_dir {
            continue;
        }

        let file = SimpleFile::new(
            Path::new(&entry.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&entry.path)
                .to_string(),
            entry.data.clone(),
        );

        if let Some(parent) = Path::new(&entry.path).parent().and_then(|p| p.to_str()) {
            if parent.is_empty() {
                root.add_file(file);
            } else {
                add_file_to_subdir(&mut root, parent, file);
            }
        } else {
            root.add_file(file);
        }
    }

    // Add empty directories
    for entry in entries {
        if !entry.is_dir {
            continue;
        }

        let dir = SimpleDir::new(
            Path::new(&entry.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&entry.path)
                .to_string(),
        );

        if let Some(parent) = Path::new(&entry.path).parent().and_then(|p| p.to_str()) {
            if parent.is_empty() {
                root.add_dir(dir);
            } else {
                add_dir_to_subdir(&mut root, parent, dir);
            }
        } else {
            root.add_dir(dir);
        }
    }

    Ok(root)
}

fn add_file_to_subdir(dir: &mut SimpleDir, path: &str, file: SimpleFile) {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        dir.add_file(file);
        return;
    }
    if segments.len() == 1 {
        dir.add_file(file);
        return;
    }
    // Find or create the subdirectory
    let first_seg = segments[0];
    let rest = segments[1..].join("/");
    for subdir in &mut dir.subdirs {
        if subdir.name == first_seg {
            add_file_to_subdir(subdir, &rest, file);
            return;
        }
    }
    // Subdirectory doesn't exist - add file to root (best effort)
    dir.add_file(file);
}

fn add_dir_to_subdir(dir: &mut SimpleDir, path: &str, sub: SimpleDir) {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() || segments.len() == 1 {
        dir.add_dir(sub);
        return;
    }
    let first_seg = segments[0];
    let rest = segments[1..].join("/");
    for existing in &mut dir.subdirs {
        if existing.name == first_seg {
            add_dir_to_subdir(existing, &rest, sub);
            return;
        }
    }
    // Create intermediate directory
    let mut intermediate = SimpleDir::new(first_seg);
    add_dir_to_subdir(&mut intermediate, &rest, sub);
    dir.add_dir(intermediate);
}

impl<W: Write + Send> ArchiveWriter for UdfWriter<W> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn add_entry_from_reader(&mut self, path: &Path, reader: &mut dyn Read) -> GeeZipResult<()> {
        let udf_path = validate_udf_entry_path(path)?;

        let mut data = Vec::with_capacity(65536);
        let mut chunk = [0u8; 65536];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| {
                GeeZipError::io(e, format!("reading data for UDF entry '{udf_path}'"))
            })?;
            if n == 0 {
                break;
            }
            if data.len() as u64 + n as u64 > UDF_MAX_ENTRY_SIZE {
                return Err(GeeZipError::format(
                    format!("UDF entry '{}' exceeds size limit", udf_path),
                    ArchiveFormat::Udf,
                ));
            }
            data.extend_from_slice(&chunk[..n]);
        }

        self.entries.push(BufferedUdfEntry {
            path: udf_path,
            data,
            is_dir: false,
        });
        Ok(())
    }

    fn add_directory(&mut self, path: &Path) -> GeeZipResult<()> {
        let udf_path = validate_udf_entry_path(path)?;
        self.entries.push(BufferedUdfEntry {
            path: udf_path,
            data: Vec::new(),
            is_dir: true,
        });
        Ok(())
    }

    fn finish(self: Box<Self>) -> GeeZipResult<u64> {
        let this = *self;
        let UdfWriter {
            inner,
            entries,
            format: _,
        } = this;

        let mut writer = inner.ok_or_else(|| {
            GeeZipError::format("UDF writer already finalised", ArchiveFormat::Udf)
        })?;

        if entries.is_empty() {
            return Err(GeeZipError::format(
                "cannot create an empty UDF image",
                ArchiveFormat::Udf,
            ));
        }

        let root = build_simple_dir_from_entries(&entries)?;

        let options = UdfWriteOptions {
            volume_id: "GEEZIPX".to_string(),
            ..Default::default()
        };

        let total_data: usize = entries.iter().map(|e| e.data.len()).sum();
        let entry_count = entries.len();
        let estimate = total_data + 270 * 2048 + entry_count * 8 * 2048;
        let mut buf = vec![0u8; estimate];
        let mut cursor = Cursor::new(&mut buf[..]);

        HadrisUdfWriter::format(&mut cursor, &root, options).map_err(|err| {
            GeeZipError::format(format!("formatting UDF image: {err}"), ArchiveFormat::Udf)
        })?;

        let written = cursor.position();
        let udf_data = &buf[..written as usize];

        writer
            .write_all(udf_data)
            .map_err(|e| GeeZipError::io(e, "writing UDF image"))?;
        writer
            .flush()
            .map_err(|e| GeeZipError::io(e, "flushing UDF image"))?;

        Ok(written)
    }
}

// ============================================================================
// Error conversion
// ============================================================================

fn convert_udf_error(err: impl std::fmt::Display, context: impl Into<String>) -> GeeZipError {
    GeeZipError::format(format!("{}: {}", context.into(), err), ArchiveFormat::Udf)
}

// ============================================================================
// Tests
// ============================================================================
