//! LZH/LHA (`.lzh`, `.lha`) archive reader and writer.
//!
//! GeeZipX exposes LZH/LHA reading via the `delharc` decoder and writing
//! via `oxiarc-lzhuf` (鈮?v0.3.5) for compression methods lh0 (store) through
//! lh7 (64 KB window, static Huffman).  The writer supports configurable
//! compression levels 0-4 mapping to lh0-lh7.
//!
//! **Important**: `oxiarc-lzhuf` v0.3.4+ uses MSB-first bit packing
//! (the canonical LZH convention), making the compressed output compatible
//! with standard LZH decoders like `delharc`.  Versions 鈮?0.3.3 used a
//! non-standard LSB-first packing that could not be decoded by `delharc`.
//! The project requires 鈮?0.3.5 to ensure write-read compatibility.
//!
//! Path handling notes:
//! - `delharc`'s parsed pathname helpers intentionally normalise separators and
//!   strip `.` / `..` / empty components. GeeZipX therefore inspects the raw
//!   filename and extended path headers first so dangerous names are surfaced
//!   as path-traversal errors instead of being silently renamed on extraction.
//! - Safe paths still use `delharc`'s parsed pathname so non-ASCII and control
//!   bytes are rendered consistently with the upstream decoder.

use std::fmt::{self, Write as _};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use delharc::crc::Crc16;
use delharc::header::{LhaHeader, OsType, ext::EXT_HEADER_FILENAME, ext::EXT_HEADER_PATH};
use delharc::{LhaDecodeReader, LhaError};
use oxiarc_lzhuf::{LzhMethod, encode_lzh};

use crate::archive::{ArchiveReader, ArchiveWriter, Entry, is_entry_path_dangerous};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// Read-only LZH/LHA archive reader.
pub struct LzhReader<R: Read + Seek + Send> {
    inner: R,
    format: ArchiveFormat,
}

impl<R: Read + Seek + Send> fmt::Debug for LzhReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LzhReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek + Send> LzhReader<R> {
    /// Create an LZH reader from any `Read + Seek + Send` source.
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            format: ArchiveFormat::Lzh,
        }
    }
}

impl LzhReader<std::io::Cursor<Vec<u8>>> {
    /// Create an LZH reader from an already-loaded byte buffer.
    pub fn from_buf(buf: Vec<u8>) -> Self {
        Self::new(std::io::Cursor::new(buf))
    }
}

const LZH_LEVEL0_FIXED_HEADER_LEN: usize = 22;
const LZH_LEVEL0_MAX_HEADER_LEN: usize = u8::MAX as usize;
const LZH_LEVEL0_MAX_PATH_LEN: usize = LZH_LEVEL0_MAX_HEADER_LEN - LZH_LEVEL0_FIXED_HEADER_LEN;
const LZH_LEVEL0_MAX_ENTRY_SIZE: u64 = u32::MAX as u64;
const LZH_METHOD_STORE: [u8; 5] = *b"-lh0-";
const LZH_METHOD_DIRECTORY: [u8; 5] = *b"-lhd-";
const LZH_ATTR_FILE: u8 = 0x20;
const LZH_ATTR_DIRECTORY: u8 = 0x10;
const LZH_READ_CHUNK_SIZE: usize = 65_536;
const LZH_METHOD_LH4: [u8; 5] = *b"-lh4-";
const LZH_METHOD_LH5: [u8; 5] = *b"-lh5-";
const LZH_METHOD_LH6: [u8; 5] = *b"-lh6-";
const LZH_METHOD_LH7: [u8; 5] = *b"-lh7-";
/// Compression method for LZH/LHA writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LzhCompressionMethod {
    Store,
    Lh4,
    Lh5,
    Lh6,
    Lh7,
}

impl LzhCompressionMethod {
    /// Convert to the 5-byte method ID used in LZH headers.
    pub fn method_bytes(self) -> [u8; 5] {
        match self {
            Self::Store => LZH_METHOD_STORE,
            Self::Lh4 => LZH_METHOD_LH4,
            Self::Lh5 => LZH_METHOD_LH5,
            Self::Lh6 => LZH_METHOD_LH6,
            Self::Lh7 => LZH_METHOD_LH7,
        }
    }

