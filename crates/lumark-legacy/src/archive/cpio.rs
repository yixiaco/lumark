//! CPIO (`.cpio`) archive reader and writer.
//!
//! GeeZipX exposes CPIO support backed by the
//! [`cpio-archive`](https://crates.io/crates/cpio-archive) crate. The current
//! implementation supports `newc` and `odc` archives for listing, extraction,
//! integrity verification, and creation (writing).
//!
//! # Design notes
//!
//! - **Extension-only detection** 鈥?GeeZipX maps `.cpio` to this reader/writer
//!   but does not auto-detect CPIO from leading bytes. CPIO has per-entry
//!   header magic, not a stable file-wide magic, so shallow sniffing is prone
//!   to false positives.
//! - **Dual-format writing** 鈥?newc (SVR4) entries are built manually, while
//!   odc entries use the `cpio_archive::OdcBuilder`.
//! - **Path-based reading** 鈥?the reader stores the archive path and re-opens
//!   the file for each operation because the upstream reader is a forward-only
//!   cursor.
//! - **Filesystem safety first** 鈥?regular files and directories extract
//!   normally; symlinks, hard links, device nodes, FIFOs, sockets, and unknown
//!   special entries are reported as unsupported instead of being created on the
//!   host filesystem. Writing only supports regular files and directories.

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cpio_archive::{CpioHeader, OdcBuilder};

use crate::archive::{
    ArchiveReader, ArchiveWriter, CancellableWriter, CountWriter, Entry, ExtractReport,
    check_entry_path_safety, normalize_path,
};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};
use crate::test::TestReport;

const S_IFMT: u32 = 0o170000;
const S_IFIFO: u32 = 0o010000;
const S_IFCHR: u32 = 0o020000;
const S_IFDIR: u32 = 0o040000;
const S_IFBLK: u32 = 0o060000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;
const S_IFSOCK: u32 = 0o140000;

type OpenCpioArchive = Box<cpio_archive::ChainedCpioReader<File>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpioEntryKind {
    Regular,
    Directory,
    Symlink,
    HardLink,
    CharacterDevice,
    BlockDevice,
    Fifo,
    Socket,
    UnknownSpecial,
}

impl CpioEntryKind {
    fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }

    fn is_extractable(self) -> bool {
        matches!(self, Self::Regular | Self::Directory)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Regular => "regular file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::HardLink => "hard link",
            Self::CharacterDevice => "character device",
            Self::BlockDevice => "block device",
            Self::Fifo => "FIFO",
            Self::Socket => "socket",
            Self::UnknownSpecial => "special entry",
        }
    }
}

#[derive(Debug, Clone)]
struct CpioEntryRecord {
    entry: Entry,
    kind: CpioEntryKind,
}

/// Read-only CPIO reader.
pub struct CpioReader {
    path: PathBuf,
    format: ArchiveFormat,
}

impl fmt::Debug for CpioReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CpioReader")
            .field("format", &self.format)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl CpioReader {
    /// Create a new CPIO reader for the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            format: ArchiveFormat::Cpio,
        }
    }

    fn open_archive(&self) -> GeeZipResult<OpenCpioArchive> {
        open_cpio_archive(&self.path)
    }

    fn scan_records(&self) -> GeeZipResult<Vec<CpioEntryRecord>> {
        let mut reader = self.open_archive()?;
        let mut records = Vec::new();

        while let Some(header) = reader
            .read_next()
            .map_err(|err| convert_cpio_error(err, format!("reading '{}'", self.path.display())))?
        {
            records.push(cpio_header_to_record(&*header)?);
        }

        Ok(records)
    }

    fn collect_entries(&self) -> GeeZipResult<Vec<Entry>> {
        Ok(self
            .scan_records()?
            .into_iter()
            .map(|record| record.entry)
            .collect())
    }
}

impl ArchiveReader for CpioReader {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        self.collect_entries()
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        let mut reader = self.open_archive()?;

