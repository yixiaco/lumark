//! 单流压缩、解压、大小查询和加密工具。
use super::*;

// ---------------- Public API ----------------

/// Compresses `input` into a `Vec<u8>` using the given ZPAQ method string.
///
/// This is a convenience wrapper around [`compress_stream`] that owns both the
/// input and output buffers.  For large data, prefer [`compress_stream`] to
/// avoid double-buffering, or [`compress_size`] if you only need the size.
///
/// # Example
///
/// ```rust
/// let compressed = zpaq_rs::compress_to_vec(b"hello zpaq", "1").unwrap();
/// assert!(!compressed.is_empty());
/// ```
pub fn compress_to_vec(input: &[u8], method: &str) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(input);
    let mut out = Vec::new();
    compress_stream(cursor, &mut out, method, None, None)?;
    Ok(out)
}

/// Returns the compressed size of `input` in bytes without materialising the
/// compressed data.
///
/// Uses a C++-side counting writer so no allocation is needed for the compressed
/// bytes.  This is the fastest single-threaded way to compute $C(x)$ for
/// information-theoretic metrics such as NCD.
///
/// # Example
///
/// ```rust
/// let sz = zpaq_rs::compress_size(b"aaaaaaaaa", "1").unwrap();
/// assert!(sz > 0);
/// ```
pub fn compress_size(input: &[u8], method: &str) -> Result<u64> {
    compress_size_stream(std::io::Cursor::new(input), method, None, None)
}

/// Returns the compressed size of `input` in bytes using multiple threads.
///
/// Splits the input into ZPAQ blocks (based on the method's block size) and
/// compresses them in parallel using `libzpaq::compressBlock`.  For
/// `threads <= 1` this falls back to the single-threaded path.
///
/// Equivalent to [`compress_size_stream_parallel`] with a [`std::io::Cursor`]
/// over `input`.
pub fn compress_size_parallel(input: &[u8], method: &str, threads: usize) -> Result<u64> {
    compress_size_stream_parallel(std::io::Cursor::new(input), method, None, None, threads)
}

/// Returns the compressed size of data from `reader` in bytes without
/// materialising the compressed output.
///
/// Uses a C++-side counting writer to avoid Rust-side per-write overhead.
/// `filename` and `comment` are optional ZPAQ segment metadata fields; pass
/// `None` for both unless you are building interoperable archives.
pub fn compress_size_stream<R: Read + Send>(
    reader: R,
    method: &str,
    filename: Option<&str>,
    comment: Option<&str>,
) -> Result<u64> {
    clear_last_error();
    let method_c = CString::new(method).map_err(|_| ZpaqError::NulInString)?;
    let filename_c = match filename {
        Some(s) => Some(CString::new(s).map_err(|_| ZpaqError::NulInString)?),
        None => None,
    };
    let comment_c = match comment {
        Some(s) => Some(CString::new(s).map_err(|_| ZpaqError::NulInString)?),
        None => None,
    };
    let reader = FfiReader::new(reader)?;
    let mut out_size: u64 = 0;
    let rc = unsafe {
        sys::zpaq_compress_size(
            reader.raw,
            method_c.as_ptr(),
            filename_c
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(ptr::null()),
            comment_c
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(ptr::null()),
            1,
            &mut out_size as *mut u64,
        )
    };
    if rc == 0 {
        Ok(out_size)
    } else {
        Err(err_from_last())
    }
}

/// Returns the compressed size of data from `reader` in bytes using multiple
/// threads.
///
/// Splits the input into ZPAQ blocks and compresses them in parallel.  Falls
/// back to the single-threaded path when `threads <= 1`.
pub fn compress_size_stream_parallel<R: Read + Send>(
    reader: R,
    method: &str,
    filename: Option<&str>,
    comment: Option<&str>,
    threads: usize,
) -> Result<u64> {
    clear_last_error();
    let method_c = CString::new(method).map_err(|_| ZpaqError::NulInString)?;
    let filename_c = match filename {
        Some(s) => Some(CString::new(s).map_err(|_| ZpaqError::NulInString)?),
        None => None,
    };
    let comment_c = match comment {
        Some(s) => Some(CString::new(s).map_err(|_| ZpaqError::NulInString)?),
        None => None,
    };
    let reader = FfiReader::new(reader)?;
    let mut out_size: u64 = 0;
    let rc = unsafe {
        sys::zpaq_compress_size_parallel(
            reader.raw,
            method_c.as_ptr(),
            filename_c
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(ptr::null()),
            comment_c
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(ptr::null()),
            1,
            threads as i32,
            &mut out_size as *mut u64,
        )
    };
    if rc == 0 {
        Ok(out_size)
    } else {
        Err(err_from_last())
    }
}