    /// Convert to an `oxiarc_lzhuf::LzhMethod`.
    pub fn to_lzh_method(self) -> LzhMethod {
        match self {
            Self::Store => LzhMethod::Lh0,
            Self::Lh4 => LzhMethod::Lh4,
            Self::Lh5 => LzhMethod::Lh5,
            Self::Lh6 => LzhMethod::Lh6,
            Self::Lh7 => LzhMethod::Lh7,
        }
    }

    /// Whether this method does actual compression.
    pub fn is_store(self) -> bool {
        matches!(self, Self::Store)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for LzhCompressionMethod {
    fn default() -> Self {
        Self::Lh5
    }
}

fn lzh_crc16(data: &[u8]) -> u16 {
    let mut crc = Crc16::default();
    crc.digest(data);
    crc.sum16()
}

fn lzh_header_checksum(header: &[u8]) -> u8 {
    header.iter().fold(0u8, |acc, byte| acc.wrapping_add(*byte))
}

fn lzh_raw_text_path_is_dangerous(path: &str) -> bool {
    raw_lzh_path_is_dangerous(&split_raw_lzh_path_components(path.as_bytes(), false))
}

fn invalid_lzh_writer_path_error(path: &str) -> GeeZipError {
    GeeZipError::format(
        format!(
            "invalid LZH entry path '{}': absolute paths, UNC paths, drive-prefixed names, and '.'/'..' components are not allowed",
            path
        ),
        ArchiveFormat::Lzh,
    )
}

fn lzh_entry_size_limit_error(path: &Path, max_size: u64) -> GeeZipError {
    let limit = if max_size == LZH_LEVEL0_MAX_ENTRY_SIZE {
        String::from("4 GiB")
    } else {
        format!("{max_size} bytes")
    };

    GeeZipError::format(
        format!(
            "LZH writer does not support entries larger than {limit}: {}",
            path.display()
        ),
        ArchiveFormat::Lzh,
    )
}

fn normalize_lzh_writer_path(path: &Path, is_dir: bool) -> GeeZipResult<Vec<u8>> {
    let raw = path.to_str().ok_or_else(|| {
        GeeZipError::format(
            format!("non-UTF-8 path: {}", path.display()),
            ArchiveFormat::Lzh,
        )
    })?;

    let mut normalized = raw.replace('\\', "/");
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }

    if normalized.is_empty() {
        return Err(GeeZipError::format(
            "LZH entry path cannot be empty",
            ArchiveFormat::Lzh,
        ));
    }

    if lzh_raw_text_path_is_dangerous(&normalized)
        || is_entry_path_dangerous(Path::new(&normalized))
    {
        return Err(invalid_lzh_writer_path_error(&normalized));
    }

    if is_dir {
        normalized.push('/');
    }

    let path_bytes = normalized.into_bytes();
    if path_bytes.len() > LZH_LEVEL0_MAX_PATH_LEN {
        return Err(GeeZipError::format(
            format!(
                "LZH level-0 pathname too long ({} bytes, max {} bytes): {}",
                path_bytes.len(),
                LZH_LEVEL0_MAX_PATH_LEN,
                path.display()
            ),
            ArchiveFormat::Lzh,
        ));
    }

    Ok(path_bytes)
}

fn build_lzh_level0_header(
    path_bytes: &[u8],
    method: [u8; 5],
    compressed_size: u32,
    original_size: u32,
    file_crc: u16,
    attrs: u8,
) -> GeeZipResult<Vec<u8>> {
    let header_len = LZH_LEVEL0_FIXED_HEADER_LEN + path_bytes.len();
    if header_len > LZH_LEVEL0_MAX_HEADER_LEN {
        return Err(GeeZipError::format(
            format!(
                "LZH level-0 header too large ({} bytes, max {} bytes)",
                header_len, LZH_LEVEL0_MAX_HEADER_LEN
            ),
            ArchiveFormat::Lzh,
        ));
    }

    let mut header = Vec::with_capacity(header_len);
    header.extend_from_slice(&method);
    header.extend_from_slice(&compressed_size.to_le_bytes());
    header.extend_from_slice(&original_size.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.push(attrs);
    header.push(0);
    header.push(u8::try_from(path_bytes.len()).map_err(|_| {
        GeeZipError::format(
            format!(
                "LZH level-0 pathname too long ({} bytes, max {} bytes)",
                path_bytes.len(),
                LZH_LEVEL0_MAX_PATH_LEN
            ),
            ArchiveFormat::Lzh,
        )
    })?);
    header.extend_from_slice(path_bytes);
    header.extend_from_slice(&file_crc.to_le_bytes());

    Ok(header)
}

/// Write an LZH entry header and payload.
///
/// `compressed_size` and `original_size` may differ for compressed entries.
/// `file_crc` is the CRC-16 of the **original** (uncompressed) data.
#[allow(clippy::too_many_arguments)]
fn write_lzh_entry<W: Write>(
    writer: &mut W,
    path: &Path,
    path_bytes: &[u8],
    method: [u8; 5],
    compressed_size: u32,
    original_size: u32,
    file_crc: u16,
    is_dir: bool,
    payload: &[u8],
) -> GeeZipResult<()> {
    let header = build_lzh_level0_header(
        path_bytes,
        method,
        compressed_size,
        original_size,
        file_crc,
        if is_dir {
            LZH_ATTR_DIRECTORY
        } else {
            LZH_ATTR_FILE
        },
    )?;
    let header_len = u8::try_from(header.len()).map_err(|_| {
        GeeZipError::format(
            format!(
                "LZH level-0 header too large ({} bytes, max {} bytes)",
                header.len(),
                LZH_LEVEL0_MAX_HEADER_LEN
            ),
            ArchiveFormat::Lzh,
        )
    })?;

    writer
        .write_all(&[header_len, lzh_header_checksum(&header)])
        .map_err(|err| {
            GeeZipError::io(err, format!("writing LZH header for '{}'", path.display()))
        })?;
    writer.write_all(&header).map_err(|err| {
        GeeZipError::io(err, format!("writing LZH header for '{}'", path.display()))
    })?;
    if !is_dir {
        writer.write_all(payload).map_err(|err| {
            GeeZipError::io(err, format!("writing LZH payload for '{}'", path.display()))
        })?;
    }

    Ok(())
}

fn read_lzh_entry_data(path: &Path, reader: &mut dyn Read, max_size: u64) -> GeeZipResult<Vec<u8>> {
    let mut data = Vec::new();
    let mut chunk = [0u8; LZH_READ_CHUNK_SIZE];

    loop {
        let n = reader.read(&mut chunk).map_err(|err| {
            GeeZipError::io(
                err,
                format!("reading data for LZH entry '{}'", path.display()),
            )
        })?;
        if n == 0 {
            break;
        }

        let next_len = data
            .len()
            .checked_add(n)
            .ok_or_else(|| lzh_entry_size_limit_error(path, max_size))?;
        let next_len =
            u64::try_from(next_len).map_err(|_| lzh_entry_size_limit_error(path, max_size))?;
        if next_len > max_size {
            return Err(lzh_entry_size_limit_error(path, max_size));
        }

        data.extend_from_slice(&chunk[..n]);
    }

    Ok(data)
}

pub struct LzhWriter<W: Write + Seek + Send> {
    inner: Option<W>,
    start_pos: u64,
    format: ArchiveFormat,
    method: LzhCompressionMethod,
}

impl<W: Write + Seek + Send> fmt::Debug for LzhWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LzhWriter")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Seek + Send> LzhWriter<W> {
    pub fn new(mut writer: W, method: LzhCompressionMethod) -> Self {
        let start_pos = writer.stream_position().unwrap_or(0);
        Self {
            inner: Some(writer),
            start_pos,
            format: ArchiveFormat::Lzh,
            method,
        }
    }

    /// Create a new LZH writer with store-only (no compression) mode.
    pub fn new_store_only(writer: W) -> Self {
        Self::new(writer, LzhCompressionMethod::Store)
    }

    pub fn finalize(mut self) -> GeeZipResult<(u64, W)> {
        let mut writer = self.inner.take().ok_or_else(|| {
            GeeZipError::format("LZH writer already finalised", ArchiveFormat::Lzh)
        })?;
        writer
            .write_all(&[0])
            .map_err(|err| GeeZipError::io(err, "writing LZH end marker"))?;
        writer
            .flush()
            .map_err(|err| GeeZipError::io(err, "flushing LZH archive"))?;
        let end_pos = writer
            .stream_position()
            .map_err(|err| GeeZipError::io(err, "getting final LZH archive size"))?;
        Ok((end_pos - self.start_pos, writer))
    }
}

fn lzh_modified_timestamp(header: &LhaHeader) -> Option<u64> {
    header
        .parse_last_modified()
        .to_utc()
        .and_then(|dt| u64::try_from(dt.timestamp()).ok())
}

fn lzh_is_amiga_nilterm(header: &LhaHeader) -> bool {
    header.parse_os_type() == Ok(OsType::Amiga)
}

fn split_raw_lzh_path_components(data: &[u8], nilterm: bool) -> Vec<Vec<u8>> {
    let mut components = Vec::new();
    let mut current = Vec::new();

    for &byte in data {
        if nilterm && byte == 0 {
            break;
        }

        if matches!(byte, 0xFF | b'/' | b'\\') {
            components.push(std::mem::take(&mut current));
        } else {
            current.push(byte);
        }
    }

    components.push(current);
    components
}

fn trim_trailing_empty_raw_components(components: &mut Vec<Vec<u8>>) {
    while components
        .last()
        .is_some_and(|component| component.is_empty())
    {
        components.pop();
    }
}

fn append_raw_lzh_path_source(components: &mut Vec<Vec<u8>>, data: &[u8], nilterm: bool) {
    if !components.is_empty() {
        trim_trailing_empty_raw_components(components);
    }
    components.extend(split_raw_lzh_path_components(data, nilterm));
}

fn raw_lzh_path_components(header: &LhaHeader) -> Vec<Vec<u8>> {
    let nilterm = lzh_is_amiga_nilterm(header);
    let mut components = Vec::new();
    let mut raw_path_headers = Vec::new();
    let mut filename_header = None;

    for extra in header.iter_extra() {
        match extra {
            [EXT_HEADER_FILENAME, data @ ..] => filename_header = Some(data),
            [EXT_HEADER_PATH, data @ ..] => raw_path_headers.push(data),
            _ => {}
        }
    }

    for data in raw_path_headers {
        append_raw_lzh_path_source(&mut components, data, false);
    }

    if let Some(data) = filename_header {
        append_raw_lzh_path_source(&mut components, data, nilterm);
    } else if !header.filename.is_empty() {
        append_raw_lzh_path_source(&mut components, &header.filename, nilterm);
    }

    components
}

fn raw_lzh_component_display(component: &[u8]) -> String {
    let mut out = String::with_capacity(component.len());
    for &byte in component {
        match byte {
            0x20..=0x7E => out.push(byte as char),
            _ => {
                let _ = write!(out, "%{:02x}", byte);
            }
        }
    }
    out
}

fn raw_lzh_display_path(components: &[Vec<u8>]) -> String {
    if components.is_empty() {
        return String::new();
    }

    let mut display = String::new();
    for (index, component) in components.iter().enumerate() {
        if index > 0 {
            display.push('/');
        }
        display.push_str(&raw_lzh_component_display(component));
    }
    display
}

fn raw_lzh_path_is_dangerous(components: &[Vec<u8>]) -> bool {
    let Some(last_index) = components.len().checked_sub(1) else {
        return true;
    };

    for (index, component) in components.iter().enumerate() {
        if component.is_empty() {
            if index == last_index {
                continue;
            }
            return true;
        }

        if component.as_slice() == b"." || component.as_slice() == b".." {
            return true;
        }

        if index == 0
            && component.len() >= 2
            && component[0].is_ascii_alphabetic()
            && component[1] == b':'
        {
            return true;
        }
    }

    false
}

fn normalize_lzh_display_path(mut path: String, is_dir: bool) -> String {
    if is_dir {
        while path.len() > 1 && path.ends_with('/') {
            path.pop();
        }
    }
    path
}

fn lzh_entry_path(header: &LhaHeader) -> GeeZipResult<String> {
    let raw_components = raw_lzh_path_components(header);
    let raw_display =
        normalize_lzh_display_path(raw_lzh_display_path(&raw_components), header.is_directory());
    if raw_display.is_empty() {
        return Err(GeeZipError::format(
            "LZH entry is missing a pathname",
            ArchiveFormat::Lzh,
        ));
    }

    if raw_lzh_path_is_dangerous(&raw_components) {
        return Ok(raw_display);
    }

    let path = normalize_lzh_display_path(
        header.parse_pathname_to_str().replace('\\', "/"),
        header.is_directory(),
    );
    if path.is_empty() {
        return Err(GeeZipError::format(
            "LZH entry is missing a pathname",
            ArchiveFormat::Lzh,
        ));
    }

    Ok(path)
}

fn compression_label(header: &LhaHeader) -> String {
    match header.compression_method() {
        Ok(method) => method.to_string(),
        Err(_) => String::from_utf8_lossy(&header.compression).into_owned(),
    }
}

fn unsupported_method_error(header: &LhaHeader, path: &str) -> GeeZipError {
    GeeZipError::format(
        format!(
            "unsupported LZH compression method '{}' for entry '{}'",
            compression_label(header),
            path
        ),
        ArchiveFormat::Lzh,
    )
}

fn convert_lha_error(err: LhaError<std::io::Error>, context: impl Into<String>) -> GeeZipError {
    match err {
        LhaError::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
                GeeZipError::format(format!("invalid LZH archive: {io_err}"), ArchiveFormat::Lzh)
            }
            _ => GeeZipError::io(io_err, context),
        },
        LhaError::HeaderParse(message) => GeeZipError::format(
            format!("invalid LZH archive: {message}"),
            ArchiveFormat::Lzh,
        ),
        LhaError::Decompress(message) => GeeZipError::format(
            format!("failed to decompress LZH entry: {message}"),
            ArchiveFormat::Lzh,
        ),
        LhaError::Checksum(message) => GeeZipError::format(
            format!("LZH CRC-16 verification failed: {message}"),
            ArchiveFormat::Lzh,
        ),
        _ => GeeZipError::format("LZH decoder error", ArchiveFormat::Lzh),
    }
}

