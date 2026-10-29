//! Format detection from magic bytes and file extensions.
//!
//! This module provides lightweight magic-byte matching to identify
//! archive and compression formats without pulling in heavy format
//! libraries.  Tar is recognised via file extension only (no magic).

use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// ArchiveFormat
// ---------------------------------------------------------------------------

/// Supported archive and compression format identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArchiveFormat {
    /// ZIP archive.
    Zip,
    /// GNU tar archive (no magic 鈥?extension-based).
    Tar,
    /// Gzip compressed stream (`\x1F\x8B`).
    Gzip,
    /// Bzip2 compressed stream (`BZh`).
    Bzip2,
    /// Brotli-compressed stream (extension-based 鈥?`.br`; no stable magic header).
    Brotli,
    /// LZ4 frame (`04 22 4D 18`).
    Lz4,
    /// Tar archive compressed with gzip (extension-based 鈥?`.tar.gz`, `.tgz`).
    TarGz,
    /// Tar archive compressed with bzip2 (extension-based 鈥?`.tar.bz2`, `.tbz`, `.tbz2`).
    TarBz2,
    /// Tar archive compressed with Brotli (extension-based 鈥?`.tar.br`).
    TarBr,
    /// Tar archive compressed with LZ4 (extension-based 鈥?`.tar.lz4`).
    TarLz4,
    /// LZMA-compressed XZ stream (`\xFD7zXZ\x00`).
    Xz,
    /// Zstandard frame (`\x28\xB5\x2F\xFD`).
    Zstd,
    /// Tar archive compressed with Zstandard (extension-based 鈥?`.tar.zst`, `.tzst`).
    TarZst,
    /// LZMA Alone compressed stream (no magic 鈥?extension-based).
    Lz,
    /// Lzip compressed stream (`.lz`) 鈥?LZMA-based container with CRC-32 integrity checking.
    Lzma,
    /// Tar archive compressed with XZ (extension-based 鈥?`.tar.xz`, `.txz`).
    TarXz,
    /// 7z archive (`37 7A BC AF 27 1C`).
    SevenZip,
    /// RAR archive (`52 61 72 21 1A 07`).
    Rar,
    /// Microsoft Cabinet archive (`MSCF`).
    Cab,
    /// Electron ASAR archive (extension-based 鈥?`.asar`; no stable magic header).
    Asar,
    /// Debian package archive (extension-based 鈥?`.deb`; deliberately no `ar` magic sniff).
    Deb,
    /// LZH/LHA archive (extension-based 鈥?`.lzh`, `.lha`; deliberately no magic sniff).
    Lzh,
    /// ISO disc image (extension-based 鈥?`.iso`; no useful magic in the first 8 bytes).
    Iso,
    /// CPIO archive (extension-based 鈥?`.cpio`; deliberately no file-level magic sniff).
    Cpio,
    /// ZPAQ archive (extension-based 鈥?`.zpaq`, `.zpq`; deliberately no shallow magic sniff).
    Zpaq,
    /// WIM (Windows Imaging Format) 鈥?extension-based (`.wim`, `.swm`).
    Wim,
    /// ARJ archive (extension-based 鈥?`.arj`; no stable magic header).
    Arj,
    /// ACE archive (extension-based 鈥?`.ace`; no stable magic header).
    Ace,
    /// ARC/PAK archive (extension-based 鈥?`.arc`, `.pak`; no stable magic header).
    Arc,
    /// Unix compress stream (`.Z`) 鈥?LZW compression (extension-based; no stable magic header).
    /// ALZ archive (extension-based 鈥?`.alz`; no stable magic header).
    Alz,
    /// Unix compress stream (`.Z`) 鈥?LZW compression (extension-based; no stable magic header).
    Z,
    /// UDF disc image (`.udf`) 鈥?extension-based; no reliable magic in first 8 bytes.
    Udf,
    /// UU/UUE encoded file (extension-based 鈥?`.uu`, `.uue`).
    Uu,
    /// XXE encoded file (extension-based 鈥?`.xxe`).
    Xxe,
    /// Raw disk image (`.img`/`.ima`) 鈥?identity pass-through, no compression.
    Img,
    /// AES encrypted container (`.enc`) 鈥?AES-256-GCM-SIV with Argon2id KDF.
    /// Extension-only detection (CBOR header precludes magic-byte sniffing).
    Aes,
    /// Raw binary data (`.bin`) 鈥?identity pass-through, no compression.
    Bin,
    /// ISZ compressed ISO image (`.isz`) 鈥?block-compressed ISO wrapper.
    Isz,
    /// Unknown or unrecognised format.
    Unknown,
}