        while let Some(header) = reader
            .read_next()
            .map_err(|err| convert_cpio_error(err, format!("reading '{}'", self.path.display())))?
        {
            let record = cpio_header_to_record(&*header)?;
            if record.entry.path != entry.path {
                continue;
            }

            return match record.kind {
                CpioEntryKind::Directory => Ok(0),
                CpioEntryKind::Regular => std::io::copy(&mut reader, writer).map_err(|err| {
                    GeeZipError::io(err, format!("extracting CPIO entry '{}'", entry.path))
                }),
                _ => Err(unsupported_extract_error(&record.entry.path, record.kind)),
            };
        }

        Err(GeeZipError::EntryNotFound {
            name: entry.path.clone(),
        })
    }

    fn extract_all(&mut self, dest: &Path, overwrite: bool) -> GeeZipResult<ExtractReport> {
        self.extract_all_with_cancel(dest, overwrite, &|| false)
    }

    fn extract_all_with_cancel(
        &mut self,
        dest: &Path,
        overwrite: bool,
        is_cancelled: &dyn Fn() -> bool,
    ) -> GeeZipResult<ExtractReport> {
        let records = self.scan_records()?;
        let mut report = ExtractReport::default();
        let dest = normalize_path(dest);

        for record in &records {
            if is_cancelled() {
                return Err(GeeZipError::Cancelled);
            }

            let entry = &record.entry;
            let entry_path = Path::new(&entry.path);
            let target = match check_entry_path_safety(entry_path, &entry.path, &dest) {
                Ok(target) => target,
                Err((name, err)) => {
                    report.errors.push((name, err));
                    continue;
                }
            };

            if record.kind.is_directory() {
                if let Err(err) = std::fs::create_dir_all(&target) {
                    report.errors.push((
                        entry.path.clone(),
                        GeeZipError::io(err, "creating directory"),
                    ));
                    continue;
                }
                report.files_extracted += 1;
                continue;
            }

            if !record.kind.is_extractable() {
                report.errors.push((
                    entry.path.clone(),
                    unsupported_extract_error(&entry.path, record.kind),
                ));
                continue;
            }

            if let Some(parent) = target.parent() {
                if !parent.exists() {
                    if let Err(err) = std::fs::create_dir_all(parent) {
                        report.errors.push((
                            entry.path.clone(),
                            GeeZipError::io(err, "creating parent directory"),
                        ));
                        continue;
                    }
                }
            }

            let output = if overwrite {
                match std::fs::File::create(&target) {
                    Ok(file) => file,
                    Err(err) => {
                        report.errors.push((
                            entry.path.clone(),
                            GeeZipError::io(
                                err,
                                format!("creating output file '{}'", target.display()),
                            ),
                        ));
                        continue;
                    }
                }
            } else {
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                {
                    Ok(file) => file,
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                        report.files_skipped += 1;
                        report.errors.push((
                            entry.path.clone(),
                            GeeZipError::clobber_denied(target.display().to_string()),
                        ));
                        continue;
                    }
                    Err(err) => {
                        report.errors.push((
                            entry.path.clone(),
                            GeeZipError::io(
                                err,
                                format!("creating output file '{}'", target.display()),
                            ),
                        ));
                        continue;
                    }
                }
            };

            let mut output = CancellableWriter::new(output, is_cancelled);
            let extract_result = self.extract(entry, &mut output);
            let was_cancelled = output.was_cancelled();
            drop(output);

            match extract_result {
                Ok(bytes) => {
                    if was_cancelled {
                        let _ = std::fs::remove_file(&target);
                        return Err(GeeZipError::Cancelled);
                    }
                    report.files_extracted += 1;
                    report.bytes_extracted += bytes;
                }
                Err(err) => {
                    let _ = std::fs::remove_file(&target);
                    if was_cancelled {
                        return Err(GeeZipError::Cancelled);
                    }
                    report.errors.push((entry.path.clone(), err));
                }
            }
        }

        Ok(report)
    }
}

