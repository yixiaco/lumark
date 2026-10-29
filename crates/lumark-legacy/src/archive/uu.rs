//! UU/UUE decode helpers (single-stream).

use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use crate::error::{GeeZipError, GeeZipResult};

/// Encode raw binary data into a UU-encoded body (without header/footer).
///
/// Data is split into 45-byte lines, each prefixed with a length
/// character.  This conforms to the standard UUencoding format used by
/// `uu_decode`.
pub fn uu_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(45) {
        // Length byte: chunk.len() + 32 (printable ASCII range)
        out.push(((chunk.len() as u8).saturating_add(32)) as char);
        for triple in chunk.chunks(3) {
            let a = triple.first().copied().unwrap_or(0) as u32;
            let b = triple.get(1).copied().unwrap_or(0) as u32;
            let c = triple.get(2).copied().unwrap_or(0) as u32;
            let val = (a << 16) | (b << 8) | c;
            for shift in [18u32, 12, 6, 0] {
                let six = ((val >> shift) & 0x3F) as u8;
                out.push((six + 32) as char);
            }
        }
        out.push('\n');
    }
    out
}

/// Encode data from `reader` into a full UU-encoded stream (with
/// `begin` / `end` headers) and write it to `writer`.
pub fn uu_encode_to_writer<R: Read, W: Write>(
    reader: &mut R,
    name: &str,
    writer: &mut W,
) -> GeeZipResult<u64> {
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| GeeZipError::io(e, "reading input for uu encoding"))?;
    let body = uu_encode(&data);
    let output = format!("begin 644 {}\n{}end\n", name, body.trim_end());
    writer
        .write_all(output.as_bytes())
        .map_err(|e| GeeZipError::io(e, "writing uu-encoded output"))?;
    Ok(output.len() as u64)
}

pub fn uu_decode_file(path: &Path) -> GeeZipResult<(String, Vec<u8>)> {
    let content = fs::read_to_string(path)
        .map_err(|e| GeeZipError::io(e, format!("reading '{}'", path.display())))?;
    uu_decode(&content).ok_or_else(|| {
        GeeZipError::format(
            format!("uuencode file '{}' is empty or malformed", path.display()),
            crate::detect::ArchiveFormat::Uu,
        )
    })
}

pub fn uu_decode(input: &str) -> Option<(String, Vec<u8>)> {
    let mut lines = input.lines();
    let header = lines.next()?;
    let header = header.strip_prefix("begin ")?;
    let filename = header
        .split_whitespace()
        .nth(1)
        .unwrap_or("unknown")
        .to_string();

    let mut output = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line == "end" || line.is_empty() {
            break;
        }
        let bytes = line.as_bytes();
        if bytes.is_empty() {
            continue;
        }
        let encoded_len = bytes[0];
        if encoded_len < 33 {
            break; // ` = 96 marks an empty line end
        }
        let mut remaining = (encoded_len - 32) as usize;
        if remaining == 0 {
            continue;
        }
        let encoded = &bytes[1..];
        let mut i = 0;
        while i + 3 < encoded.len() && remaining > 0 {
            let a = (encoded[i].wrapping_sub(32)) & 0x3F;
            let b = (encoded[i + 1].wrapping_sub(32)) & 0x3F;
            let c = (encoded[i + 2].wrapping_sub(32)) & 0x3F;
            let d = (encoded[i + 3].wrapping_sub(32)) & 0x3F;
            let triple = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
            if remaining > 0 {
                output.push(((triple >> 16) & 0xFF) as u8);
                remaining -= 1;
            }
            if remaining > 0 {
                output.push(((triple >> 8) & 0xFF) as u8);
                remaining -= 1;
            }
            if remaining > 0 {
                output.push((triple & 0xFF) as u8);
                remaining -= 1;
            }
            i += 4;
        }
    }
    if filename.is_empty() && output.is_empty() {
        return None;
    }
    Some((filename, output))
}

pub fn uu_decode_to_writer(path: &Path, writer: &mut dyn Write) -> GeeZipResult<u64> {
    let (_filename, data) = uu_decode_file(path)?;
    let len = data.len() as u64;
    writer.write_all(&data).map_err(|e| {
        GeeZipError::io(
            e,
            format!("writing decoded output for '{}'", path.display()),
        )
    })?;
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uu_decode_roundtrip() {
        for data in &[
            b"cat".as_slice(),
            b"hello uuencode".as_slice(),
            b"".as_slice(),
            b"x".as_slice(),
        ] {
            let encoded = uu_encode(data);
            let input = format!("begin 644 test.bin\n{}\nend\n", encoded);
            if let Some((filename, decoded)) = uu_decode(&input) {
                assert_eq!(filename, "test.bin");
                assert_eq!(decoded, data.to_vec());
            } else if !data.is_empty() {
                panic!("decode failed for {:?}", data);
            }
        }
    }

    #[test]
    fn uu_decode_empty_fails() {
        assert!(uu_decode("").is_none());
    }

    #[test]
    fn uu_decode_file_test() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.uu");
        let data = b"file content";
        let encoded = uu_encode(data);
        let content = format!("begin 644 test.txt\n{}\nend\n", encoded);
        std::fs::write(&path, content).unwrap();
        let (filename, decoded) = uu_decode_file(&path).unwrap();
        assert_eq!(filename, "test.txt");
        assert_eq!(decoded, data.to_vec());
    }
}