impl fmt::Display for ArchiveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveFormat::Zip => write!(f, "zip"),
            ArchiveFormat::Tar => write!(f, "tar"),
            ArchiveFormat::Gzip => write!(f, "gzip"),
            ArchiveFormat::Bzip2 => write!(f, "bzip2"),
            ArchiveFormat::Brotli => write!(f, "brotli"),
            ArchiveFormat::Lz4 => write!(f, "lz4"),
            ArchiveFormat::TarGz => write!(f, "tar.gz"),
            ArchiveFormat::TarBz2 => write!(f, "tar.bz2"),
            ArchiveFormat::TarBr => write!(f, "tar.br"),
            ArchiveFormat::TarLz4 => write!(f, "tar.lz4"),
            ArchiveFormat::Xz => write!(f, "xz"),
            ArchiveFormat::Zstd => write!(f, "zstd"),
            ArchiveFormat::TarZst => write!(f, "tar.zst"),
            ArchiveFormat::Udf => write!(f, "udf"),
            ArchiveFormat::TarXz => write!(f, "tar.xz"),
            ArchiveFormat::Lzma => write!(f, "lzma"),
            ArchiveFormat::Lz => write!(f, "lz"),
            ArchiveFormat::SevenZip => write!(f, "7z"),
            ArchiveFormat::Rar => write!(f, "rar"),
            ArchiveFormat::Cab => write!(f, "cab"),
            ArchiveFormat::Asar => write!(f, "asar"),
            ArchiveFormat::Deb => write!(f, "deb"),
            ArchiveFormat::Lzh => write!(f, "lzh"),
            ArchiveFormat::Iso => write!(f, "iso"),
            ArchiveFormat::Cpio => write!(f, "cpio"),
            ArchiveFormat::Wim => write!(f, "wim"),
            ArchiveFormat::Zpaq => write!(f, "zpaq"),
            ArchiveFormat::Arj => write!(f, "arj"),
            ArchiveFormat::Ace => write!(f, "ace"),
            ArchiveFormat::Arc => write!(f, "arc"),
            ArchiveFormat::Z => write!(f, "z"),
            ArchiveFormat::Alz => write!(f, "alz"),
            ArchiveFormat::Uu => write!(f, "uu"),
            ArchiveFormat::Xxe => write!(f, "xxe"),
            ArchiveFormat::Img => write!(f, "img"),
            ArchiveFormat::Aes => write!(f, "aes"),
            ArchiveFormat::Bin => write!(f, "bin"),
            ArchiveFormat::Isz => write!(f, "isz"),
            ArchiveFormat::Unknown => write!(f, "unknown"),
        }
    }
}

// ---------------------------------------------------------------------------
// Magic constants
// ---------------------------------------------------------------------------