/// Verify a CPIO archive by streaming every entry payload to `sink()`.
///
/// This helper is used by CLI/GUI `test` flows so archives containing
/// symlinks or other special entries can still be integrity-checked without
/// requiring filesystem creation semantics.
pub fn verify_cpio_archive(path: &Path) -> GeeZipResult<TestReport> {
    let mut reader = open_cpio_archive(path)?;
    let mut entry_count = 0u64;
    let mut bytes_read = 0u64;

    while let Some(header) = reader
        .read_next()
        .map_err(|err| convert_cpio_error(err, format!("verifying '{}'", path.display())))?
    {
        let record = cpio_header_to_record(&*header)?;
        entry_count += 1;

        if record.kind.is_directory() {
            continue;
        }

        let bytes = std::io::copy(&mut reader, &mut std::io::sink()).map_err(|err| {
            GeeZipError::io(err, format!("verifying CPIO entry '{}'", record.entry.path))
        })?;
        bytes_read += bytes;
    }

    Ok(TestReport {
        format: ArchiveFormat::Cpio,
        entry_count,
        bytes_read,
        crc32_verified: false,
    })
}

fn open_cpio_archive(path: &Path) -> GeeZipResult<OpenCpioArchive> {
    let file = File::open(path)
        .map_err(|err| GeeZipError::io(err, format!("opening '{}'", path.display())))?;
    cpio_archive::reader(file)
        .map_err(|err| convert_cpio_error(err, format!("reading '{}'", path.display())))
}

fn cpio_header_to_record(header: &dyn CpioHeader) -> GeeZipResult<CpioEntryRecord> {
    let kind = entry_kind_from_header(header);
    let path = normalize_cpio_entry_path(header.name(), kind.is_directory());
    if path.is_empty() {
        return Err(GeeZipError::format(
            "CPIO entry is missing a pathname",
            ArchiveFormat::Cpio,
        ));
    }

    Ok(CpioEntryRecord {
        entry: Entry {
            path,
            size: header.file_size(),
            compressed_size: 0,
            crc32: None,
            modified: Some(u64::from(header.mtime())),
            is_dir: kind.is_directory(),
        },
        kind,
    })
}

fn entry_kind_from_header(header: &dyn CpioHeader) -> CpioEntryKind {
    let mode = header.mode();
    let file_type = mode & S_IFMT;

    match file_type {
        S_IFDIR => CpioEntryKind::Directory,
        S_IFLNK => CpioEntryKind::Symlink,
        S_IFCHR => CpioEntryKind::CharacterDevice,
        S_IFBLK => CpioEntryKind::BlockDevice,
        S_IFIFO => CpioEntryKind::Fifo,
        S_IFSOCK => CpioEntryKind::Socket,
        S_IFREG => {
            if header.nlink() > 1 && header.file_size() == 0 {
                CpioEntryKind::HardLink
            } else {
                CpioEntryKind::Regular
            }
        }
        _ if header.name().ends_with('/') => CpioEntryKind::Directory,
        _ if header.nlink() > 1 && header.file_size() == 0 => CpioEntryKind::HardLink,
        _ if file_type == 0 => CpioEntryKind::Regular,
        _ => CpioEntryKind::UnknownSpecial,
    }
}

fn normalize_cpio_entry_path(name: &str, is_dir: bool) -> String {
    let mut path = name.replace('\\', "/");
    if is_dir {
        while path.len() > 1 && path.ends_with('/') {
            path.pop();
        }
    }
    path
}

fn unsupported_extract_error(path: &str, kind: CpioEntryKind) -> GeeZipError {
    GeeZipError::format(
        format!(
            "CPIO entry '{}' is a {}; GeeZipX's read-only CPIO MVP does not create these filesystem objects during extraction",
            path,
            kind.label()
        ),
        ArchiveFormat::Cpio,
    )
}