fn convert_lha_decode_error<E>(err: E, context: impl Into<String>) -> GeeZipError
where
    E: Into<LhaError<std::io::Error>>,
{
    convert_lha_error(err.into(), context)
}

fn dangerous_lzh_entry_error(path: &str) -> GeeZipError {
    GeeZipError::PathTraversal {
        entry: path.to_string(),
        target: "requested extraction target".into(),
    }
}

fn scan_lzh_entries<R: Read + Seek + Send>(inner: &mut R) -> GeeZipResult<Vec<Entry>> {
    inner.seek(SeekFrom::Start(0))?;
    let mut reader = LhaDecodeReader::new(&mut *inner)
        .map_err(|err| convert_lha_decode_error(err, "reading LZH archive"))?;
    let mut entries = Vec::new();

    loop {
        let header = reader.header();
        let path = lzh_entry_path(header)?;
        entries.push(Entry {
            path,
            size: header.original_size,
            compressed_size: header.compressed_size,
            crc32: None,
            modified: lzh_modified_timestamp(header),
            is_dir: header.is_directory(),
        });

        if !reader
            .next_file()
            .map_err(|err| convert_lha_decode_error(err, "reading LZH archive"))?
        {
            break;
        }
    }

    Ok(entries)
}

impl<R: Read + Seek + Send> ArchiveReader for LzhReader<R> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        scan_lzh_entries(&mut self.inner)
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        self.inner.seek(SeekFrom::Start(0))?;
        let mut reader = LhaDecodeReader::new(&mut self.inner)
            .map_err(|err| convert_lha_decode_error(err, "reading LZH archive"))?;

        loop {
            let header = reader.header();
            let path = lzh_entry_path(header)?;
            if path == entry.path {
                let raw_components = raw_lzh_path_components(header);
                if raw_lzh_path_is_dangerous(&raw_components)
                    || is_entry_path_dangerous(Path::new(&path))
                {
                    return Err(dangerous_lzh_entry_error(&path));
                }
                if header.is_directory() {
                    return Ok(0);
                }
                if !reader.is_decoder_supported() {
                    return Err(unsupported_method_error(header, &path));
                }

                let bytes = std::io::copy(&mut reader, writer).map_err(|err| {
                    GeeZipError::io(err, format!("extracting '{}' from LZH archive", entry.path))
                })?;
                reader.crc_check().map_err(|err| {
                    convert_lha_error(
                        err,
                        format!("verifying CRC-16 for '{}' in LZH archive", entry.path),
                    )
                })?;
                return Ok(bytes);
            }

            if !reader
                .next_file()
                .map_err(|err| convert_lha_decode_error(err, "reading LZH archive"))?
            {
                break;
            }
        }

        Err(GeeZipError::EntryNotFound {
            name: entry.path.clone(),
        })
    }
}

