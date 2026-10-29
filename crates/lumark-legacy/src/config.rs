//! Compression configuration types.
//!
//! Provides [`CompressOptions`] 鈥?a shared options bag that carries
//! compression level and multi-threading configuration through the
//! core/CLI boundary without coupling the two layers.

/// Compression options passed from CLI/UI down to the core engine.
///
/// # Fields
///
/// * `level` 鈥?optional compression level.  `None` means "use the
///   format's default level".  The valid range is format-dependent:
///   - gzip/tar.gz/xz/tar.xz: 0..=9
///   - zstd/tar.zst: 0..=22
///   - zip/tar: ignored
///
/// * `jobs` 鈥?optional number of worker threads.
///   - `None` / `Some(1)`: single-threaded (default, safe for all formats)
///   - `Some(0)`: automatically use all available logical CPUs
///   - `Some(2..=256)`: explicit thread count
///
///   Formats that act on `jobs`:
///   - **zstd / tar.zst**: native zstd multi-thread encoder
///   - **tar.gz**: pigz-style parallel gzip via `gzp` (when jobs > 1)
///
///   **Note (tar.gz stdin mode):** When compressing via `--stdin` with
///   tar.gz format, the raw-stream compression path is used (the outer
///   gzip layer only), which currently does **not** use the parallel
///   TarGzWriter.  In that mode `--jobs` has no effect; parallel
///   compression only applies during archive-mode (`compress -f tar.gz
///   files...`).
///
/// * `password` 鈥?optional password for ZIP and 7z AES-256 encryption.
///   Only ZIP and 7z formats support this option; using `--password` with
///   other formats will produce an error.
///
/// * `seven_zip` 鈥?optional 7z-specific advanced options (compression method,
///   dictionary size, filename encryption toggle).
#[derive(Clone, Default)]
pub struct CompressOptions {
    pub level: Option<u32>,
    pub jobs: Option<u32>,
    pub password: Option<String>,
    /// 7z-specific advanced options.
    pub seven_zip: Option<SevenZipOptions>,
}

/// 7z-specific compression options.
#[derive(Debug, Clone, Default)]
pub struct SevenZipOptions {
    /// Compression method: "lzma2" (default), "lzma", "bzip2", "ppmd", "deflate", "copy".
    pub method: Option<String>,
    /// LZMA2 dictionary size in bytes (e.g., 16777216 for 16 MiB).
    /// `None` = use level-based default.
    pub dictionary_size: Option<u32>,
    /// If `true`, encrypt file names in addition to content.
    /// `None` = use default (encrypt when password is set).
    pub encrypt_filenames: Option<bool>,
    /// If `true`, enable solid archive mode (all files compressed together
    /// in one block for better compression ratios, especially for small files).
    /// Default: `false` (non-solid, each file compressed independently).
    pub solid: Option<bool>,
}

impl std::fmt::Debug for CompressOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompressOptions")
            .field("level", &self.level)
            .field("jobs", &self.jobs)
            .field("password_set", &self.password.is_some())
            .field("seven_zip", &self.seven_zip)
            .finish()
    }
}

impl CompressOptions {
    /// Return the effective number of worker threads.
    ///
    /// | `jobs`        | Result                               |
    /// |---------------|--------------------------------------|
    /// | `None`        | 1 (single-threaded, backward compat) |
    /// | `Some(1)`     | 1                                    |
    /// | `Some(0)`     | `available_parallelism()` or 1       |
    /// | `Some(n)`     | `n`                                  |
    pub fn effective_jobs(&self) -> usize {
        match self.jobs {
            None | Some(1) => 1,
            Some(0) => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            Some(n) => n as usize,
        }
    }

    /// Return the level, or `None` if level is the format default.
    pub fn level(&self) -> Option<u32> {
        self.level
    }

    /// Builder-style: set the compression level.
    pub fn with_level(mut self, level: Option<u32>) -> Self {
        self.level = level;
        self
    }

    /// Builder-style: set the job count.
    pub fn with_jobs(mut self, jobs: Option<u32>) -> Self {
        self.jobs = jobs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_jobs_default_is_one() {
        let opts = CompressOptions {
            level: None,
            jobs: None,
            password: None,
            seven_zip: None,
        };
        assert_eq!(opts.effective_jobs(), 1);
    }

    #[test]
    fn effective_jobs_explicit_one_is_one() {
        let opts = CompressOptions {
            level: None,
            jobs: Some(1),
            password: None,
            seven_zip: None,
        };
        assert_eq!(opts.effective_jobs(), 1);
    }

    #[test]
    fn effective_jobs_explicit_two() {
        let opts = CompressOptions {
            level: None,
            jobs: Some(2),
            password: None,
            seven_zip: None,
        };
        assert_eq!(opts.effective_jobs(), 2);
    }

    #[test]
    fn effective_jobs_explicit_four() {
        let opts = CompressOptions {
            level: None,
            jobs: Some(4),
            password: None,
            seven_zip: None,
        };
        assert_eq!(opts.effective_jobs(), 4);
    }

    #[test]
    fn effective_jobs_zero_auto_at_least_one() {
        let opts = CompressOptions {
            level: None,
            jobs: Some(0),
            password: None,
            seven_zip: None,
        };
        assert!(opts.effective_jobs() >= 1, "jobs=0 should yield >=1");
    }

    #[test]
    fn level_returns_option_level_field() {
        // Should return None when no level is set
        let opts = CompressOptions {
            level: None,
            jobs: None,
            password: None,
            seven_zip: None,
        };
        assert_eq!(opts.level(), None);

        // Should return Some when level is set
        let opts = CompressOptions {
            level: Some(6),
            jobs: None,
            password: None,
            seven_zip: None,
        };
        assert_eq!(opts.level(), Some(6));
    }

    #[test]
    fn with_level_builder_sets_and_returns_self() {
        let opts = CompressOptions::default()
            .with_level(Some(9))
            .with_jobs(Some(4));

        assert_eq!(opts.level, Some(9));
        // jobs should not be affected by with_level
        assert_eq!(opts.jobs, Some(4));
    }

    #[test]
    fn with_jobs_builder_sets_and_returns_self() {
        let opts = CompressOptions::default().with_jobs(Some(4));

        assert_eq!(opts.effective_jobs(), 4);
        // level should not be affected by with_jobs
        assert_eq!(opts.level, None);
    }

    #[test]
    fn debug_redacts_password_value() {
        let opts = CompressOptions {
            level: Some(6),
            jobs: Some(2),
            password: Some("secret-value".to_string()),
            seven_zip: None,
        };

        let debug = format!("{opts:?}");

        assert!(debug.contains("level: Some(6)"));
        assert!(debug.contains("jobs: Some(2)"));
        assert!(debug.contains("password_set: true"));
        assert!(!debug.contains("secret-value"));
    }
}
