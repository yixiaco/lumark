//! ISO disc image (`.iso`) archive reader and writer (ISO 9660 Level 1).
//!
//! Reading is delegated to `isomage` for detection and parsing, while extraction
//! flows through GeeZipX's shared `ArchiveReader` interface so path traversal
//! protection stays centralized in `ArchiveReader::extract_all`.
//!
//! Writing produces ISO 9660 Level 1 images via `hadris-iso`. Entries are
//! buffered in memory during `add_entry_from_reader` and written in a single
//! pass when `finish()` is called (non-streaming by ISO format constraints).
//! Joliet (Unicode filenames) and Rock Ridge (POSIX attributes) are enabled.

use std::fmt;
use std::io::{Read, Seek, SeekFrom, Write};

use isomage::TreeNode;

use crate::archive::{ArchiveReader, ArchiveWriter, CountWriter, Entry, is_entry_path_dangerous};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

use std::sync::Arc;

use hadris_iso::joliet::JolietLevel;
use hadris_iso::read::PathSeparator;
use hadris_iso::rrip::RripOptions;
use hadris_iso::write::options::{BaseIsoLevel, CreationFeatures, FormatOptions};
use hadris_iso::write::{File as HadrisFile, InputFiles, IsoImageWriter};

/// Maximum single file size for ISO 9660 Level 1 (4 GiB).
const ISO_MAX_ENTRY_SIZE: u64 = u32::MAX as u64;

fn validate_iso_entry_path(path: &std::path::Path) -> GeeZipResult<String> {
    let raw = path.to_str().ok_or_else(|| {
        GeeZipError::format(
            format!("non-UTF-8 path: {}", path.display()),
            ArchiveFormat::Iso,
        )
    })?;

    let normalized = raw.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');

    if normalized.is_empty() || normalized == "." || normalized == ".." {
        return Err(GeeZipError::format(
            format!("invalid ISO entry path: {raw}"),
            ArchiveFormat::Iso,
        ));
    }

    if is_entry_path_dangerous(std::path::Path::new(normalized)) {
        return Err(GeeZipError::format(
            format!("invalid ISO entry path: {raw}"),
            ArchiveFormat::Iso,
        ));
    }

    Ok(normalized.to_string())
}
/// Read-only ISO image reader.
pub struct IsoReader<R: Read + Seek + Send> {
    inner: R,
    root: Option<TreeNode>,
    format: ArchiveFormat,
}

impl<R: Read + Seek + Send> fmt::Debug for IsoReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IsoReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek + Send> IsoReader<R> {
    /// Create an ISO reader from any `Read + Seek + Send` source.
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            root: None,
            format: ArchiveFormat::Iso,
        }
    }

    fn ensure_parsed(&mut self) -> GeeZipResult<()> {
        if self.root.is_some() {
            return Ok(());
        }

        self.inner.seek(SeekFrom::Start(0))?;
        let root = isomage::detect_and_parse_filesystem(&mut self.inner, "archive.iso")
            .map_err(|err| convert_iso_error(err, "parsing ISO filesystem"))?;
        self.root = Some(root);
        Ok(())
    }
}

impl IsoReader<std::io::Cursor<Vec<u8>>> {
    /// Create an ISO reader from an already-loaded byte buffer.
    pub fn from_buf(buf: Vec<u8>) -> Self {
        Self::new(std::io::Cursor::new(buf))
    }
}

// ---------------------------------------------------------------------------
// IsoWriter
// ---------------------------------------------------------------------------

/// ISO 9660 disc image writer.
///
/// Entries are buffered in memory during `add_entry_from_reader` calls and
/// written in a single pass when `finish()` is called.
pub struct IsoWriter<W: std::io::Write + Send> {
    inner: Option<W>,
    entries: Vec<BufferedIsoEntry>,
    format: ArchiveFormat,
}

#[derive(Clone)]
struct BufferedIsoEntry {
    path: String,
    data: Vec<u8>,
    is_dir: bool,
}

impl<W: std::io::Write + Send> std::fmt::Debug for IsoWriter<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IsoWriter")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

