//! Lzip (`.lz`) single-stream compression and decompression helpers.
//!
//! Lzip is a container format for LZMA-compressed data with integrity
//! checking (CRC-32 per member). Uses `lzma-rust2` with the `lzip` feature.
//!
//! These functions work on a single byte stream 鈥?they do **not**
//! implement [`ArchiveReader`] or [`ArchiveWriter`] because lzip is a
//! compression format, not an archive container.
//!
//! [`ArchiveReader`]: super::ArchiveReader
//! [`ArchiveWriter`]: super::ArchiveWriter

use std::io::{Read, Write};

use lzma_rust2::{LzipOptions, LzipReader, LzipWriter};

use crate::config::CompressOptions;
use crate::error::{GeeZipError, GeeZipResult};

/// Compress data from `reader` into `writer` using lzip at the given level.
///
/// `level` controls the LZMA compression strength:
/// - `None`: use the default level (6).
/// - `Some(0)`: no compression (store only).
/// - `Some(1)`: fastest compression.
/// - `Some(6)`: default (good balance).
/// - `Some(9)`: best compression ratio (slowest).
///
/// Returns the number of bytes read from the source (uncompressed size).
pub fn lz_compress_with_level<R: Read, W: Write>(
    reader: &mut R,
    writer: W,
    level: Option<u32>,
) -> GeeZipResult<u64> {
    let preset = level.unwrap_or(6);
    let options = LzipOptions::with_preset(preset);
    let mut encoder = LzipWriter::new(writer, options);
    let bytes = std::io::copy(reader, &mut encoder)
        .map_err(|e| GeeZipError::io(e, "lzip compression failed"))?;
    encoder.finish().map_err(|e| {
        GeeZipError::io(
            std::io::Error::other(e),
            "lzip compression finalisation failed",
        )
    })?;
    Ok(bytes)
}

/// Compress data from `reader` into `writer` using lzip with full options.
///
/// Currently only `options.level` is applied.
///
/// Returns the number of bytes read from the source (uncompressed size).
pub fn lz_compress_with_options<R: Read, W: Write>(
    reader: &mut R,
    writer: W,
    options: &CompressOptions,
) -> GeeZipResult<u64> {
    lz_compress_with_level(reader, writer, options.level)
}

/// Compress data from `reader` into `writer` using lzip with the default level.
///
/// This is a convenience wrapper around [`lz_compress_with_level`] with
/// `level: None` (default compression).
///
/// Returns the number of bytes read from the source (uncompressed size).
pub fn lz_compress<R: Read, W: Write>(reader: &mut R, writer: W) -> GeeZipResult<u64> {
    lz_compress_with_level(reader, writer, None)
}