/// ZIP local file header signature: `PK\x03\x04`.
const MAGIC_ZIP: &[u8] = b"PK\x03\x04";
/// ZIP empty archive (EOCD-only) signature: `PK\x05\x06`.
const MAGIC_ZIP_EMPTY: &[u8] = b"PK\x05\x06";
/// Gzip magic: `\x1F\x8B`.
const MAGIC_GZIP: &[u8] = &[0x1F, 0x8B];
/// Bzip2 magic: `BZh`.
const MAGIC_BZIP2: &[u8] = b"BZh";
/// LZ4 frame magic: `04 22 4D 18`.
const MAGIC_LZ4_FRAME: &[u8] = &[0x04, 0x22, 0x4D, 0x18];
/// Zstandard magic: `\x28\xB5\x2F\xFD`.
const MAGIC_ZSTD: &[u8] = &[0x28, 0xB5, 0x2F, 0xFD];
/// XZ magic: `\xFD7zXZ\x00`.
const MAGIC_XZ: &[u8] = &[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
/// 7z magic: `37 7A BC AF 27 1C`.
pub const MAGIC_SEVENZIP: &[u8] = &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
/// RAR magic: `52 61 72 21 1A 07` (RAR4 and RAR5 share this prefix).
pub const MAGIC_RAR: &[u8] = b"Rar!\x1A\x07";
/// CAB magic: `MSCF`.
pub const MAGIC_CAB: &[u8] = b"MSCF";

/// Map of well-known single extensions to their archive format.
const EXTENSION_MAP: &[(&str, ArchiveFormat)] = &[
    (".zip", ArchiveFormat::Zip),
    (".zipx", ArchiveFormat::Zip),
    (".jar", ArchiveFormat::Zip),
    (".war", ArchiveFormat::Zip),
    (".apk", ArchiveFormat::Zip),
    (".ipa", ArchiveFormat::Zip),
    (".xpi", ArchiveFormat::Zip),
    (".tar", ArchiveFormat::Tar),
    (".gz", ArchiveFormat::Gzip),
    (".bz2", ArchiveFormat::Bzip2),
    (".br", ArchiveFormat::Brotli),
    (".lz4", ArchiveFormat::Lz4),
    // Note: .tgz/.tbz/.tbz2 are handled by detect_from_extension's compound checks.
    (".xz", ArchiveFormat::Xz),
    (".zst", ArchiveFormat::Zstd),
    (".zstd", ArchiveFormat::Zstd),
    (".lzma", ArchiveFormat::Lzma),
    (".lz", ArchiveFormat::Lz),
    (".gzip", ArchiveFormat::Gzip),
    (".7z", ArchiveFormat::SevenZip),
    (".rar", ArchiveFormat::Rar),
    (".cab", ArchiveFormat::Cab),
    (".asar", ArchiveFormat::Asar),
    (".deb", ArchiveFormat::Deb),
    (".lzh", ArchiveFormat::Lzh),
    (".lha", ArchiveFormat::Lzh),
    (".iso", ArchiveFormat::Iso),
    (".cpio", ArchiveFormat::Cpio),
    (".wim", ArchiveFormat::Wim),
    (".swm", ArchiveFormat::Wim),
    (".zpaq", ArchiveFormat::Zpaq),
    (".zpq", ArchiveFormat::Zpaq),
    (".arj", ArchiveFormat::Arj),
    (".ace", ArchiveFormat::Ace),
    (".arc", ArchiveFormat::Arc),
    (".Z", ArchiveFormat::Z),
    (".alz", ArchiveFormat::Alz),
    (".udf", ArchiveFormat::Udf),
    (".img", ArchiveFormat::Img),
    (".ima", ArchiveFormat::Img),
    (".enc", ArchiveFormat::Aes),
    (".bin", ArchiveFormat::Bin),
    (".isz", ArchiveFormat::Isz),
    (".uu", ArchiveFormat::Uu),
    (".uue", ArchiveFormat::Uu),
    (".xxe", ArchiveFormat::Xxe),
];

// ---------------------------------------------------------------------------
// Magic-number-based detection
// ---------------------------------------------------------------------------

/// Detect the archive format from magic bytes.
///
/// Reads the first few bytes of `data` and compares against known magic
/// sequences.  Returns `None` when no magic matches; the caller can
/// fall back to extension-based detection.
///
/// **Note**: For tar-wrapped compressed formats such as `.tar.gz`/`.tgz`,
/// `.tar.bz2`/`.tbz`/`.tbz2`, and `.tar.lz4`, this function returns the
/// outer stream format (`Gzip` / `Bzip2` / `Lz4`) because the compression
/// magic header does not reveal whether the inner stream is a tar archive.
/// Brotli streams have no stable magic header here, so `.br` / `.tar.br`
/// rely on extension-based detection only. DEB deliberately does not sniff the
/// outer `ar` magic because many non-DEB archives share it; `.deb` relies on
/// explicit format / extension. CAB archives do expose an `MSCF` magic header
/// and are recognized here directly. ASAR similarly has no reliable fixed magic
/// header and is handled only by explicit format / extension. ISO volume
/// descriptors live at sector 16, so `.iso` also relies on explicit format /
/// extension here. CPIO deliberately stays extension-only because `newc` / `odc`
/// header magic appears on every entry rather than in a stable file-wide magic
/// prelude, making shallow first-bytes sniffing too eager. ZPAQ likewise stays
/// extension-only because GeeZipX routes it through an optional parser
/// integration rather than a shallow first-bytes recognizer.
pub fn detect_format(data: &[u8]) -> Option<ArchiveFormat> {
    if data.starts_with(MAGIC_ZIP) || data.starts_with(MAGIC_ZIP_EMPTY) {
        return Some(ArchiveFormat::Zip);
    }
    if data.starts_with(MAGIC_GZIP) {
        return Some(ArchiveFormat::Gzip);
    }
    if data.starts_with(MAGIC_BZIP2) {
        return Some(ArchiveFormat::Bzip2);
    }
    if data.starts_with(MAGIC_LZ4_FRAME) {
        return Some(ArchiveFormat::Lz4);
    }
    if data.starts_with(MAGIC_ZSTD) {
        return Some(ArchiveFormat::Zstd);
    }
    if data.starts_with(MAGIC_SEVENZIP) {
        return Some(ArchiveFormat::SevenZip);
    }
    if data.starts_with(MAGIC_RAR) {
        return Some(ArchiveFormat::Rar);
    }
    if data.starts_with(MAGIC_CAB) {
        return Some(ArchiveFormat::Cab);
    }
    if data.starts_with(MAGIC_XZ) {
        return Some(ArchiveFormat::Xz);
    }
    if data.len() >= 4 && &data[0..4] == b"IsZ!" {
        return Some(ArchiveFormat::Isz);
    }
    None
}

// ---------------------------------------------------------------------------
// Extension-based detection
// ---------------------------------------------------------------------------

/// Detect the archive format from a file path's extension.
///
/// This is a best-effort fallback for formats without a magic signature
/// (e.g. tar) and for stream sources where magic detection isn't possible.
///
/// **Compound extensions** (`.tar.gz`, `.tgz`) are checked first so they
/// take priority over the single-extension map.
pub fn detect_from_extension(path: &Path) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();

    // Check compound extensions first (e.g. .tar.gz, .tar.xz).
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz") || lower.ends_with(".tbz2") {
        return Some(ArchiveFormat::TarBz2);
    }
    if lower.ends_with(".tar.br") {
        return Some(ArchiveFormat::TarBr);
    }
    if lower.ends_with(".tar.lz4") {
        return Some(ArchiveFormat::TarLz4);
    }
    if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        return Some(ArchiveFormat::TarXz);
    }
    if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
        return Some(ArchiveFormat::TarZst);
    }

    // Check single extensions.
    for (ext, format) in EXTENSION_MAP {
        if lower.ends_with(ext) {
            return Some(*format);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Number of bytes needed for magic-byte detection of all supported formats.
pub const MAGIC_DETECT_SIZE: usize = 8;

/// Read up to `MAGIC_DETECT_SIZE` bytes from a reader for detection.
///
/// **Note:** this function **consumes** the bytes from the reader; it
/// does NOT peek or un-read them.  If the data needs to be re-read after
/// detection, the caller is responsible for buffering appropriately.
/// If the reader returns fewer than `size` bytes, the buffer is returned
/// as-is.
pub fn read_magic_bytes<R: std::io::Read>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; MAGIC_DETECT_SIZE];
    let n = reader.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