/// Build a tree of `HadrisFile` entries from a flat list of buffered entries.
///
/// Missing intermediate directory nodes are automatically created so that
/// deeply nested files (e.g. `a/b/c/f.txt`) are not silently dropped when
/// only the leaf entries were registered.
fn build_hadris_files(entries: &[BufferedIsoEntry]) -> GeeZipResult<Vec<HadrisFile>> {
    use std::collections::HashSet;

    // Collect all directory paths: explicit + synthetic (from file parent chains).
    let mut dir_paths: HashSet<String> = HashSet::new();
    for entry in entries {
        if entry.is_dir {
            dir_paths.insert(entry.path.clone());
        }
        let p = std::path::Path::new(&entry.path);
        let mut current = p.parent();
        while let Some(parent) = current {
            if let Some(s) = parent.to_str() {
                if s.is_empty() {
                    break;
                }
                dir_paths.insert(s.to_string());
                current = parent.parent();
            } else {
                break;
            }
        }
    }

    // Index entries by path for fast lookup.
    let mut entry_by_path: std::collections::HashMap<&str, &BufferedIsoEntry> =
        std::collections::HashMap::new();
    for entry in entries {
        entry_by_path.insert(&entry.path, entry);
    }

    /// Recursively build children for `prefix`.  Entries whose parent is
    /// `prefix` become direct children.  Synthetic directories (paths in
    /// `dir_paths` that have `prefix` as parent but have no explicit entry)
    /// are also emitted as empty directories.
    fn build_subtree(
        entry_by_path: &std::collections::HashMap<&str, &BufferedIsoEntry>,
        dir_paths: &HashSet<String>,
        prefix: &str,
    ) -> Vec<HadrisFile> {
        let mut children: Vec<HadrisFile> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();

        // 1) Entries whose parent is `prefix` (real entries).
        for (path, entry) in entry_by_path {
            let p = std::path::Path::new(path);
            let parent = p.parent().and_then(|x| x.to_str()).unwrap_or("");
            if parent != prefix {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.is_empty() || seen.contains(name) {
                continue;
            }
            seen.insert(name);

            if entry.is_dir {
                children.push(HadrisFile::Directory {
                    name: Arc::new(name.to_string()),
                    children: build_subtree(entry_by_path, dir_paths, path),
                });
            } else {
                children.push(HadrisFile::File {
                    name: Arc::new(name.to_string()),
                    contents: entry.data.clone(),
                });
            }
        }

        // 2) Synthetic directories: paths in `dir_paths` whose parent is
        //    `prefix` but that have no explicit entry.
        for dir_path in dir_paths {
            let p = std::path::Path::new(dir_path);
            let parent = p.parent().and_then(|x| x.to_str()).unwrap_or("");
            if parent != prefix {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.is_empty() || seen.contains(name) {
                continue;
            }
            seen.insert(name);

            children.push(HadrisFile::Directory {
                name: Arc::new(name.to_string()),
                children: build_subtree(entry_by_path, dir_paths, dir_path),
            });
        }

        children
    }

    Ok(build_subtree(&entry_by_path, &dir_paths, ""))
}

impl<W: std::io::Write + Send> IsoWriter<W> {
    /// Create a new ISO writer targeting the given output.
    pub fn new(writer: W) -> Self {
        Self {
            inner: Some(writer),
            entries: Vec::new(),
            format: ArchiveFormat::Iso,
        }
    }

    /// Finalise the archive and return the inner writer alongside the bytes written.
    pub fn finalize(mut self) -> GeeZipResult<(u64, W)> {
        let mut writer = self.inner.take().ok_or_else(|| {
            GeeZipError::format("ISO writer already finalised", ArchiveFormat::Iso)
        })?;

        if self.entries.is_empty() {
            return Err(GeeZipError::format(
                "cannot create an empty ISO image",
                ArchiveFormat::Iso,
            ));
        }

        let hadris_files = build_hadris_files(&self.entries)?;

        let input_files = InputFiles {
            path_separator: PathSeparator::ForwardSlash,
            files: hadris_files,
        };

        let format_options = FormatOptions {
            volume_name: "GEEZIPX".to_string(),
            sector_size: 2048,
            system_id: None,
            volume_set_id: None,
            publisher_id: None,
            preparer_id: None,
            application_id: None,
            features: CreationFeatures {
                filenames: BaseIsoLevel::Level1 {
                    supports_lowercase: true,
                    supports_rrip: true,
                },
                long_filenames: false,
                joliet: Some(JolietLevel::Level3),
                rock_ridge: Some(RripOptions::default()),
                el_torito: None,
                hybrid_boot: None,
            },
            path_separator: PathSeparator::ForwardSlash,
            strict_charset: false,
        };

        // Write to a growable buffer so the underlying Vec can expand if the
        // estimate is too low.  Pre-allocate a generous lower bound.
        let total_data: usize = self.entries.iter().map(|e| e.data.len()).sum();
        let estimate = total_data
            .saturating_add(total_data / 8)
            .saturating_add(65536 * 16)
            .saturating_add(self.entries.len() * 512);
        // Pre-allocate with a conservative upper bound.  ISO 9660 directory
        // records are ~34-72 bytes each, plus path tables and descriptors.
        let buf = vec![0u8; estimate];
        let mut cursor = std::io::Cursor::new(buf);
        IsoImageWriter::format_new(&mut cursor, input_files, format_options).map_err(|err| {
            GeeZipError::format(format!("formatting ISO image: {err}"), ArchiveFormat::Iso)
        })?;
        let iso_data = cursor.into_inner();
        let byte_count = iso_data.len() as u64;

        writer
            .write_all(&iso_data)
            .map_err(|err| GeeZipError::io(err, "writing ISO image"))?;
        writer
            .flush()
            .map_err(|err| GeeZipError::io(err, "flushing ISO image"))?;

        Ok((byte_count, writer))
    }
}

impl<W: std::io::Write + Send> ArchiveWriter for IsoWriter<W> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn add_entry_from_reader(
        &mut self,
        path: &std::path::Path,
        reader: &mut dyn std::io::Read,
    ) -> GeeZipResult<()> {
        let iso_path = validate_iso_entry_path(path)?;

        // Pre-allocate with a 64 KiB base 鈥?most ISO entries are small enough
        // to fit, and the Vec will grow geometrically for larger files anyway.
        let mut data = Vec::with_capacity(65536);
        let mut chunk = [0u8; 65536];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| {
                GeeZipError::io(e, format!("reading data for ISO entry '{iso_path}'"))
            })?;
            if n == 0 {
                break;
            }
            if data.len() as u64 + n as u64 > ISO_MAX_ENTRY_SIZE {
                return Err(GeeZipError::format(
                    format!("ISO entry '{}' exceeds 4 GiB size limit", iso_path),
                    ArchiveFormat::Iso,
                ));
            }
            data.extend_from_slice(&chunk[..n]);
        }

        self.entries.push(BufferedIsoEntry {
            path: iso_path,
            data,
            is_dir: false,
        });
        Ok(())
    }

    fn add_directory(&mut self, path: &std::path::Path) -> GeeZipResult<()> {
        let iso_path = validate_iso_entry_path(path)?;
        self.entries.push(BufferedIsoEntry {
            path: iso_path,
            data: Vec::new(),
            is_dir: true,
        });
        Ok(())
    }

    fn finish(self: Box<Self>) -> GeeZipResult<u64> {
        let (bytes, _writer) = (*self).finalize()?;
        Ok(bytes)
    }
}

