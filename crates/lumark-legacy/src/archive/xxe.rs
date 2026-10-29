//! XXE decode helpers (single-stream).
//!
//! XXencode uses the alphabet: `+-0123456789A-Za-z` (indices 0-63).

use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use crate::error::{GeeZipError, GeeZipResult};

const XXE_ALPHABET: &[u8; 64] = b"+-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn build_reverse_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    for (i, &ch) in XXE_ALPHABET.iter().enumerate() {
        table[ch as usize] = i as u8;
    }
    table
}

/// Encode raw binary data into an XXE-encoded body (without header/footer).
///
/// Data is split into 45-byte lines, each prefixed with a length
/// character.  This conforms to the standard XXencode format used by
/// `xxe_decode`.
pub fn xxe_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(45) {
        // Length byte using XXE alphabet
        out.push(XXE_ALPHABET[chunk.len().min(45)] as char);
        for triple in chunk.chunks(3) {
            let a = triple.first().copied().unwrap_or(0) as u32;
            let b = triple.get(1).copied().unwrap_or(0) as u32;
            let c = triple.get(2).copied().unwrap_or(0) as u32;
            let val = (a << 16) | (b << 8) | c;
            for shift in [18u32, 12, 6, 0] {
                let six = ((val >> shift) & 0x3F) as usize;
                out.push(XXE_ALPHABET[six] as char);
            }
        }
        out.push('\n');
    }
    out
}

/// Encode data from `reader` into a full XXE-encoded stream (with
/// `begin` / `end` headers) and write it to `writer`.
pub fn xxe_encode_to_writer<R: Read, W: Write>(
    reader: &mut R,
    name: &str,
    writer: &mut W,
) -> GeeZipResult<u64> {
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| GeeZipError::io(e, "reading input for xxe encoding"))?;
    let body = xxe_encode(&data);
    let output = format!("begin 644 {}\n{}end\n", name, body.trim_end());
    writer
        .write_all(output.as_bytes())
        .map_err(|e| GeeZipError::io(e, "writing xxe-encoded output"))?;
    Ok(output.len() as u64)
}

pub fn xxe_decode_file(path: &Path) -> GeeZipResult<(String, Vec<u8>)> {
    let content = fs::read_to_string(path)
        .map_err(|e| GeeZipError::io(e, format!("reading '{}'", path.display())))?;
    xxe_decode(&content).map_err(|e| {
        GeeZipError::format(
            format!("xxencode file '{}': {}", path.display(), e),
            crate::detect::ArchiveFormat::Xxe,
        )
    })
}

pub fn xxe_decode(input: &str) -> Result<(String, Vec<u8>), String> {
    let reverse = build_reverse_table();
    let mut lines = input.lines();

    let header = lines.next().ok_or("missing header line")?;
    let header = header.trim();
    let header = header.strip_prefix("begin ").ok_or("expected 'begin'")?;
    let filename = header
        .split_whitespace()
        .nth(1)
        .unwrap_or("unknown")
        .to_string();

    let mut output = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r').trim_end();
        if line == "end" || line.is_empty() || line == "+" {
            break;
        }
        let bytes = line.as_bytes();
        if bytes.is_empty() {
            continue;
        }
        let encoded_len = reverse[bytes[0] as usize];
        if encoded_len == 0 && bytes[0] != XXE_ALPHABET[0] {
            break;
        }
        let mut remaining = encoded_len as usize;
        if remaining == 0 {
            continue;
        }
        let encoded = &bytes[1..];
        let mut i = 0;
        while i + 3 < encoded.len() && remaining > 0 {
            let a = reverse.get(encoded[i] as usize).copied().unwrap_or(0) as u32;
            let b = reverse.get(encoded[i + 1] as usize).copied().unwrap_or(0) as u32;
            let c = reverse.get(encoded[i + 2] as usize).copied().unwrap_or(0) as u32;
            let d = reverse.get(encoded[i + 3] as usize).copied().unwrap_or(0) as u32;
            let triple = (a << 18) | (b << 12) | (c << 6) | d;
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

    Ok((filename, output))
}

pub fn xxe_decode_to_writer(path: &Path, writer: &mut dyn Write) -> GeeZipResult<u64> {
    let (_filename, data) = xxe_decode_file(path)?;
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
    fn xxe_decode_roundtrip() {
        for data in &[
            b"cat".as_slice(),
            b"hello xxencode".as_slice(),
            b"x".as_slice(),
        ] {
            let encoded = xxe_encode(data);
            let input = format!("begin 644 test.bin\n{}\nend\n", encoded);
            let (filename, decoded) = xxe_decode(&input).unwrap();
            assert_eq!(filename, "test.bin");
            assert_eq!(decoded, data.to_vec());
        }
    }

    #[test]
    fn xxe_decode_malformed_fails() {
        assert!(xxe_decode("not xxencode").is_err());
        assert!(xxe_decode("").is_err());
    }

    #[test]
    fn xxe_decode_empty_data() {
        let encoded = xxe_encode(b"");
        let input = format!("begin 644 empty.txt\n{}\nend\n", encoded);
        let (filename, data) = xxe_decode(&input).unwrap();
        assert_eq!(filename, "empty.txt");
        assert!(data.is_empty());
    }

    #[test]
    fn xxe_decode_file_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.xxe");
        let data = b"test content";
        let encoded = xxe_encode(data);
        let content = format!("begin 644 test.txt\n{}\nend\n", encoded);
        std::fs::write(&path, content).unwrap();
        let (filename, decoded) = xxe_decode_file(&path).unwrap();
        assert_eq!(filename, "test.txt");
        assert_eq!(decoded, data.to_vec());
    }
}