fn convert_cpio_error(err: cpio_archive::Error, context: impl Into<String>) -> GeeZipError {
    let context = context.into();
    match err {
        cpio_archive::Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
                GeeZipError::format(
                    format!("invalid CPIO archive: {io_err}"),
                    ArchiveFormat::Cpio,
                )
            }
            _ => GeeZipError::io(io_err, context),
        },
        cpio_archive::Error::BadMagic => GeeZipError::format(
            "unsupported or invalid CPIO archive: GeeZipX currently supports only newc and odc streams",
            ArchiveFormat::Cpio,
        ),
        cpio_archive::Error::BadHeaderString
        | cpio_archive::Error::BadHeaderHex(_)
        | cpio_archive::Error::FilenameDecode
        | cpio_archive::Error::ValueTooLarge
        | cpio_archive::Error::SizeMismatch
        | cpio_archive::Error::NotAFile(_) => {
            GeeZipError::format(format!("invalid CPIO archive: {err}"), ArchiveFormat::Cpio)
        }
    }
}
// ---------------------------------------------------------------------------
// CpioWriter
// ---------------------------------------------------------------------------

const MAGIC_NEWC: &[u8] = b"070701";
const TRAILER_FILENAME: &str = "TRAILER!!!";
const DEFAULT_FILE_MODE: u32 = 0o100000 | 0o644; // S_IFREG | 0o644
const DEFAULT_DIR_MODE: u32 = 0o040000 | 0o755; // S_IFDIR | 0o755

/// CPIO archive writer.
///
/// Supports both `newc` (SVR4) and `odc` (portable ASCII) formats.
/// Construct via [`CpioWriter::new`] for newc or [`CpioWriter::new_odc`]
/// for odc, then add entries with
/// [`add_entry_from_reader`](ArchiveWriter::add_entry_from_reader),
/// then finalise with either:
///
/// - [`CpioWriter::finalize`] 鈥?returns `(total_bytes, inner_writer)`
/// - [`ArchiveWriter::finish`] 鈥?returns `total_bytes` (trait object-safe)
pub struct CpioWriter<W: Write + Send> {
    inner: Option<CpioWriterInner<W>>,
    format: ArchiveFormat,
    variant: CpioWriteVariant,
    inode_counter: u32,
}