/// Returns the archive size (in bytes) that `zpaq add` would produce for a
/// single file on disk.
///
/// Runs the real `zpaq.cpp` JIDAC pipeline in-process 鈥?including fragment
/// deduplication and index overhead 鈥?with `archive=""` so output is discarded
/// but the byte-count accounting is identical to a real archive write.
///
/// This is the correct metric to compare against
/// `zpaq add my.arc <file>; du -b my.arc`.
///
/// `path` must be a valid filesystem path to an existing file.  `threads`
/// controls the number of parallel compression threads; `0` lets libzpaq
/// choose.
///
/// # Errors
///
/// Returns [`ZpaqError::Ffi`] if the file cannot be opened or if the JIDAC
/// pipeline encounters an error.
pub fn zpaq_add_archive_size_file(path: &str, method: &str, threads: usize) -> Result<u64> {
    clear_last_error();
    let path_c = CString::new(path).map_err(|_| ZpaqError::NulInString)?;
    let method_c = CString::new(method).map_err(|_| ZpaqError::NulInString)?;
    let mut out_size: u64 = 0;
    let rc = unsafe {
        sys::zpaq_jidac_add_archive_size_file(
            path_c.as_ptr(),
            method_c.as_ptr(),
            threads as c_int,
            &mut out_size as *mut u64,
        )
    };
    if rc == 0 {
        Ok(out_size)
    } else {
        Err(err_from_last())
    }
}

/// Runs an embedded `zpaq` command in-process and captures its output.
///
/// `args` must contain only the command arguments, exactly as you would pass
/// after `zpaq` on the shell.
///
/// # Examples
///
/// ```rust,no_run
/// let out = zpaq_rs::zpaq_command(&["list", "archive.zpaq"])?;
/// println!("{}", out.stdout);
/// # Ok::<(), zpaq_rs::ZpaqError>(())
/// ```
pub fn zpaq_command(args: &[&str]) -> Result<ZpaqCommandOutput> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    zpaq_command_inner(&owned)
}

/// Equivalent of `zpaq add <archive> <inputs...> -method <method> -threads <threads>`.
///
/// This uses the real JIDAC engine from `zpaq.cpp`, so append semantics,
/// deduplication, and archive metadata are fully interoperable with the `zpaq`
/// binary.
pub fn zpaq_add(
    archive: &str,
    inputs: &[&str],
    method: &str,
    threads: usize,
) -> Result<ZpaqCommandOutput> {
    if inputs.is_empty() {
        return Err(ZpaqError::Ffi(
            "zpaq add requires at least one input path".to_string(),
        ));
    }
    let mut args = Vec::with_capacity(inputs.len() + 7);
    args.push("add".to_string());
    args.push(archive.to_string());
    for input in inputs {
        args.push((*input).to_string());
    }
    args.push("-method".to_string());
    args.push(method.to_string());
    args.push("-threads".to_string());
    args.push(threads.to_string());
    zpaq_command_inner(&args)
}

/// Equivalent of `zpaq extract <archive> [files...]`.
pub fn zpaq_extract(archive: &str, files: &[&str]) -> Result<ZpaqCommandOutput> {
    let mut args = Vec::with_capacity(files.len() + 2);
    args.push("extract".to_string());
    args.push(archive.to_string());
    for file in files {
        args.push((*file).to_string());
    }
    zpaq_command_inner(&args)
}

/// Equivalent of `zpaq list <archive> [files...]`.
pub fn zpaq_list(archive: &str, files: &[&str]) -> Result<ZpaqCommandOutput> {
    let mut args = Vec::with_capacity(files.len() + 2);
    args.push("list".to_string());
    args.push(archive.to_string());
    for file in files {
        args.push((*file).to_string());
    }
    zpaq_command_inner(&args)
}

/// Decompresses a complete ZPAQ stream held in `input` and returns the
/// original data as a `Vec<u8>`.
///
/// This is a convenience wrapper around [`decompress_stream`] that owns both
/// the input and output.  For large data, prefer [`decompress_stream`] to
/// avoid double-buffering.
///
/// # Example
///
/// ```rust
/// let c = zpaq_rs::compress_to_vec(b"hello zpaq", "1").unwrap();
/// let d = zpaq_rs::decompress_to_vec(&c).unwrap();
/// assert_eq!(d, b"hello zpaq");
/// ```
pub fn decompress_to_vec(input: &[u8]) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(input);
    let mut out = Vec::new();
    decompress_stream(cursor, &mut out)?;
    Ok(out)
}

