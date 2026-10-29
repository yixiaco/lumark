//! Safe Rust bindings for [libzpaq](https://github.com/zpaq/zpaq) 鈥?the ZPAQ
//! compression library by Matt Mahoney.
//!
//! This crate wraps a small C++ shim (`zpaq_rs_ffi.cpp`) that is compiled at
//! build time via the `cc` crate.  No dynamic libraries are required at
//! runtime; everything is statically linked.
//!
//! # Compression method strings
//!
//! Most functions accept a `method` parameter that controls the ZPAQ
//! compression algorithm and level.  Recognised values:
//!
//! | Value | Meaning |
//! |-------|---------|
//! | `"1"` | Fast (level 1) |
//! | `"2"` | Balanced (level 2) |
//! | `"3"` | Better (level 3) |
//! | `"4"` | Maximum (level 4) |
//! | `"5"` | Ultra (level 5) |
//! | `"x4.3ci1"` | Example explicit method string |
//!
//! Higher numeric levels compress better but are slower and use more memory.
//! Explicit method strings (starting with `x`, `s`, `i`, `0`鈥揱9`) allow
//! fine-grained control; see the [zpaq specification](http://mattmahoney.net/dc/zpaq206.pdf)
//! for details.
//!
//! # Feature flags
//!
//! * **`nojit`** 鈥?Compiles `libzpaq` with `NOJIT` defined, disabling the JIT
//!   x86 back-end.  Required on platforms without a functional x86 JIT (NetBSD,
//!   OpenBSD).  Enabled automatically by the CI for those targets.
//!
//! # Quick start
//!
//! ```rust
//! use zpaq_rs::{compress_to_vec, decompress_to_vec};
//!
//! let original = b"hello zpaq";
//! let compressed = compress_to_vec(original, "1").unwrap();
//! let restored   = decompress_to_vec(&compressed).unwrap();
//! assert_eq!(restored, original);
//! ```
//!
//! For large data or streaming use cases prefer [`compress_stream`] /
//! [`decompress_stream`], which accept any [`std::io::Read`] / [`std::io::Write`].
//!
//! Use [`compress_size`] / [`compress_size_stream`] when you only need the
//! compressed byte count and not the compressed data itself (avoids allocation).
//!
//! # Performance notes
//!
//! * Rust LTO applies within Rust crates; C++ LTO applies within the C++ object
//!   files.  Cross-language inlining (Rust 鈫?C++) is toolchain-dependent.
//! * [`compress_size`] / [`compress_size_stream`] use a C++-side counting writer
//!   to avoid per-byte FFI round-trips and are the fastest way to obtain $C(x)$
//!   for information-theoretic metrics.
//! * [`compress_size_parallel`] / [`compress_size_stream_parallel`] split the
//!   input into ZPAQ blocks and compress them in parallel, which can be faster
//!   on multi-core machines for large inputs.

mod archive_stream;
mod stream;
mod sys;

pub use archive_stream::*;
pub use stream::*;

use std::collections::VecDeque;
use std::ffi::CString;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex};

/// Convenience alias for `std::result::Result<T, ZpaqError>`.
pub type Result<T> = std::result::Result<T, ZpaqError>;

/// Errors returned by this crate.
#[derive(Debug)]
pub enum ZpaqError {
    /// An error originating inside the C++ `libzpaq` / FFI shim.
    ///
    /// The inner string is the message reported by the C++ error channel.  It
    /// may be empty (shown as `"unknown error"`) if libzpaq did not supply one.
    Ffi(String),
    /// A method or path string contained an interior NUL byte (`\0`).
    ///
    /// ZPAQ method strings and file paths are passed to C++ as NUL-terminated
    /// strings, so any input containing `\0` is rejected before crossing the FFI
    /// boundary.
    NulInString,
}

impl std::fmt::Display for ZpaqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZpaqError::Ffi(s) => write!(f, "libzpaq: {s}"),
            ZpaqError::NulInString => write!(f, "string contained NUL byte"),
        }
    }
}

impl std::error::Error for ZpaqError {}

fn last_error_string() -> Option<String> {
    unsafe {
        let len = sys::zpaq_last_error_len();
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        let copied = sys::zpaq_last_error_copy(buf.as_mut_ptr() as *mut c_char, len);
        buf.truncate(copied);
        String::from_utf8(buf).ok()
    }
}

fn last_stdout_string() -> Option<String> {
    unsafe {
        let len = sys::zpaq_last_stdout_len();
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        let copied = sys::zpaq_last_stdout_copy(buf.as_mut_ptr() as *mut c_char, len);
        buf.truncate(copied);
        String::from_utf8(buf).ok()
    }
}