/// Decompress lzip-compressed data from `reader` into `writer`.
///
/// Returns the number of bytes written to the output (decompressed size).
pub fn lz_decompress<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> GeeZipResult<u64> {
    let mut decoder = LzipReader::new(reader);
    let bytes = std::io::copy(&mut decoder, writer)
        .map_err(|e| GeeZipError::io(e, "lzip decompression failed"))?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn lz_roundtrip() {
        let original = b"Hello, GeeZipX! This is a test of lzip compression.";
        let mut source = Cursor::new(original.as_slice());

        let compressed = {
            let mut buf = Vec::new();
            lz_compress(&mut source, &mut buf).unwrap();
            buf
        };

        assert!(
            !compressed.is_empty(),
            "compressed output should not be empty"
        );
        // First 4 bytes should be LZIP magic
        assert_eq!(&compressed[..4], b"LZIP", "LZIP magic expected");

        // Decompress
        let mut decompressed = Vec::new();
        let mut compressed_reader = Cursor::new(compressed.as_slice());
        let bytes = lz_decompress(&mut compressed_reader, &mut decompressed).unwrap();

        assert_eq!(bytes, original.len() as u64);
        assert_eq!(decompressed, original);
    }

    #[test]
    fn lz_empty_data() {
        let mut source = Cursor::new(b"");
        let compressed = {
            let mut buf = Vec::new();
            lz_compress(&mut source, &mut buf).unwrap();
            buf
        };

        assert!(
            !compressed.is_empty(),
            "empty data should still produce lzip stream"
        );

        let mut decompressed = Vec::new();
        let mut compressed_reader = Cursor::new(compressed.as_slice());
        let bytes = lz_decompress(&mut compressed_reader, &mut decompressed).unwrap();

        assert_eq!(bytes, 0);
        assert!(decompressed.is_empty());
    }

    #[test]
    fn lz_corrupted_data_fails() {
        let bad_data = b"this is not lzip data at all!";
        let mut reader = Cursor::new(bad_data.as_slice());
        let mut output = Vec::new();

        match lz_decompress(&mut reader, &mut output) {
            Ok(0) => {} // Some decoders may return 0 bytes for invalid input
            Ok(n) => panic!("expected error or 0 bytes, got {n} bytes"),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                assert!(
                    msg.contains("lzip") || msg.contains("io") || msg.contains("invalid"),
                    "expected lzip/io error, got: {e}"
                );
            }
        }
    }

    #[test]
    fn lz_large_data() {
        // 1 MB of repeating data
        let original = vec![0xABu8; 1_048_576];
        let mut source = Cursor::new(original.as_slice());

        let compressed = {
            let mut buf = Vec::new();
            lz_compress(&mut source, &mut buf).unwrap();
            buf
        };

        assert!(
            compressed.len() < original.len(),
            "compressed size ({}) should be less than original ({}) for repetitive data",
            compressed.len(),
            original.len()
        );

        let mut decompressed = Vec::new();
        let mut compressed_reader = Cursor::new(compressed.as_slice());
        let bytes = lz_decompress(&mut compressed_reader, &mut decompressed).unwrap();

        assert_eq!(bytes, original.len() as u64);
        assert_eq!(decompressed, original);
    }

    #[test]
    fn lz_with_level_9() {
        let original = b"Hello, GeeZipX! Level 9 compression test data.";
        let mut source = Cursor::new(original.as_slice());
        let compressed = {
            let mut buf = Vec::new();
            lz_compress_with_level(&mut source, &mut buf, Some(9)).unwrap();
            buf
        };

        assert!(!compressed.is_empty());
        assert_eq!(&compressed[..4], b"LZIP", "LZIP magic expected");

        let mut decompressed = Vec::new();
        let mut compressed_reader = Cursor::new(compressed.as_slice());
        lz_decompress(&mut compressed_reader, &mut decompressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn lz_with_level_0_store() {
        let original = b"Hello, GeeZipX! Level 0 (store) test.";
        let mut source = Cursor::new(original.as_slice());
        let compressed = {
            let mut buf = Vec::new();
            lz_compress_with_level(&mut source, &mut buf, Some(0)).unwrap();
            buf
        };

        assert!(!compressed.is_empty());
        assert_eq!(&compressed[..4], b"LZIP", "LZIP magic expected");

        let mut decompressed = Vec::new();
        let mut compressed_reader = Cursor::new(compressed.as_slice());
        lz_decompress(&mut compressed_reader, &mut decompressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn lz_level_none_falls_back_to_default() {
        let original = b"GeeZipX default level test.";
        let mut source = Cursor::new(original.as_slice());

        let compressed_default = {
            let mut buf = Vec::new();
            lz_compress(&mut source, &mut buf).unwrap();
            buf
        };

        source.set_position(0);
        let compressed_with_level = {
            let mut buf = Vec::new();
            lz_compress_with_level(&mut source, &mut buf, None).unwrap();
            buf
        };

        assert!(!compressed_default.is_empty());
        assert!(!compressed_with_level.is_empty());

        let mut out1 = Vec::new();
        let mut reader1 = Cursor::new(compressed_default.as_slice());
        lz_decompress(&mut reader1, &mut out1).unwrap();
        assert_eq!(out1, original);

        let mut out2 = Vec::new();
        let mut reader2 = Cursor::new(compressed_with_level.as_slice());
        lz_decompress(&mut reader2, &mut out2).unwrap();
        assert_eq!(out2, original);
    }
}
