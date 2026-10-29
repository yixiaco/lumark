//! Debian package (`.deb`) archive reader and writer.
//!
//! GeeZipX supports both reading and writing DEB archives.  The DEB format
//! wraps an outer Unix `ar` archive whose useful payload lives in a single
//! `data.tar*` member.  When reading, `debian-binary` and `control.tar.*` are
//! intentionally ignored so the behavior matches `dpkg-deb -x/-c` style
//! payload extraction.  When writing, a minimal `control.tar.gz` is generated
//! automatically.

use std::fmt;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::archive::{ArchiveReader, ArchiveWriter, CountWriter, Entry, is_entry_path_dangerous};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// Read-only Debian package reader.
pub struct DebReader<R: Read + Seek + Send> {
    inner: R,
    format: ArchiveFormat,
}

impl<R: Read + Seek + Send> fmt::Debug for DebReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DebReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek + Send> DebReader<R> {
    /// Create a DEB reader from any `Read + Seek + Send` source.
    pub fn new(reader: R) -> Self {
        DebReader {
            inner: reader,
            format: ArchiveFormat::Deb,
        }
    }
}

impl DebReader<std::io::Cursor<Vec<u8>>> {
    /// Create a DEB reader from an already-loaded byte buffer.
    pub fn from_buf(buf: Vec<u8>) -> Self {
        DebReader::new(std::io::Cursor::new(buf))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataPayloadCodec {
    Tar,
    #[default]
    Gzip,
    Xz,
    Zstd,
    Bzip2,
    Lzma,
}

impl DataPayloadCodec {
    fn member_name(self) -> &'static str {
        match self {
            DataPayloadCodec::Tar => "data.tar",
            DataPayloadCodec::Gzip => "data.tar.gz",
            DataPayloadCodec::Xz => "data.tar.xz",
            DataPayloadCodec::Zstd => "data.tar.zst",
            DataPayloadCodec::Bzip2 => "data.tar.bz2",
            DataPayloadCodec::Lzma => "data.tar.lzma",
        }
    }
}

enum DataMemberKind {
    Supported(DataPayloadCodec),
    Unsupported(String),
    Ignore,
}

fn classify_data_member(name: &str) -> DataMemberKind {
    match name {
        "data.tar" => DataMemberKind::Supported(DataPayloadCodec::Tar),
        "data.tar.gz" => DataMemberKind::Supported(DataPayloadCodec::Gzip),
        "data.tar.xz" => DataMemberKind::Supported(DataPayloadCodec::Xz),
        "data.tar.zst" => DataMemberKind::Supported(DataPayloadCodec::Zstd),
        "data.tar.bz2" => DataMemberKind::Supported(DataPayloadCodec::Bzip2),
        "data.tar.lzma" => DataMemberKind::Supported(DataPayloadCodec::Lzma),
        other if other.starts_with("data.tar") => DataMemberKind::Unsupported(other.to_owned()),
        _ => DataMemberKind::Ignore,
    }
}

fn normalize_ar_identifier(raw: &[u8]) -> GeeZipResult<String> {
    let name = std::str::from_utf8(raw).map_err(|e| {
        GeeZipError::format(
            format!("non-UTF-8 DEB ar member name: {e}"),
            ArchiveFormat::Deb,
        )
    })?;
    Ok(name.trim_end().trim_end_matches('/').to_owned())
}

fn unsupported_data_payload(name: &str) -> GeeZipError {
    GeeZipError::format(
        format!(
            "unsupported DEB data payload '{name}' (supported: data.tar, data.tar.gz, data.tar.xz, data.tar.zst, data.tar.bz2, data.tar.lzma)"
        ),
        ArchiveFormat::Deb,
    )
}

fn convert_deb_ar_error(err: std::io::Error) -> GeeZipError {
    match err.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
            GeeZipError::format(format!("invalid DEB ar archive: {err}"), ArchiveFormat::Deb)
        }
        _ => GeeZipError::io(err, "reading DEB ar archive"),
    }
}

fn convert_deb_payload_error(err: std::io::Error, member: &str) -> GeeZipError {
    match err.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => GeeZipError::format(
            format!("invalid DEB payload '{member}': {err}"),
            ArchiveFormat::Deb,
        ),
        _ => GeeZipError::io(err, format!("reading DEB payload '{member}'")),
    }
}