fn last_stderr_string() -> Option<String> {
    unsafe {
        let len = sys::zpaq_last_stderr_len();
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        let copied = sys::zpaq_last_stderr_copy(buf.as_mut_ptr() as *mut c_char, len);
        buf.truncate(copied);
        String::from_utf8(buf).ok()
    }
}

fn err_from_last() -> ZpaqError {
    last_error_string()
        .map(ZpaqError::Ffi)
        .unwrap_or_else(|| ZpaqError::Ffi("unknown error".to_string()))
}

fn clear_last_error() {
    unsafe { sys::zpaq_clear_last_error() };
}

fn clear_last_output() {
    unsafe { sys::zpaq_clear_last_output() };
}

/// Captured output of an embedded `zpaq` command.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ZpaqCommandOutput {
    /// Captured standard output from the command.
    pub stdout: String,
    /// Captured standard error from the command.
    pub stderr: String,
}

fn zpaq_command_inner(args: &[String]) -> Result<ZpaqCommandOutput> {
    clear_last_error();
    clear_last_output();

    let mut cargs = Vec::with_capacity(args.len() + 1);
    cargs.push(CString::new("zpaq").map_err(|_| ZpaqError::NulInString)?);
    for arg in args {
        cargs.push(CString::new(arg.as_str()).map_err(|_| ZpaqError::NulInString)?);
    }
    let ptrs: Vec<*const c_char> = cargs.iter().map(|s| s.as_ptr()).collect();

    let rc = unsafe { sys::zpaq_jidac_run(ptrs.len() as c_int, ptrs.as_ptr()) };
    if rc != 0 {
        return Err(err_from_last());
    }

    Ok(ZpaqCommandOutput {
        stdout: last_stdout_string().unwrap_or_default(),
        stderr: last_stderr_string().unwrap_or_default(),
    })
}

/// A [`Write`] implementation that discards written bytes while counting them.
///
/// Useful when you need the compressed (or decompressed) size without
/// allocating a buffer.  For compressed-size measurements prefer the dedicated
/// [`compress_size`] / [`compress_size_stream`] functions, which use a C++-side
/// counting writer and avoid per-write FFI overhead entirely.
///
/// # Example
///
/// ```rust
/// use std::io::Write;
/// use zpaq_rs::CountingWriter;
///
/// let mut cw = CountingWriter::default();
/// cw.write_all(b"hello").unwrap();
/// assert_eq!(cw.bytes_written(), 5);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct CountingWriter {
    bytes: u64,
}