fn convert_iso_io_error(err: std::io::Error, context: impl Into<String>) -> GeeZipError {
    match err.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
            GeeZipError::format(format!("{}: {err}", context.into()), ArchiveFormat::Iso)
        }
        _ => GeeZipError::io(err, context),
    }
}

fn convert_iso_error(err: isomage::Error, context: impl Into<String>) -> GeeZipError {
    let context = context.into();
    match err.downcast::<std::io::Error>() {
        Ok(io_err) => convert_iso_io_error(*io_err, context),
        Err(err) => GeeZipError::format(format!("{context}: {err}"), ArchiveFormat::Iso),
    }
}

fn push_entries(node: &TreeNode, parent: &str, out: &mut Vec<Entry>) {
    let path = if parent.is_empty() {
        node.name.clone()
    } else if node.name.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}/{}", node.name)
    };

    if !path.is_empty() {
        out.push(Entry {
            path: path.clone(),
            size: if node.is_directory { 0 } else { node.size },
            compressed_size: if node.is_directory { 0 } else { node.size },
            crc32: None,
            modified: None,
            is_dir: node.is_directory,
        });
    }

    let next_parent = if path.is_empty() { parent } else { &path };
    for child in &node.children {
        push_entries(child, next_parent, out);
    }
}

fn find_node_by_path<'a>(node: &'a TreeNode, path: &str) -> Option<&'a TreeNode> {
    fn recurse<'a>(node: &'a TreeNode, segments: &[&str]) -> Option<&'a TreeNode> {
        match segments {
            [] => Some(node),
            [head, tail @ ..] => {
                let child = node.children.iter().find(|child| child.name == *head)?;
                recurse(child, tail)
            }
        }
    }

    let segments: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    recurse(node, &segments)
}

impl<R: Read + Seek + Send> ArchiveReader for IsoReader<R> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        self.ensure_parsed()?;
        let root = self.root.as_ref().expect("ISO tree should be cached");
        let mut entries = Vec::new();
        for child in &root.children {
            push_entries(child, "", &mut entries);
        }
        Ok(entries)
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        self.ensure_parsed()?;

        let Self { inner, root, .. } = self;
        let root = root.as_ref().expect("ISO tree should be cached");
        let node =
            find_node_by_path(root, &entry.path).ok_or_else(|| GeeZipError::EntryNotFound {
                name: entry.path.clone(),
            })?;

        if node.is_directory {
            return if entry.is_dir {
                Ok(0)
            } else {
                Err(GeeZipError::format(
                    format!("ISO entry '{}' resolves to a directory", entry.path),
                    ArchiveFormat::Iso,
                ))
            };
        }

        if entry.is_dir {
            return Err(GeeZipError::format(
                format!("ISO entry '{}' resolves to a file", entry.path),
                ArchiveFormat::Iso,
            ));
        }

        let mut counted = CountWriter {
            inner: writer,
            count: 0,
        };
        isomage::cat_node(inner, node, &mut counted).map_err(|err| {
            convert_iso_error(err, format!("extracting ISO entry '{}'", entry.path))
        })?;
        Ok(counted.count)
    }
}