enum CpioWriterInner<W: Write + Send> {
    Newc(CountWriter<W>),
    Odc(OdcBuilder<CountWriter<W>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpioWriteVariant {
    Newc,
    Odc,
}

impl<W: Write + Send> fmt::Debug for CpioWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CpioWriter")
            .field("format", &self.format)
            .field("variant", &self.variant)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Send> CpioWriter<W> {
    /// Create a new CPIO writer in newc (SVR4) format.
    pub fn new(writer: W) -> Self {
        let counter = CountWriter {
            inner: writer,
            count: 0,
        };
        CpioWriter {
            inner: Some(CpioWriterInner::Newc(counter)),
            format: ArchiveFormat::Cpio,
            variant: CpioWriteVariant::Newc,
            inode_counter: 0,
        }
    }

    /// Create a new CPIO writer in odc (portable ASCII) format.
    pub fn new_odc(writer: W) -> Self {
        let counter = CountWriter {
            inner: writer,
            count: 0,
        };
        let mut builder = OdcBuilder::new(counter);
        builder.auto_write_dirs(false);
        CpioWriter {
            inner: Some(CpioWriterInner::Odc(builder)),
            format: ArchiveFormat::Cpio,
            variant: CpioWriteVariant::Odc,
            inode_counter: 0,
        }
    }

    /// Finalise the archive and return the inner writer alongside
    /// the total number of bytes written.
    pub fn finalize(mut self) -> GeeZipResult<(u64, W)> {
        let inner = self.inner.take().ok_or_else(|| GeeZipError::Format {
            message: "CPIO writer already finalised".into(),
            format: ArchiveFormat::Cpio,
        })?;

        match inner {
            CpioWriterInner::Newc(mut cw) => {
                write_newc_trailer(&mut cw, &mut self.inode_counter)?;
                // write_newc_trailer already appended the TRAILER!!! entry above.
                let bytes = cw.count;
                Ok((bytes, cw.inner))
            }
            CpioWriterInner::Odc(mut builder) => {
                builder
                    .finish()
                    .map_err(|e| convert_odc_write_error(e, "<trailer>"))?;
                let counter = builder
                    .into_inner()
                    .map_err(|e| convert_odc_write_error(e, "<close>"))?;
                let bytes = counter.count;
                Ok((bytes, counter.inner))
            }
        }
    }
}

impl<W: Write + Send> ArchiveWriter for CpioWriter<W> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn add_entry_from_reader(&mut self, path: &Path, reader: &mut dyn Read) -> GeeZipResult<()> {
        let name = validate_cpio_write_path(path)?;

        // Buffer the data to know the size (CPIO requires size in header).
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(|e| GeeZipError::io(e, format!("reading data for entry '{name}'")))?;

        self.inode_counter += 1;
        let inode = self.inode_counter;
        let file_size = data.len() as u32;

        let inner = self.inner.as_mut().ok_or_else(|| GeeZipError::Format {
            message: "CPIO writer not initialised (already consumed)".into(),
            format: ArchiveFormat::Cpio,
        })?;

        match inner {
            CpioWriterInner::Newc(cw) => {
                write_newc_entry(cw, inode, &name, file_size, DEFAULT_FILE_MODE)?;
                if file_size > 0 {
                    cw.write_all(&data).map_err(|e| {
                        GeeZipError::io(e, format!("writing data for entry '{name}'"))
                    })?;
                }
                write_newc_data_padding(cw, file_size)?;
            }
            CpioWriterInner::Odc(builder) => {
                let mut header = builder.next_header();
                header.inode = inode;
                header.name = name.clone();
                header.mode = DEFAULT_FILE_MODE;
                header.nlink = 1;
                header.file_size = file_size as u64;
                builder
                    .append_header_with_data(header, &data)
                    .map_err(|e| convert_odc_write_error(e, &name))?;
            }
        }

        Ok(())
    }

    fn add_directory(&mut self, path: &Path) -> GeeZipResult<()> {
        let mut name = validate_cpio_write_path(path)?;
        // Ensure trailing `/` for directory entries.
        if !name.ends_with('/') {
            name.push('/');
        }

        self.inode_counter += 1;
        let inode = self.inode_counter;

        let inner = self.inner.as_mut().ok_or_else(|| GeeZipError::Format {
            message: "CPIO writer not initialised (already consumed)".into(),
            format: ArchiveFormat::Cpio,
        })?;

        match inner {
            CpioWriterInner::Newc(cw) => {
                write_newc_entry(cw, inode, &name, 0, DEFAULT_DIR_MODE)?;
                write_newc_data_padding(cw, 0)?;
            }
            CpioWriterInner::Odc(builder) => {
                let mut header = builder.next_header();
                header.inode = inode;
                header.name = name.clone();
                header.mode = DEFAULT_DIR_MODE;
                header.nlink = 2;
                header.file_size = 0;
                builder
                    .append_header_with_data(header, [])
                    .map_err(|e| convert_odc_write_error(e, &name))?;
            }
        }

        Ok(())
    }

    fn finish(self: Box<Self>) -> GeeZipResult<u64> {
        let (bytes, _writer) = (*self).finalize()?;
        Ok(bytes)
    }
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validate a CPIO write path. Rejects absolute paths, paths containing
/// `..`, and Windows drive-prefixed/UNC paths. Returns the path as a
/// forward-slash-separated string.
fn validate_cpio_write_path(path: &Path) -> GeeZipResult<String> {
    let path_str = path.to_string_lossy();

    // Reject absolute paths.
    if path.has_root() {
        return Err(GeeZipError::PathTraversal {
            entry: path_str.into_owned(),
            target: "archive root".into(),
        });
    }

    // Reject Windows drive-letter and UNC paths.
    let path_str_check = path_str.replace('/', "\\");
    let bytes = path_str_check.as_bytes();
    if path_str_check.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\')
    {
        return Err(GeeZipError::PathTraversal {
            entry: path_str.into_owned(),
            target: "archive root".into(),
        });
    }

    // Normalise and reject parent-dir traversal.
    let norm = normalize_path(path);
    let norm_str = norm.to_string_lossy();

    // Check if any component is ".." (explicit traversal attempt).
    if norm
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(GeeZipError::PathTraversal {
            entry: path_str.into_owned(),
            target: "archive root".into(),
        });
    }

    // Retain forward slashes (the normalised path may have OS-dependent
    // separators; convert back to forward slashes for cpio).
    Ok(norm_str.replace('\\', "/"))
}

// ---------------------------------------------------------------------------
// newc header helpers
// ---------------------------------------------------------------------------

/// Write a newc (SVR4) CPIO entry header followed by the filename.
///
/// The header consists of a 6-byte magic (`070701`) followed by 13
/// 8-character hexadecimal fields (104 bytes) and a NUL-terminated
/// filename padded to a 4-byte boundary.
fn write_newc_entry<Wr: Write>(
    writer: &mut Wr,
    inode: u32,
    name: &str,
    file_size: u32,
    mode: u32,
) -> GeeZipResult<u64> {
    let mut header_bytes = 0u64;

    // Magic.
    writer
        .write_all(MAGIC_NEWC)
        .map_err(|e| GeeZipError::io(e, "writing newc header magic"))?;
    header_bytes += 6;

    // Write each field as 8-character uppercase hex.
    let fields: &[(u32, &str)] = &[
        (inode, "inode"),
        (mode, "mode"),
        (0, "uid"),
        (0, "gid"),
        (1, "nlink"),
        (0, "mtime"),
        (file_size, "filesize"),
        (0, "devmajor"),
        (0, "devminor"),
        (0, "rdevmajor"),
        (0, "rdevminor"),
        ((name.len() + 1) as u32, "namesize"),
        (0, "check"),
    ];

    for &(value, label) in fields {
        let hex = format!("{value:08X}");
        writer
            .write_all(hex.as_bytes())
            .map_err(|e| GeeZipError::io(e, format!("writing newc header field '{label}'")))?;
        header_bytes += 8;
    }

    // Filename + NUL.
    writer
        .write_all(name.as_bytes())
        .map_err(|e| GeeZipError::io(e, "writing newc filename"))?;
    writer
        .write_all(b"\0")
        .map_err(|e| GeeZipError::io(e, "writing newc filename NUL"))?;
    header_bytes += (name.len() + 1) as u64;

    // Pad to 4-byte boundary (110-byte header + filename + NUL must be aligned).
    let position_after_name = 110 + name.len() as u64 + 1;
    let name_padding = (4 - position_after_name % 4) % 4;
    if name_padding > 0 {
        writer
            .write_all(&vec![0u8; name_padding as usize])
            .map_err(|e| GeeZipError::io(e, "writing newc name padding"))?;
        header_bytes += name_padding;
    }

    Ok(header_bytes)
}

/// Write NUL padding bytes to align to a 4-byte boundary after
/// `file_size` bytes of data.
fn write_newc_data_padding<Wr: Write>(writer: &mut Wr, file_size: u32) -> GeeZipResult<u64> {
    let padding = (4 - (file_size % 4)) % 4;
    if padding > 0 {
        writer
            .write_all(&vec![0u8; padding as usize])
            .map_err(|e| GeeZipError::io(e, "writing newc data padding"))?;
    }
    Ok(padding as u64)
}

/// Write the newc trailer entry (`TRAILER!!!`). The trailer must use
/// inode 0 to mark the end of the archive.
fn write_newc_trailer<Wr: Write>(writer: &mut Wr, _inode_counter: &mut u32) -> GeeZipResult<()> {
    write_newc_entry(writer, 0, TRAILER_FILENAME, 0, DEFAULT_FILE_MODE)?;
    write_newc_data_padding(writer, 0)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ODC error conversion
// ---------------------------------------------------------------------------

fn convert_odc_write_error(err: cpio_archive::Error, entry_name: &str) -> GeeZipError {
    match err {
        cpio_archive::Error::Io(io_err) => {
            GeeZipError::io(io_err, format!("writing odc entry '{entry_name}'"))
        }
        other => GeeZipError::format(
            format!("error writing odc entry '{entry_name}': {other}"),
            ArchiveFormat::Cpio,
        ),
    }
}