impl CountingWriter {
    /// Returns the total number of bytes written so far.
    pub fn bytes_written(&self) -> u64 {
        self.bytes
    }
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buf.len() as u64)
            .ok_or_else(|| std::io::Error::other("byte counter overflow"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------- Callback plumbing ----------------

struct ReadCtx<R: Read + Send> {
    reader: R,
}

struct WriteCtx<W: Write + Send> {
    writer: W,
}

unsafe extern "C" fn read_cb<R: Read + Send>(
    ctx: *mut std::os::raw::c_void,
    buf: *mut c_char,
    n: c_int,
) -> c_int {
    unsafe {
        let ctx = &mut *(ctx as *mut ReadCtx<R>);
        let slice = slice::from_raw_parts_mut(buf as *mut u8, n as usize);
        match ctx.reader.read(slice) {
            Ok(bytes) => bytes as c_int,
            Err(e) => {
                set_callback_error(&e.to_string());
                sys::RUST_CALLBACK_ERROR
            }
        }
    }
}

unsafe extern "C" fn write_cb<W: Write + Send>(
    ctx: *mut std::os::raw::c_void,
    buf: *const c_char,
    n: c_int,
) -> c_int {
    unsafe {
        let ctx = &mut *(ctx as *mut WriteCtx<W>);
        let slice = slice::from_raw_parts(buf as *const u8, n as usize);
        match ctx.writer.write_all(slice) {
            Ok(()) => 0,
            Err(e) => {
                set_callback_error(&e.to_string());
                sys::RUST_CALLBACK_ERROR
            }
        }
    }
}

// ---------------- Streaming compressor ----------------

#[derive(Default)]
struct StreamReader {
    buf: VecDeque<u8>,
}

impl StreamReader {
    fn push(&mut self, b: u8) {
        self.buf.push_back(b);
    }
}

impl Read for StreamReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0usize;
        while n < out.len() {
            match self.buf.pop_front() {
                Some(b) => {
                    out[n] = b;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }
}

/// Byte-at-a-time ZPAQ compressor that reports the running encoded bit count.
///
/// Unlike the block-oriented [`compress_stream`], `StreamingCompressor` feeds
/// one byte at a time to the underlying ZPAQ `Compressor` and queries the
/// internal bit counter after each byte.  This is useful for measuring how many
/// bits are required to encode each symbol incrementally 鈥?for example when
/// computing per-symbol information content.
///
/// # Method string restrictions
///
/// Streaming mode only supports:
/// * Numeric levels **1, 2, or 3** 鈥?levels 4 and 5 use block pre-processing
///   that is incompatible with per-byte feeding.
/// * Explicit method strings that start with `x`, `s`, `i`, or a digit and do
///   **not** enable block pre-processing.
///
/// Attempting to create a compressor with level 4, 5, or a method that requires
/// block preprocessing will return [`ZpaqError::Ffi`].
///
/// # Example
///
/// ```rust
/// use zpaq_rs::StreamingCompressor;
///
/// let mut sc = StreamingCompressor::new("2").unwrap();
/// for &b in b"hello" {
///     sc.push(b).unwrap();
/// }
/// println!("bits so far: {:.2}", sc.bits());
/// ```
pub struct StreamingCompressor {
    compressor: *mut sys::Compressor,
    reader: *mut sys::RustReader,
    writer: *mut sys::RustWriter,
    reader_ctx: *mut ReadCtx<StreamReader>,
    writer_ctx: *mut WriteCtx<CountingWriter>,
}

unsafe impl Send for StreamingCompressor {}

impl StreamingCompressor {
    /// Creates a new streaming compressor using the given method string.
    ///
    /// Allocates and initialises the underlying `libzpaq::Compressor`, sets up
    /// internal reader/writer callbacks, writes the ZPAQ block tag, and opens
    /// the first segment ready to receive bytes via [`push`](Self::push).
    ///
    /// Returns [`ZpaqError::Ffi`] if the method is unsupported in streaming mode
    /// (e.g. numeric levels 4鈥?) or if any C++ initialisation step fails.
    pub fn new(method: &str) -> Result<Self> {
        let method_trim = method.trim();
        if method_trim.is_empty() {
            return Err(ZpaqError::Ffi("method string is empty".into()));
        }
        let numeric = method_trim.parse::<i32>().ok();
        let level = numeric.filter(|v| (1..=3).contains(v));
        if numeric.is_some() && level.is_none() {
            return Err(ZpaqError::Ffi(
                "streaming numeric levels support 1..3 only; use x/s/i/0 with no preprocessing"
                    .into(),
            ));
        }

        let compressor = unsafe { sys::zpaq_compressor_new() };
        if compressor.is_null() {
            return Err(ZpaqError::Ffi("zpaq_compressor_new failed".into()));
        }

        let reader_ctx = Box::into_raw(Box::new(ReadCtx {
            reader: StreamReader::default(),
        }));
        let writer_ctx = Box::into_raw(Box::new(WriteCtx {
            writer: CountingWriter::default(),
        }));

        let reader =
            unsafe { sys::zpaq_reader_new(reader_ctx.cast(), None, Some(read_cb::<StreamReader>)) };
        if reader.is_null() {
            unsafe {
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(ZpaqError::Ffi("zpaq_reader_new failed".into()));
        }

        let writer = unsafe {
            sys::zpaq_writer_new(
                writer_ctx.cast(),
                Some(put_cb::<CountingWriter>),
                Some(write_cb::<CountingWriter>),
            )
        };
        if writer.is_null() {
            unsafe {
                sys::zpaq_reader_free(reader);
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(ZpaqError::Ffi("zpaq_writer_new failed".into()));
        }

        let rc_out = unsafe { sys::zpaq_compressor_set_output(compressor, writer) };
        if rc_out != 0 {
            unsafe {
                sys::zpaq_writer_free(writer);
                sys::zpaq_reader_free(reader);
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(err_from_last());
        }

        let rc_in = unsafe { sys::zpaq_compressor_set_input(compressor, reader) };
        if rc_in != 0 {
            unsafe {
                sys::zpaq_writer_free(writer);
                sys::zpaq_reader_free(reader);
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(err_from_last());
        }

        let rc_tag = unsafe { sys::zpaq_compressor_write_tag(compressor) };
        if rc_tag != 0 {
            unsafe {
                sys::zpaq_writer_free(writer);
                sys::zpaq_reader_free(reader);
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(err_from_last());
        }

        let rc_block = if let Some(level) = level {
            unsafe { sys::zpaq_compressor_start_block_level(compressor, level) }
        } else {
            let method_c = CString::new(method_trim).map_err(|_| ZpaqError::NulInString)?;
            unsafe { sys::zpaq_compressor_start_block_method(compressor, method_c.as_ptr()) }
        };
        if rc_block != 0 {
            unsafe {
                sys::zpaq_writer_free(writer);
                sys::zpaq_reader_free(reader);
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(err_from_last());
        }

        let rc_seg =
            unsafe { sys::zpaq_compressor_start_segment(compressor, ptr::null(), ptr::null()) };
        if rc_seg != 0 {
            unsafe {
                sys::zpaq_writer_free(writer);
                sys::zpaq_reader_free(reader);
                sys::zpaq_compressor_free(compressor);
                drop(Box::from_raw(reader_ctx));
                drop(Box::from_raw(writer_ctx));
            }
            return Err(err_from_last());
        }

        Ok(Self {
            compressor,
            reader,
            writer,
            reader_ctx,
            writer_ctx,
        })
    }

    /// Feeds one byte into the compressor and advances the internal state.
    ///
    /// Returns [`ZpaqError::Ffi`] if the underlying `libzpaq::Compressor::compress`
    /// call fails (e.g. due to an I/O error in the underlying writer callback).
    pub fn push(&mut self, b: u8) -> Result<()> {
        unsafe {
            let ctx = &mut *self.reader_ctx;
            ctx.reader.push(b);
        }
        let rc = unsafe { sys::zpaq_compressor_compress(self.compressor, 1) };
        if rc < 0 {
            return Err(err_from_last());
        }
        Ok(())
    }

    /// Returns the number of bits written to the compressed output so far.
    ///
    /// This reflects the running total emitted by `libzpaq`'s internal bit
    /// counter and accounts for both the block header and all bytes fed via
    /// [`push`](Self::push).  The value is a `f64` because `libzpaq` tracks
    /// fractional bits internally.
    pub fn bits(&self) -> f64 {
        unsafe { sys::zpaq_compressor_get_bits(self.compressor) }
    }
}

impl Drop for StreamingCompressor {
    fn drop(&mut self) {
        unsafe {
            sys::zpaq_writer_free(self.writer);
            sys::zpaq_reader_free(self.reader);
            sys::zpaq_compressor_free(self.compressor);
            drop(Box::from_raw(self.reader_ctx));
            drop(Box::from_raw(self.writer_ctx));
        }
    }
}

unsafe extern "C" fn put_cb<W: Write + Send>(ctx: *mut std::os::raw::c_void, c: c_int) -> c_int {
    unsafe {
        let ctx = &mut *(ctx as *mut WriteCtx<W>);
        let byte = [c as u8];
        match ctx.writer.write_all(&byte) {
            Ok(()) => 0,
            Err(e) => {
                set_callback_error(&e.to_string());
                sys::RUST_CALLBACK_ERROR
            }
        }
    }
}

fn set_callback_error(msg: &str) {
    if let Ok(cstr) = CString::new(msg) {
        unsafe { sys::zpaq_set_last_error(cstr.as_ptr()) };
    }
}

struct FfiReader<R: Read + Send> {
    raw: *mut sys::RustReader,
    ctx: *mut ReadCtx<R>,
}

impl<R: Read + Send> FfiReader<R> {
    fn new(reader: R) -> Result<Self> {
        let ctx = Box::into_raw(Box::new(ReadCtx { reader }));
        let raw = unsafe { sys::zpaq_reader_new(ctx as *mut _, None, Some(read_cb::<R>)) };
        if raw.is_null() {
            unsafe {
                drop(Box::from_raw(ctx));
            }
            return Err(err_from_last());
        }
        Ok(Self { raw, ctx })
    }
}

impl<R: Read + Send> Drop for FfiReader<R> {
    fn drop(&mut self) {
        unsafe {
            sys::zpaq_reader_free(self.raw);
            drop(Box::from_raw(self.ctx));
        }
    }
}

struct FfiWriter<W: Write + Send> {
    raw: *mut sys::RustWriter,
    ctx: *mut WriteCtx<W>,
}

impl<W: Write + Send> FfiWriter<W> {
    fn new(writer: W) -> Result<Self> {
        let ctx = Box::into_raw(Box::new(WriteCtx { writer }));
        let raw =
            unsafe { sys::zpaq_writer_new(ctx as *mut _, Some(put_cb::<W>), Some(write_cb::<W>)) };
        if raw.is_null() {
            unsafe {
                drop(Box::from_raw(ctx));
            }
            return Err(err_from_last());
        }
        Ok(Self { raw, ctx })
    }
}

impl<W: Write + Send> Drop for FfiWriter<W> {
    fn drop(&mut self) {
        unsafe {
            sys::zpaq_writer_free(self.raw);
            drop(Box::from_raw(self.ctx));
        }
    }
}

#[derive(Clone, Default)]
struct SharedVecWriter {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl SharedVecWriter {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.inner.lock().expect("poisoned writer buffer").clone()
    }
}

impl Write for SharedVecWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner
            .lock()
            .map_err(|_| std::io::Error::other("poisoned writer buffer"))?
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct SinkWriter;

impl Write for SinkWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
