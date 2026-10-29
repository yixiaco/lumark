//! Unified error types for GeeZipX.
//!
//! All core operations return `GeeZipError` instead of library-specific
//! errors. This ensures callers (CLI, GUI) have a single error type to
//! handle.

use std::fmt;

use thiserror::Error;

use crate::detect::ArchiveFormat;

/// Unified error type for all GeeZipX core operations.
///
/// Every variant is `Send + Sync`. Each variant carries enough context
/// for the caller to produce a user-facing error message.
///
/// `Send + Sync` are automatically satisfied since all field types
/// already implement them (no `unsafe` impl needed).
#[derive(Error)]
pub enum GeeZipError {
    /// I/O error with contextual message.
    #[error("{context}: {source}")]
    Io {
        /// The underlying I/O error.
        source: std::io::Error,
        /// Human-readable context (e.g. "failed to read archive").
        context: String,
    },

    /// Format error 鈥?corrupt header, CRC mismatch, etc.
    #[error("format error in {format}: {message}")]
    Format {
        /// Human-readable description of the problem.
        message: String,
        /// The archive format being processed when the error occurred.
        format: ArchiveFormat,
    },

    /// The input data does not match any supported format.
    #[error("unsupported format (magic: {0:#04X?})")]
    UnsupportedFormat(Vec<u8>),

    /// Operation was cancelled by the user.
    #[error("operation cancelled")]
    Cancelled,

    /// Password or encryption related error.
    #[error("crypto error: {message}")]
    Crypto {
        /// Human-readable description.
        message: String,
    },

    /// Path traversal attack detected (Zip Slip protection).
    #[error(
        "path traversal detected: entry '{entry}' resolves outside target directory '{target}', \
         use --unsafe to bypass"
    )]
    PathTraversal {
        /// Entry path inside the archive.
        entry: String,
        /// Target directory the entry would have escaped to.
        target: String,
    },

    #[error("skipped '{path}': file already exists (use --force to overwrite)")]
    ClobberDenied {
        /// Path of the existing file.
        path: String,
    },

    /// Entry not found in archive.
    #[error("entry '{name}' not found in archive")]
    EntryNotFound {
        /// Name of the entry that was looked up.
        name: String,
    },
}

/// Manual Debug impl to avoid printing the inner `source` of Io in an
/// overly verbose way while still keeping it accessible via `.source()`.
impl fmt::Debug for GeeZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Delegate to Display + source chain.
        fmt::Display::fmt(self, f)
    }
}

impl From<std::io::Error> for GeeZipError {
    fn from(source: std::io::Error) -> Self {
        GeeZipError::Io {
            source,
            context: "I/O operation failed".into(),
        }
    }
}

impl GeeZipError {
    /// Create an `Io` error with a custom context string.
    pub fn io(source: std::io::Error, context: impl Into<String>) -> Self {
        GeeZipError::Io {
            source,
            context: context.into(),
        }
    }

    /// Create a `Format` error.
    pub fn format(message: impl Into<String>, format: ArchiveFormat) -> Self {
        GeeZipError::Format {
            message: message.into(),
            format,
        }
    }

    /// Create an unsupported-format error from up to 8 magic bytes.
    pub fn unsupported_format(magic: &[u8]) -> Self {
        let head = if magic.len() > 8 {
            magic[..8].to_vec()
        } else {
            magic.to_vec()
        };
        GeeZipError::UnsupportedFormat(head)
    }

    /// Create a `ClobberDenied` error.
    pub fn clobber_denied(path: impl Into<String>) -> Self {
        GeeZipError::ClobberDenied { path: path.into() }
    }
}

// ---------------------------------------------------------------------------
// Convenience alias
// ---------------------------------------------------------------------------

/// Convenience alias for `Result<T, GeeZipError>`.
pub type GeeZipResult<T> = Result<T, GeeZipError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::ArchiveFormat;

    #[test]
    fn error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<GeeZipError>();
        assert_sync::<GeeZipError>();
    }

    #[test]
    fn error_io_display() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = GeeZipError::io(inner, "opening archive");
        let msg = err.to_string();
        assert!(msg.contains("opening archive"), "msg: {msg}");
        assert!(msg.contains("no such file"), "msg: {msg}");
    }

    #[test]
    fn error_format_display() {
        let err = GeeZipError::format("bad header", ArchiveFormat::Zip);
        let msg = err.to_string();
        assert!(msg.contains("zip"), "msg: {msg}");
        assert!(msg.contains("bad header"), "msg: {msg}");
    }

    #[test]
    fn error_unsupported_format_display() {
        let magic = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let err = GeeZipError::unsupported_format(&magic);
        let msg = err.to_string();
        assert!(
            msg.contains("0xDE") && msg.contains("unsupported format"),
            "msg: {msg}"
        );
    }

    #[test]
    fn error_cancelled_display() {
        let msg = GeeZipError::Cancelled.to_string();
        assert_eq!(msg, "operation cancelled");
    }

    #[test]
    fn error_crypto_display() {
        let err = GeeZipError::Crypto {
            message: "wrong password".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("wrong password"));
    }

    #[test]
    fn error_path_traversal_display() {
        let err = GeeZipError::PathTraversal {
            entry: "../etc/passwd".into(),
            target: "/tmp/out".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("path traversal"), "msg: {msg}");
        assert!(msg.contains("../etc/passwd"), "msg: {msg}");
        assert!(msg.contains("--unsafe"), "msg: {msg}");
    }

    #[test]
    fn error_clobber_denied_display() {
        let err = GeeZipError::clobber_denied("/tmp/out/readme.txt");
        let msg = err.to_string();
        assert!(msg.contains("skipped"), "msg: {msg}");
        assert!(msg.contains("--force"), "msg: {msg}");
        assert!(msg.contains("readme.txt"), "msg: {msg}");
    }

    #[test]
    fn error_entry_not_found_display() {
        let err = GeeZipError::EntryNotFound {
            name: "missing.txt".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing.txt"), "msg: {msg}");
        assert!(msg.contains("not found"), "msg: {msg}");
    }

    #[test]
    fn error_io_from_std() {
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: GeeZipError = inner.into();
        let msg = err.to_string();
        assert!(msg.contains("I/O operation failed"), "msg: {msg}");
        assert!(msg.contains("access denied"), "msg: {msg}");
    }

    #[test]
    fn error_debug_uses_display() {
        let err = GeeZipError::Cancelled;
        let debug = format!("{err:?}");
        let display = format!("{err}");
        assert_eq!(debug, display);
    }
}
