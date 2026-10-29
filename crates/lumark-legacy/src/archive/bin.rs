//! Single-stream BIN (raw binary) pass-through helpers.
//!
//! BIN is the simplest possible format 鈥?zero transformation, just a
//! byte-for-byte copy.  Both "compress" and "decompress" are identity
//! operations implemented as `std::io::copy`.

use std::io::{Read, Write};

use crate::config::CompressOptions;
use crate::error::{GeeZipError, GeeZipResult};

/// "Compress" (pass through) 鈥?BIN is raw bytes, no transformation.
///
/// Returns the number of bytes read from the source.
pub fn bin_compress<R: Read, W: Write>(reader: &mut R, mut writer: W) -> GeeZipResult<u64> {
    std::io::copy(reader, &mut writer).map_err(|e| GeeZipError::io(e, "BIN pass-through failed"))
}

/// Compress with options 鈥?provided for dispatch consistency.
///
/// `options` is accepted but ignored (there is nothing to configure for a
/// pass-through format).  Delegates to [`bin_compress`].
pub fn bin_compress_with_options<R: Read, W: Write>(
    reader: &mut R,
    writer: W,
    _options: CompressOptions,
) -> GeeZipResult<u64> {
    bin_compress(reader, writer)
}

/// "Decompress" (pass through) 鈥?BIN is raw bytes, no transformation.
///
/// Returns the number of bytes written to the output.
pub fn bin_decompress<R: Read, W: Write>(reader: &mut R, mut writer: W) -> GeeZipResult<u64> {
    std::io::copy(reader, &mut writer).map_err(|e| GeeZipError::io(e, "BIN pass-through failed"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bin_roundtrip_basic() {
        let original = b"Hello, GeeZipX! Raw binary pass-through test.";
        let mut source = Cursor::new(original.as_slice());

        // "Compress" 鈥?just copy
        let compressed = {
            let mut buf = Vec::new();
            bin_compress(&mut source, &mut buf).unwrap();
            buf
        };

        assert_eq!(
            compressed, original,
            "BIN pass-through must be byte-identical"
        );

        // "Decompress" 鈥?just copy
        let mut decompressed = Vec::new();
        let mut compressed_reader = Cursor::new(compressed.as_slice());
        let bytes = bin_decompress(&mut compressed_reader, &mut decompressed).unwrap();

        assert_eq!(bytes, original.len() as u64);
        assert_eq!(decompressed, original);
    }

    #[test]
    fn bin_roundtrip_empty() {
        let mut source = Cursor::new(b"");

        let compressed = {
            let mut buf = Vec::new();
            bin_compress(&mut source, &mut buf).unwrap();
            buf
        };
        assert!(compressed.is_empty());

        let mut decompressed = Vec::new();
        let mut reader = Cursor::new(compressed.as_slice());
        let bytes = bin_decompress(&mut reader, &mut decompressed).unwrap();
        assert_eq!(bytes, 0);
        assert!(decompressed.is_empty());
    }

    #[test]
    fn bin_roundtrip_large() {
        // 1 MB of repeating data
        let original = vec![0xABu8; 1_048_576];
        let mut source = Cursor::new(original.as_slice());

        let compressed = {
            let mut buf = Vec::new();
            bin_compress(&mut source, &mut buf).unwrap();
            buf
        };

        assert_eq!(
            compressed.len(),
            original.len(),
            "BIN pass-through produces identical size for large data"
        );

        let mut decompressed = Vec::new();
        let mut reader = Cursor::new(compressed.as_slice());
        let bytes = bin_decompress(&mut reader, &mut decompressed).unwrap();

        assert_eq!(bytes, original.len() as u64);
        assert_eq!(decompressed, original);
    }

    #[test]
    fn bin_compress_with_options_ignores_level() {
        let original = b"pass-through with options";
        let mut source = Cursor::new(original.as_slice());

        let compressed = {
            let mut buf = Vec::new();
            bin_compress_with_options(
                &mut source,
                &mut buf,
                CompressOptions {
                    level: Some(9),
                    jobs: None,
                    password: None,
                    ..Default::default()
                },
            )
            .unwrap();
            buf
        };

        assert_eq!(compressed, original);
    }
}