fn collect_deb_tar_entries<R: Read>(
    member: &str,
    archive: &mut tar::Archive<R>,
) -> GeeZipResult<Vec<Entry>> {
    let mut entries = Vec::new();

    for result in archive
        .entries()
        .map_err(|e| convert_deb_payload_error(e, member))?
    {
        let tar_entry = result.map_err(|e| convert_deb_payload_error(e, member))?;
        let header = tar_entry.header();
        let entry_type = header.entry_type();
        if matches!(
            entry_type,
            tar::EntryType::XGlobalHeader
                | tar::EntryType::GNULongLink
                | tar::EntryType::GNULongName
        ) {
            continue;
        }

        let path = tar_entry
            .path()
            .map_err(|e| convert_deb_payload_error(e, member))?
            .to_string_lossy()
            .into_owned();
        let size = tar_entry.size();
        let is_dir = entry_type.is_dir();

        entries.push(Entry {
            path,
            size,
            compressed_size: 0,
            crc32: None,
            modified: header.mtime().ok(),
            is_dir,
        });
    }

    Ok(entries)
}

fn extract_deb_tar_entry<R: Read>(
    member: &str,
    archive: &mut tar::Archive<R>,
    entry: &Entry,
    writer: &mut dyn Write,
) -> GeeZipResult<u64> {
    for result in archive
        .entries()
        .map_err(|e| convert_deb_payload_error(e, member))?
    {
        let mut tar_entry = result.map_err(|e| convert_deb_payload_error(e, member))?;
        let path = tar_entry
            .path()
            .map_err(|e| convert_deb_payload_error(e, member))?
            .to_string_lossy()
            .into_owned();

        if path == entry.path {
            if tar_entry.header().entry_type().is_dir() {
                return Ok(0);
            }
            let bytes = std::io::copy(&mut tar_entry, writer).map_err(|e| {
                GeeZipError::io(
                    e,
                    format!("extracting '{}' from DEB payload '{member}'", entry.path),
                )
            })?;
            return Ok(bytes);
        }
    }

    Err(GeeZipError::EntryNotFound {
        name: entry.path.clone(),
    })
}

fn collect_entries_from_member<R: Read>(
    member: ar::Entry<R>,
    codec: DataPayloadCodec,
) -> GeeZipResult<Vec<Entry>> {
    let member_name = codec.member_name();
    match codec {
        DataPayloadCodec::Tar => {
            let mut archive = tar::Archive::new(member);
            collect_deb_tar_entries(member_name, &mut archive)
        }
        DataPayloadCodec::Gzip => {
            let decoder = flate2::read::MultiGzDecoder::new(member);
            let mut archive = tar::Archive::new(decoder);
            collect_deb_tar_entries(member_name, &mut archive)
        }
        DataPayloadCodec::Xz => {
            let decoder = xz2::read::XzDecoder::new_multi_decoder(member);
            let mut archive = tar::Archive::new(decoder);
            collect_deb_tar_entries(member_name, &mut archive)
        }
        DataPayloadCodec::Zstd => {
            let decoder = zstd::stream::read::Decoder::new(member).map_err(|e| {
                GeeZipError::io(
                    e,
                    format!("initializing DEB payload '{member_name}' decoder"),
                )
            })?;
            let mut archive = tar::Archive::new(decoder);
            collect_deb_tar_entries(member_name, &mut archive)
        }
        DataPayloadCodec::Bzip2 => {
            let decoder = ::bzip2::read::MultiBzDecoder::new(member);
            let mut archive = tar::Archive::new(decoder);
            collect_deb_tar_entries(member_name, &mut archive)
        }
        DataPayloadCodec::Lzma => {
            let stream = xz2::stream::Stream::new_lzma_decoder(u64::MAX).map_err(|e| {
                GeeZipError::io(
                    e.into(),
                    format!("initializing DEB payload '{member_name}' decoder"),
                )
            })?;
            let decoder = xz2::read::XzDecoder::new_stream(member, stream);
            let mut archive = tar::Archive::new(decoder);
            collect_deb_tar_entries(member_name, &mut archive)
        }
    }
}