/// Returns the decompressed size of the ZPAQ stream in `input` without
/// materialising the output.
///
/// Wrapper around [`decompress_size_stream`] with a [`std::io::Cursor`].
pub fn decompress_size(input: &[u8]) -> Result<u64> {
    decompress_size_stream(std::io::Cursor::new(input))
}

/// Returns the decompressed size of the ZPAQ stream from `reader` without
/// materialising output.
///
/// Uses a C++-side counting writer so no allocation is needed for the
/// decompressed bytes.
pub fn decompress_size_stream<R: Read + Send>(reader: R) -> Result<u64> {
    clear_last_error();
    let reader = FfiReader::new(reader)?;
    let mut out_size: u64 = 0;
    let rc = unsafe { sys::zpaq_decompress_size(reader.raw, &mut out_size as *mut u64) };
    if rc == 0 {
        Ok(out_size)
    } else {
        Err(err_from_last())
    }
}

/// Compresses data from `reader` and writes the ZPAQ archive to `writer`.
///
/// `method` is the ZPAQ method string (e.g. `"1"`, `"x4.3ci1"`).
/// `filename` and `comment` are optional segment metadata; pass `None` for
/// both in typical usage.
///
/// # Example
///
/// ```rust
/// use std::io::Cursor;
/// let mut out = Vec::new();
/// zpaq_rs::compress_stream(Cursor::new(b"hello"), &mut out, "1", None, None).unwrap();
/// assert!(!out.is_empty());
/// ```
pub fn compress_stream<R: Read + Send, W: Write + Send>(
    reader: R,
    writer: W,
    method: &str,
    filename: Option<&str>,
    comment: Option<&str>,
) -> Result<()> {
    clear_last_error();
    let method_c = CString::new(method).map_err(|_| ZpaqError::NulInString)?;
    let filename_c = match filename {
        Some(s) => Some(CString::new(s).map_err(|_| ZpaqError::NulInString)?),
        None => None,
    };
    let comment_c = match comment {
        Some(s) => Some(CString::new(s).map_err(|_| ZpaqError::NulInString)?),
        None => None,
    };

    let reader = FfiReader::new(reader)?;
    let writer = FfiWriter::new(writer)?;

    let rc = unsafe {
        sys::zpaq_compress(
            reader.raw,
            writer.raw,
            method_c.as_ptr(),
            filename_c
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(ptr::null()),
            comment_c
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(ptr::null()),
            1,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(err_from_last())
    }
}

/// Decompresses a ZPAQ archive from `reader` and writes raw data to `writer`.
///
/// # Example
///
/// ```rust
/// use std::io::Cursor;
/// let compressed = zpaq_rs::compress_to_vec(b"hello", "1").unwrap();
/// let mut out = Vec::new();
/// zpaq_rs::decompress_stream(Cursor::new(&compressed), &mut out).unwrap();
/// assert_eq!(out, b"hello");
/// ```
pub fn decompress_stream<R: Read + Send, W: Write + Send>(reader: R, writer: W) -> Result<()> {
    clear_last_error();
    let reader = FfiReader::new(reader)?;
    let writer = FfiWriter::new(writer)?;
    let rc = unsafe { sys::zpaq_decompress(reader.raw, writer.raw) };
    if rc == 0 {
        Ok(())
    } else {
        Err(err_from_last())
    }
}

/// Derives a 32-byte key from `key32` and `salt32` using scrypt.
///
/// Uses libzpaq's fixed scrypt parameters: N = 16 384, r = 8, p = 1.
/// Both input arrays must be exactly 32 bytes.
///
/// This is the same key-stretching used by `zpaq` encrypted archives.
pub fn stretch_key(key32: [u8; 32], salt32: [u8; 32]) -> Result<[u8; 32]> {
    clear_last_error();
    let mut out = [0u8; 32];
    let rc = unsafe { sys::zpaq_stretch_key(out.as_mut_ptr(), key32.as_ptr(), salt32.as_ptr()) };
    if rc == 0 {
        Ok(out)
    } else {
        Err(err_from_last())
    }
}

/// Returns `len` cryptographically strong random bytes.
///
/// On Unix delegates to `/dev/urandom`; on Windows uses `CryptGenRandom`.
/// Returns [`ZpaqError::Ffi`] if the platform RNG is unavailable.
pub fn random_bytes(len: usize) -> Result<Vec<u8>> {
    clear_last_error();
    let mut buf = vec![0u8; len];
    let rc = unsafe { sys::zpaq_random(buf.as_mut_ptr(), len as c_int) };
    if rc == 0 {
        Ok(buf)
    } else {
        Err(err_from_last())
    }
}

/// Computes the SHA-1 digest of `bytes` using the libzpaq implementation.
///
/// Returns the 20-byte raw digest.  For new designs prefer [`sha256`];
/// SHA-1 is exposed because it is used internally by ZPAQ segment checksums.
pub fn sha1(bytes: &[u8]) -> Result<[u8; 20]> {
    clear_last_error();
    let s = unsafe { sys::zpaq_sha1_new() };
    if s.is_null() {
        return Err(err_from_last());
    }
    unsafe {
        sys::zpaq_sha1_write(s, bytes.as_ptr() as *const c_char, bytes.len() as i64);
    }
    let mut out = [0u8; 20];
    let rc = unsafe { sys::zpaq_sha1_result(s, out.as_mut_ptr()) };
    unsafe { sys::zpaq_sha1_free(s) };
    if rc == 0 {
        Ok(out)
    } else {
        Err(err_from_last())
    }
}

/// Computes the SHA-256 digest of `bytes` using the libzpaq implementation.
///
/// Returns the 32-byte raw digest.
pub fn sha256(bytes: &[u8]) -> Result<[u8; 32]> {
    clear_last_error();
    let s = unsafe { sys::zpaq_sha256_new() };
    if s.is_null() {
        return Err(err_from_last());
    }
    unsafe {
        for &b in bytes {
            sys::zpaq_sha256_put(s, b as c_int);
        }
    }
    let mut out = [0u8; 32];
    let rc = unsafe { sys::zpaq_sha256_result(s, out.as_mut_ptr()) };
    unsafe { sys::zpaq_sha256_free(s) };
    if rc == 0 {
        Ok(out)
    } else {
        Err(err_from_last())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_payloads() -> Vec<Vec<u8>> {
        vec![
            b"".to_vec(),
            b"hello zpaq".to_vec(),
            (0..1024).map(|i| (i * 31 % 251) as u8).collect(),
            (0..20_000)
                .map(|i| (i * 1315423911u64 as usize % 256) as u8)
                .collect(),
        ]
    }

    #[test]
    fn roundtrip_small() {
        let data = b"hello zpaq";
        let c = compress_to_vec(data, "1").expect("compress");
        let d = decompress_to_vec(&c).expect("decompress");
        assert_eq!(d, data);
    }

    #[test]
    fn roundtrip_random() {
        let data: Vec<u8> = (0..10_000).map(|i| (i * 31 % 251) as u8).collect();
        let c = compress_to_vec(&data, "2").expect("compress");
        let d = decompress_to_vec(&c).expect("decompress");
        assert_eq!(d, data);
    }

    #[test]
    fn compress_size_matches_vec_len() {
        for method in ["1", "2", "3", "4", "5", "x4.3ci1"] {
            for data in test_payloads() {
                let c = compress_to_vec(&data, method).expect("compress");
                let sz = compress_size(&data, method).expect("compress_size");
                assert_eq!(sz as usize, c.len(), "method={method}");
            }
        }
    }

    #[test]
    fn decompress_size_matches_output_len() {
        for method in ["1", "2", "x4.3ci1"] {
            for data in test_payloads() {
                let c = compress_to_vec(&data, method).expect("compress");
                let out = decompress_to_vec(&c).expect("decompress");
                let sz = decompress_size(&c).expect("decompress_size");
                assert_eq!(sz as usize, out.len(), "method={method}");
            }
        }
    }

    #[test]
    fn sha_vectors() {
        // "abc" test vectors
        let s1 = sha1(b"abc").expect("sha1");
        assert_eq!(hex::encode(s1), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let s256 = sha256(b"abc").expect("sha256");
        assert_eq!(
            hex::encode(s256),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn random_bytes_len() {
        let b = random_bytes(256).expect("random_bytes");
        assert_eq!(b.len(), 256);
    }

    #[test]
    fn nul_in_method_errors() {
        let err = compress_stream(
            std::io::Cursor::new(b"hello"),
            Vec::<u8>::new(),
            "a\0b",
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ZpaqError::NulInString));
    }

    #[test]
    fn callback_error_propagates() {
        struct FailingReader;

        impl std::io::Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("boom"))
            }
        }

        let err = compress_size_stream(FailingReader, "1", None, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("callback failed"));
    }
}