impl<W: Write + Seek + Send> ArchiveWriter for LzhWriter<W> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn add_entry_from_reader(&mut self, path: &Path, reader: &mut dyn Read) -> GeeZipResult<()> {
        let writer = self.inner.as_mut().ok_or_else(|| {
            GeeZipError::format(
                "LZH writer not initialised (already consumed)",
                ArchiveFormat::Lzh,
            )
        })?;

        let path_bytes = normalize_lzh_writer_path(path, false)?;
        let data = read_lzh_entry_data(path, reader, LZH_LEVEL0_MAX_ENTRY_SIZE)?;
        let crc = lzh_crc16(&data);
        let original_size = u32::try_from(data.len()).map_err(|_| {
            GeeZipError::format(
                format!(
                    "LZH entry '{}' too large ({} bytes)",
                    path.display(),
                    data.len()
                ),
                ArchiveFormat::Lzh,
            )
        })?;

        if self.method.is_store() {
            // Store (no compression) 鈥?write directly.
            write_lzh_entry(
                writer,
                path,
                &path_bytes,
                LZH_METHOD_STORE,
                original_size,
                original_size,
                crc,
                false,
                &data,
            )
        } else {
            // Attempt compression; fall back to store if compressed data is larger.
            let compressed = encode_lzh(&data, self.method.to_lzh_method()).map_err(|e| {
                GeeZipError::format(
                    format!("LZH compression failed for '{}': {}", path.display(), e),
                    ArchiveFormat::Lzh,
                )
            })?;
            if compressed.len() >= data.len() {
                write_lzh_entry(
                    writer,
                    path,
                    &path_bytes,
                    LZH_METHOD_STORE,
                    original_size,
                    original_size,
                    crc,
                    false,
                    &data,
                )
            } else {
                let compressed_size = u32::try_from(compressed.len()).map_err(|_| {
                    GeeZipError::format(
                        format!(
                            "LZH compressed entry '{}' too large ({} bytes)",
                            path.display(),
                            compressed.len()
                        ),
                        ArchiveFormat::Lzh,
                    )
                })?;
                write_lzh_entry(
                    writer,
                    path,
                    &path_bytes,
                    self.method.method_bytes(),
                    compressed_size,
                    original_size,
                    crc,
                    false,
                    &compressed,
                )
            }
        }
    }
    fn add_directory(&mut self, path: &Path) -> GeeZipResult<()> {
        let writer = self.inner.as_mut().ok_or_else(|| {
            GeeZipError::format(
                "LZH writer not initialised (already consumed)",
                ArchiveFormat::Lzh,
            )
        })?;
        let path_bytes = normalize_lzh_writer_path(path, true)?;
        write_lzh_entry(
            writer,
            path,
            &path_bytes,
            LZH_METHOD_DIRECTORY,
            0,
            0,
            0,
            true,
            &[],
        )
    }

    fn finish(self: Box<Self>) -> GeeZipResult<u64> {
        let (bytes, _writer) = (*self).finalize()?;
        Ok(bytes)
    }
}