fn extract_from_member<R: Read>(
    member: ar::Entry<R>,
    codec: DataPayloadCodec,
    entry: &Entry,
    writer: &mut dyn Write,
) -> GeeZipResult<u64> {
    let member_name = codec.member_name();
    match codec {
        DataPayloadCodec::Tar => {
            let mut archive = tar::Archive::new(member);
            extract_deb_tar_entry(member_name, &mut archive, entry, writer)
        }
        DataPayloadCodec::Gzip => {
            let decoder = flate2::read::MultiGzDecoder::new(member);
            let mut archive = tar::Archive::new(decoder);
            extract_deb_tar_entry(member_name, &mut archive, entry, writer)
        }
        DataPayloadCodec::Xz => {
            let decoder = xz2::read::XzDecoder::new_multi_decoder(member);
            let mut archive = tar::Archive::new(decoder);
            extract_deb_tar_entry(member_name, &mut archive, entry, writer)
        }
        DataPayloadCodec::Zstd => {
            let decoder = zstd::stream::read::Decoder::new(member).map_err(|e| {
                GeeZipError::io(
                    e,
                    format!("initializing DEB payload '{member_name}' decoder"),
                )
            })?;
            let mut archive = tar::Archive::new(decoder);
            extract_deb_tar_entry(member_name, &mut archive, entry, writer)
        }
        DataPayloadCodec::Bzip2 => {
            let decoder = ::bzip2::read::MultiBzDecoder::new(member);
            let mut archive = tar::Archive::new(decoder);
            extract_deb_tar_entry(member_name, &mut archive, entry, writer)
        }
        DataPayloadCodec::Lzma => {
            let stream = xz2::stream::Stream::new_lzma_decoder(u64::MAX).map_err(|e| {
                GeeZipError::io(
                    e.into(),
                    format!("initializing DEB payload '{member_name}' decoder"),
                )
            })?;
            let decoder = xz2::read::XzDecoder::new_stream(member, stream);
            let mut archive = tar::Archive::new(decoder);
            extract_deb_tar_entry(member_name, &mut archive, entry, writer)
        }
    }
}

impl<R: Read + Seek + Send> ArchiveReader for DebReader<R> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        self.inner.seek(SeekFrom::Start(0))?;
        let mut archive = ar::Archive::new(&mut self.inner);
        let mut unsupported = None;

        while let Some(result) = archive.next_entry() {
            let entry = result.map_err(convert_deb_ar_error)?;
            let member_name = normalize_ar_identifier(entry.header().identifier())?;
            match classify_data_member(&member_name) {
                DataMemberKind::Supported(codec) => {
                    return collect_entries_from_member(entry, codec);
                }
                DataMemberKind::Unsupported(name) => {
                    if unsupported.is_none() {
                        unsupported = Some(name);
                    }
                }
                DataMemberKind::Ignore => {}
            }
        }

        if let Some(name) = unsupported {
            Err(unsupported_data_payload(&name))
        } else {
            Ok(Vec::new())
        }
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        self.inner.seek(SeekFrom::Start(0))?;
        let mut archive = ar::Archive::new(&mut self.inner);
        let mut unsupported = None;

        while let Some(result) = archive.next_entry() {
            let member = result.map_err(convert_deb_ar_error)?;
            let member_name = normalize_ar_identifier(member.header().identifier())?;
            match classify_data_member(&member_name) {
                DataMemberKind::Supported(codec) => {
                    return extract_from_member(member, codec, entry, writer);
                }
                DataMemberKind::Unsupported(name) => {
                    if unsupported.is_none() {
                        unsupported = Some(name);
                    }
                }
                DataMemberKind::Ignore => {}
            }
        }

        if let Some(name) = unsupported {
            Err(unsupported_data_payload(&name))
        } else {
            Err(GeeZipError::EntryNotFound {
                name: entry.path.clone(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// DebWriter
// ---------------------------------------------------------------------------

/// Debian package (`.deb`) archive writer.
///
/// Creates a DEB archive consisting of a `debian-binary` version marker,
/// a minimal `control.tar.gz`, and a `data.tar.{ext}` payload containing
/// the user-specified entries.  The data payload compression can be
/// controlled via [`DataPayloadCodec`]; the default is Gzip.
pub struct DebWriter<W: Write + Send> {
    writer: Option<W>,
    codec: DataPayloadCodec,
    format: ArchiveFormat,
    entries: Vec<(String, Vec<u8>)>,
    directories: Vec<String>,
}

impl<W: Write + Send> fmt::Debug for DebWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DebWriter")
            .field("format", &self.format)
            .field("codec", &self.codec)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Send> DebWriter<W> {
    /// Create a new DEB writer with default Gzip data payload compression.
    pub fn new(writer: W) -> Self {
        DebWriter {
            writer: Some(writer),
            codec: DataPayloadCodec::Gzip,
            format: ArchiveFormat::Deb,
            entries: Vec::new(),
            directories: Vec::new(),
        }
    }

    /// Create a new DEB writer with the given data payload compression codec.
    pub fn new_with_codec(writer: W, codec: DataPayloadCodec) -> Self {
        DebWriter {
            writer: Some(writer),
            codec,
            format: ArchiveFormat::Deb,
            entries: Vec::new(),
            directories: Vec::new(),
        }
    }
}

impl<W: Write + Send> ArchiveWriter for DebWriter<W> {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn add_entry_from_reader(&mut self, path: &Path, reader: &mut dyn Read) -> GeeZipResult<()> {
        let name = validate_deb_write_path(path)?;
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(|e| GeeZipError::io(e, format!("reading data for DEB entry '{name}'")))?;
        self.entries.push((name, data));
        Ok(())
    }

    fn add_directory(&mut self, path: &Path) -> GeeZipResult<()> {
        let name = validate_deb_write_path(path)?;
        self.directories.push(name);
        Ok(())
    }

    fn finish(self: Box<Self>) -> GeeZipResult<u64> {
        let writer = self.writer.ok_or_else(|| GeeZipError::Format {
            message: "DEB writer not initialised (already consumed)".into(),
            format: ArchiveFormat::Deb,
        })?;

        // Build the control.tar.gz payload.
        let control_tar_gz = build_control_tar()?;

        // Build the data.tar.{codec} payload.
        let data_tar = build_data_tar(&self.entries, &self.directories)?;
        let data_compressed = compress_tar(self.codec, &data_tar)?;

        // Write the ar archive through a CountWriter.
        let mut counter = CountWriter {
            inner: writer,
            count: 0,
        };

        let member_name = self.codec.member_name();
        {
            let mut builder = ar::Builder::new(&mut counter);

            // debian-binary
            let header = ar::Header::new(b"debian-binary".to_vec(), 4);
            builder
                .append(&header, &b"2.0\n"[..])
                .map_err(convert_deb_ar_error)?;

            // control.tar.gz
            let header = ar::Header::new(b"control.tar.gz".to_vec(), control_tar_gz.len() as u64);
            builder
                .append(&header, &control_tar_gz[..])
                .map_err(convert_deb_ar_error)?;

            // data.tar.{ext}
            let header = ar::Header::new(
                member_name.as_bytes().to_vec(),
                data_compressed.len() as u64,
            );
            builder
                .append(&header, &data_compressed[..])
                .map_err(convert_deb_ar_error)?;
        }

        Ok(counter.count)
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Minimal `control` file content for generated DEB packages.
const MINIMAL_CONTROL: &str = "\
Package: geezipx-package
Version: 1.0
Architecture: all
Maintainer: GeeZipX <geezipx@example.com>
Description: Package created by GeeZipX
";
// NOTE: trailing newline above is required by Debian policy.

/// Build a minimal control.tar.gz as a byte vector.
fn build_control_tar() -> GeeZipResult<Vec<u8>> {
    let tar_buf = Vec::new();
    let mut tar_builder = tar::Builder::new(tar_buf);
    let mut header = tar::Header::new_gnu();
    header
        .set_path("./control")
        .map_err(|e| GeeZipError::Format {
            message: format!("setting control tar path: {e}"),
            format: ArchiveFormat::Deb,
        })?;
    header.set_size(MINIMAL_CONTROL.len() as u64);
    header.set_cksum();
    tar_builder
        .append(&header, std::io::Cursor::new(MINIMAL_CONTROL))
        .map_err(|e| GeeZipError::io(e, "building control.tar for DEB writer"))?;
    let tar_bytes = tar_builder
        .into_inner()
        .map_err(|e| GeeZipError::io(e, "finalising control.tar for DEB writer"))?;

    // gzip compress
    let mut out = Vec::new();
    let mut encoder = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
    encoder
        .write_all(&tar_bytes)
        .map_err(|e| GeeZipError::io(e, "gzipping control.tar for DEB writer"))?;
    encoder
        .finish()
        .map_err(|e| GeeZipError::io(e, "finishing control.tar.gz for DEB writer"))?;
    Ok(out)
}

/// Build the uncompressed data.tar as a byte vector.
fn build_data_tar(entries: &[(String, Vec<u8>)], directories: &[String]) -> GeeZipResult<Vec<u8>> {
    let tar_buf = Vec::new();
    let mut tar_builder = tar::Builder::new(tar_buf);

    // Directories first.
    for dir in directories {
        let mut header = tar::Header::new_gnu();
        let dir_path = format!("{dir}/");
        header
            .set_path(&dir_path)
            .map_err(|e| GeeZipError::Format {
                message: format!("setting dir path for '{dir_path}': {e}"),
                format: ArchiveFormat::Deb,
            })?;
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_cksum();
        tar_builder
            .append_data(&mut header, &dir_path, std::io::empty())
            .map_err(|e| GeeZipError::io(e, format!("writing dir entry '{dir_path}'")))?;
    }

    // File entries.
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_path(name).map_err(|e| GeeZipError::Format {
            message: format!("setting path for '{name}': {e}"),
            format: ArchiveFormat::Deb,
        })?;
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar_builder
            .append_data(&mut header, name, std::io::Cursor::new(data))
            .map_err(|e| GeeZipError::io(e, format!("writing data.tar entry '{name}'")))?;
    }

    tar_builder
        .into_inner()
        .map_err(|e| GeeZipError::io(e, "finalising data.tar for DEB writer"))
}

/// Compress raw tar bytes using the chosen codec.
fn compress_tar(codec: DataPayloadCodec, data: &[u8]) -> GeeZipResult<Vec<u8>> {
    match codec {
        DataPayloadCodec::Tar => Ok(data.to_vec()),
        DataPayloadCodec::Gzip => {
            let mut out = Vec::new();
            let mut encoder =
                flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
            encoder
                .write_all(data)
                .map_err(|e| GeeZipError::io(e, "gzipping data.tar for DEB writer"))?;
            encoder
                .finish()
                .map_err(|e| GeeZipError::io(e, "finishing data.tar.gz for DEB writer"))?;
            Ok(out)
        }
        DataPayloadCodec::Xz => {
            let mut out = Vec::new();
            let mut reader = std::io::Cursor::new(data);
            crate::archive::xz::xz_compress(&mut reader, &mut out).map_err(|e| {
                GeeZipError::Format {
                    message: format!("xz compressing data.tar: {e}"),
                    format: ArchiveFormat::Deb,
                }
            })?;
            Ok(out)
        }
        DataPayloadCodec::Zstd => {
            let mut out = Vec::new();
            let mut encoder = zstd::stream::write::Encoder::new(&mut out, 0)
                .map_err(|e| GeeZipError::io(e, "initialising zstd encoder for data.tar"))?;
            encoder
                .write_all(data)
                .map_err(|e| GeeZipError::io(e, "zstd compressing data.tar for DEB writer"))?;
            encoder
                .finish()
                .map_err(|e| GeeZipError::io(e, "finishing data.tar.zst for DEB writer"))?;
            Ok(out)
        }
        DataPayloadCodec::Bzip2 => {
            let mut out = Vec::new();
            let mut encoder =
                ::bzip2::write::BzEncoder::new(&mut out, ::bzip2::Compression::default());
            encoder
                .write_all(data)
                .map_err(|e| GeeZipError::io(e, "bzip2 compressing data.tar for DEB writer"))?;
            encoder
                .finish()
                .map_err(|e| GeeZipError::io(e, "finishing data.tar.bz2 for DEB writer"))?;
            Ok(out)
        }
        DataPayloadCodec::Lzma => {
            let mut out = Vec::new();
            let mut reader = std::io::Cursor::new(data);
            crate::archive::xz::lzma_compress(&mut reader, &mut out).map_err(|e| {
                GeeZipError::Format {
                    message: format!("lzma compressing data.tar: {e}"),
                    format: ArchiveFormat::Deb,
                }
            })?;
            Ok(out)
        }
    }
}

/// Validate a path for DEB write. Rejects paths that would escape the
/// archive root (absolute, `..`, Windows drive/UNC).
fn validate_deb_write_path(path: &Path) -> GeeZipResult<String> {
    if is_entry_path_dangerous(path) {
        return Err(GeeZipError::PathTraversal {
            entry: path.to_string_lossy().into_owned(),
            target: "DEB archive root".into(),
        });
    }
    Ok(path.to_string_lossy().into_owned())
}
