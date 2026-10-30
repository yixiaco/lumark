//! Electron ASAR 只读解析。
//!
//! 头部解析复用 [`asar`] crate，解压、unpacked 与 symlink 的安全约束由本模块
//! 自己实现，避免把归档路径解析交给上游库。
//!
//! # 关键约束
//!
//! - **只读**：ASAR 创建由 `lumark-archive` 的流式写入器负责。
//! - **无固定魔数**：通过扩展名和 `{"files":` 头部结构识别。
//! - **流式读取**：只把 header 读入内存，条目按偏移 seek 后复制。
//! - **unpacked 安全**：只接受 `.asar.unpacked` 目录下、且不经过符号链接
//!   的真实文件。
//! - **symlink 安全**：symlink 只允许解析到归档内安全路径，环或越界直接失败。

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::File as FsFile;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use asar::{Header, header::FileLocation};

use crate::archive::{ArchiveReader, Entry, is_entry_path_dangerous, normalize_path};
use crate::detect::ArchiveFormat;
use crate::error::{GeeZipError, GeeZipResult};

/// ASAR 归档只读读取器。
pub struct AsarReader {
    path: PathBuf,
    format: ArchiveFormat,
    /// 头部解析结果；同一次打开中的列表和多次解压复用，避免重复解析大 header。
    cache: OnceLock<Result<Arc<LoadedAsar>, String>>,
}

impl fmt::Debug for AsarReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsarReader")
            .field("format", &self.format)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl AsarReader {
    /// Create a new ASAR reader for the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            format: ArchiveFormat::Asar,
            cache: OnceLock::new(),
        }
    }

    /// 解析并缓存 header；失败也缓存，避免每次解压重复读大文件。
    fn load_archive(&self) -> GeeZipResult<Arc<LoadedAsar>> {
        let cached = self.cache.get_or_init(|| {
            self.load_uncached()
                .map(Arc::new)
                .map_err(|error| error.to_string())
        });
        match cached {
            Ok(loaded) => Ok(Arc::clone(loaded)),
            Err(error) => Err(GeeZipError::format(error.clone(), ArchiveFormat::Asar)),
        }
    }

    /// 从磁盘读取 header 并构建条目索引。
    fn load_uncached(&self) -> GeeZipResult<LoadedAsar> {
        let mut file = FsFile::open(&self.path)
            .map_err(|e| GeeZipError::io(e, format!("opening '{}'", self.path.display())))?;
        let file_len = file
            .metadata()
            .map_err(|e| GeeZipError::io(e, format!("statting '{}'", self.path.display())))?
            .len();
        let mut prefix = [0u8; 16];
        file.read_exact(&mut prefix).map_err(|e| {
            GeeZipError::io(e, format!("reading ASAR header '{}'", self.path.display()))
        })?;
        let magic = u32::from_le_bytes(prefix[0..4].try_into().unwrap_or_default());
        let header_size = u32::from_le_bytes(prefix[4..8].try_into().unwrap_or_default()) as usize;
        let marker = u32::from_le_bytes(prefix[8..12].try_into().unwrap_or_default()) as usize;
        let json_size = u32::from_le_bytes(prefix[12..16].try_into().unwrap_or_default()) as usize;
        const MAX_HEADER_JSON: usize = 64 * 1024 * 1024;
        if magic != 4 || json_size > MAX_HEADER_JSON || header_size < 8 || marker + 4 != header_size
        {
            return Err(GeeZipError::format(
                format!("invalid ASAR header in '{}'", self.path.display()),
                ArchiveFormat::Asar,
            ));
        }
        let payload_offset = header_size + 8;
        if payload_offset as u64 > file_len {
            return Err(GeeZipError::format(
                format!("truncated ASAR header in '{}'", self.path.display()),
                ArchiveFormat::Asar,
            ));
        }
        let mut json = vec![0u8; json_size];
        file.read_exact(&mut json).map_err(|e| {
            GeeZipError::io(e, format!("reading ASAR header '{}'", self.path.display()))
        })?;
        let mut header_bytes = prefix.to_vec();
        header_bytes.extend_from_slice(&json);
        let (header, parsed_offset) =
            Header::read(&mut &header_bytes[..]).map_err(convert_asar_error)?;
        if parsed_offset != payload_offset {
            return Err(GeeZipError::format(
                format!(
                    "inconsistent ASAR payload offset in '{}'",
                    self.path.display()
                ),
                ArchiveFormat::Asar,
            ));
        }

        let mut records = BTreeMap::new();
        walk_header(
            Path::new(""),
            &header,
            payload_offset as u64,
            file_len,
            &mut records,
        )?;

        Ok(LoadedAsar { records })
    }

    fn read_unpacked_entry(&self, entry_path: &str, writer: &mut dyn Write) -> GeeZipResult<u64> {
        let unpacked_root = self.path.with_extension("asar.unpacked");
        let root_meta = std::fs::symlink_metadata(&unpacked_root).map_err(|e| {
            GeeZipError::io(
                e,
                format!(
                    "opening unpacked ASAR directory '{}' for '{}'",
                    unpacked_root.display(),
                    entry_path
                ),
            )
        })?;

        if root_meta.file_type().is_symlink() {
            return Err(GeeZipError::format(
                format!(
                    "unsafe unpacked ASAR root '{}' is a symlink",
                    unpacked_root.display()
                ),
                ArchiveFormat::Asar,
            ));
        }
        if !root_meta.is_dir() {
            return Err(GeeZipError::format(
                format!(
                    "unpacked ASAR root '{}' is not a directory",
                    unpacked_root.display()
                ),
                ArchiveFormat::Asar,
            ));
        }

        let normalized = normalize_path(Path::new(entry_path));
        if is_entry_path_dangerous(&normalized) {
            return Err(GeeZipError::format(
                format!("unsafe unpacked ASAR entry path '{}'", entry_path),
                ArchiveFormat::Asar,
            ));
        }

        let mut current = unpacked_root.clone();
        let mut components = normalized.components().peekable();
        while let Some(component) = components.next() {
            match component {
                Component::CurDir => continue,
                Component::Normal(name) => {
                    current.push(name);
                    let meta = std::fs::symlink_metadata(&current).map_err(|e| {
                        GeeZipError::io(
                            e,
                            format!(
                                "reading unpacked ASAR entry component '{}' for '{}'",
                                current.display(),
                                entry_path
                            ),
                        )
                    })?;
                    if meta.file_type().is_symlink() {
                        return Err(GeeZipError::format(
                            format!(
                                "unsafe unpacked ASAR entry '{}' traverses symlink '{}'",
                                entry_path,
                                current.display()
                            ),
                            ArchiveFormat::Asar,
                        ));
                    }
                    if components.peek().is_some() && !meta.is_dir() {
                        return Err(GeeZipError::format(
                            format!(
                                "invalid unpacked ASAR path '{}' stops at non-directory '{}'",
                                entry_path,
                                current.display()
                            ),
                            ArchiveFormat::Asar,
                        ));
                    }
                }
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(GeeZipError::format(
                        format!("unsafe unpacked ASAR entry path '{}'", entry_path),
                        ArchiveFormat::Asar,
                    ));
                }
            }
        }

        let file_meta = std::fs::metadata(&current).map_err(|e| {
            GeeZipError::io(
                e,
                format!("statting unpacked ASAR entry '{}'", current.display()),
            )
        })?;
        if file_meta.is_dir() {
            return Err(GeeZipError::format(
                format!(
                    "unpacked ASAR entry '{}' resolves to a directory, not a file",
                    entry_path
                ),
                ArchiveFormat::Asar,
            ));
        }

        let mut input = FsFile::open(&current).map_err(|e| {
            GeeZipError::io(
                e,
                format!("opening unpacked ASAR entry '{}'", current.display()),
            )
        })?;
        std::io::copy(&mut input, writer).map_err(|e| {
            GeeZipError::io(e, format!("writing unpacked ASAR entry '{}'", entry_path))
        })
    }
}

impl ArchiveReader for AsarReader {
    fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn entries(&mut self) -> GeeZipResult<Vec<Entry>> {
        let loaded = self.load_archive()?;
        Ok(loaded
            .records
            .values()
            .map(|record| record.entry.clone())
            .collect())
    }

    fn extract(&mut self, entry: &Entry, writer: &mut dyn Write) -> GeeZipResult<u64> {
        let loaded = self.load_archive()?;
        let record = resolve_record(&entry.path, &loaded.records, &mut HashSet::new())?;

        match &record.source {
            AsarSource::Directory => {
                if entry.is_dir {
                    Ok(0)
                } else {
                    Err(GeeZipError::format(
                        format!("ASAR entry '{}' resolves to a directory", entry.path),
                        ArchiveFormat::Asar,
                    ))
                }
            }
            AsarSource::Packed {
                absolute_offset,
                size,
            } => {
                let mut file = FsFile::open(&self.path).map_err(|e| {
                    GeeZipError::io(e, format!("opening '{}'", self.path.display()))
                })?;
                file.seek(SeekFrom::Start(*absolute_offset)).map_err(|e| {
                    GeeZipError::io(e, format!("seeking ASAR entry '{}'", record.entry.path))
                })?;
                let mut remaining = *size;
                let mut copied = 0u64;
                let mut buffer = [0u8; 64 * 1024];
                while remaining > 0 {
                    let take = remaining.min(buffer.len() as u64) as usize;
                    let read = file.read(&mut buffer[..take]).map_err(|e| {
                        GeeZipError::io(e, format!("reading ASAR entry '{}'", record.entry.path))
                    })?;
                    if read == 0 {
                        return Err(GeeZipError::format(
                            format!("truncated ASAR entry '{}'", record.entry.path),
                            ArchiveFormat::Asar,
                        ));
                    }
                    writer.write_all(&buffer[..read]).map_err(|e| {
                        GeeZipError::io(e, format!("writing ASAR entry '{}'", record.entry.path))
                    })?;
                    copied += read as u64;
                    remaining -= read as u64;
                }
                Ok(copied)
            }
            AsarSource::Unpacked => self.read_unpacked_entry(&record.entry.path, writer),
            AsarSource::Symlink { .. } => Err(GeeZipError::format(
                format!("ASAR symlink entry '{}' could not be resolved", entry.path),
                ArchiveFormat::Asar,
            )),
        }
    }
}

// ASAR 创建由 `lumark-archive` 的流式写入器负责；`lumark-legacy` 只保留读取。

#[derive(Debug)]
struct LoadedAsar {
    records: BTreeMap<String, AsarEntryRecord>,
}

#[derive(Debug, Clone)]
struct AsarEntryRecord {
    entry: Entry,
    source: AsarSource,
}

impl AsarEntryRecord {
    fn directory(path: String) -> Self {
        Self {
            entry: Entry {
                path,
                size: 0,
                compressed_size: 0,
                crc32: None,
                modified: None,
                is_dir: true,
            },
            source: AsarSource::Directory,
        }
    }

    fn file(path: String, size: u64, source: AsarSource) -> Self {
        Self {
            entry: Entry {
                path,
                size,
                compressed_size: size,
                crc32: None,
                modified: None,
                is_dir: false,
            },
            source,
        }
    }

    fn symlink(path: String, raw_target: PathBuf) -> Self {
        Self {
            entry: Entry {
                path,
                size: 0,
                compressed_size: 0,
                crc32: None,
                modified: None,
                is_dir: false,
            },
            source: AsarSource::Symlink { raw_target },
        }
    }
}

#[derive(Debug, Clone)]
enum AsarSource {
    Directory,
    Packed { absolute_offset: u64, size: u64 },
    Unpacked,
    Symlink { raw_target: PathBuf },
}

fn walk_header(
    current_path: &Path,
    header: &Header,
    payload_offset: u64,
    data_len: u64,
    records: &mut BTreeMap<String, AsarEntryRecord>,
) -> GeeZipResult<()> {
    match header {
        Header::Directory { files } => {
            let dir_path = normalize_entry_path(current_path);
            if !dir_path.is_empty() {
                insert_parent_directories(&dir_path, records);
                records
                    .entry(dir_path.clone())
                    .or_insert_with(|| AsarEntryRecord::directory(dir_path));
            }

            let mut children: Vec<_> = files.iter().collect();
            children.sort_by(|a, b| a.0.cmp(b.0));
            for (name, child) in children {
                let child_path = if current_path.as_os_str().is_empty() {
                    PathBuf::from(name)
                } else {
                    current_path.join(name)
                };
                walk_header(&child_path, child, payload_offset, data_len, records)?;
            }
        }
        Header::File(file) => {
            let entry_path = normalize_entry_path(current_path);
            insert_parent_directories(&entry_path, records);
            let size = file.size() as u64;
            let source = match file.location() {
                FileLocation::Offset { offset } => {
                    let absolute_offset =
                        payload_offset.checked_add(offset as u64).ok_or_else(|| {
                            GeeZipError::format(
                                format!("ASAR entry '{}' offset overflowed", entry_path),
                                ArchiveFormat::Asar,
                            )
                        })?;
                    let end = absolute_offset
                        .checked_add(file.size() as u64)
                        .ok_or_else(|| {
                            GeeZipError::format(
                                format!("ASAR entry '{}' size overflowed", entry_path),
                                ArchiveFormat::Asar,
                            )
                        })?;
                    if end > data_len {
                        return Err(GeeZipError::format(
                            format!(
                                "truncated ASAR entry '{}' (offset {}, size {}, archive {})",
                                entry_path,
                                offset,
                                file.size(),
                                data_len
                            ),
                            ArchiveFormat::Asar,
                        ));
                    }
                    AsarSource::Packed {
                        absolute_offset,
                        size: file.size() as u64,
                    }
                }
                FileLocation::Unpacked { .. } => AsarSource::Unpacked,
            };
            records.insert(
                entry_path.clone(),
                AsarEntryRecord::file(entry_path, size, source),
            );
        }
        Header::Link { link } => {
            let entry_path = normalize_entry_path(current_path);
            insert_parent_directories(&entry_path, records);
            records.insert(
                entry_path.clone(),
                AsarEntryRecord::symlink(entry_path, link.clone()),
            );
        }
    }

    Ok(())
}

fn resolve_record<'a>(
    entry_path: &str,
    records: &'a BTreeMap<String, AsarEntryRecord>,
    seen: &mut HashSet<String>,
) -> GeeZipResult<&'a AsarEntryRecord> {
    if !seen.insert(entry_path.to_owned()) {
        return Err(GeeZipError::format(
            format!(
                "cyclic ASAR symlink resolution detected at '{}'",
                entry_path
            ),
            ArchiveFormat::Asar,
        ));
    }

    let record = records
        .get(entry_path)
        .ok_or_else(|| GeeZipError::EntryNotFound {
            name: entry_path.to_owned(),
        })?;

    match &record.source {
        AsarSource::Symlink { raw_target } => {
            let resolved_target = resolve_symlink_target(entry_path, raw_target)?;
            resolve_record(&resolved_target, records, seen)
        }
        _ => Ok(record),
    }
}

fn resolve_symlink_target(entry_path: &str, raw_target: &Path) -> GeeZipResult<String> {
    let parent = Path::new(entry_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let raw_target = raw_target.to_string_lossy().replace('\\', "/");
    if looks_like_windows_absolute_or_unc(&raw_target) {
        return Err(GeeZipError::format(
            format!(
                "unsafe ASAR symlink target '{}' for '{}'",
                raw_target, entry_path
            ),
            ArchiveFormat::Asar,
        ));
    }
    let joined = normalize_path(&parent.join(&raw_target));
    if is_entry_path_dangerous(&joined) {
        return Err(GeeZipError::format(
            format!(
                "unsafe ASAR symlink target '{}' for '{}'",
                raw_target, entry_path
            ),
            ArchiveFormat::Asar,
        ));
    }

    let resolved = normalize_entry_path(&joined);
    if resolved.is_empty() {
        return Err(GeeZipError::format(
            format!("empty ASAR symlink target for '{}'", entry_path),
            ArchiveFormat::Asar,
        ));
    }
    Ok(resolved)
}

fn normalize_entry_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn insert_parent_directories(path: &str, records: &mut BTreeMap<String, AsarEntryRecord>) {
    if path.is_empty() || path.starts_with('/') || looks_like_windows_absolute_or_unc(path) {
        return;
    }

    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() <= 1 {
        return;
    }

    let mut current = String::new();
    for part in parts.iter().take(parts.len() - 1) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        let dir = current.clone();
        records
            .entry(dir.clone())
            .or_insert_with(|| AsarEntryRecord::directory(dir));
    }
}

fn looks_like_windows_absolute_or_unc(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("//")
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\'))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn convert_asar_error(err: asar::Error) -> GeeZipError {
    match err {
        asar::Error::Io(e) => GeeZipError::io(e, "reading ASAR archive"),
        asar::Error::UnpackedIoError { path, err } => GeeZipError::io(
            err,
            format!("reading unpacked ASAR entry '{}'", path.display()),
        ),
        asar::Error::Json(e) => GeeZipError::format(
            format!("invalid ASAR header JSON: {e}"),
            ArchiveFormat::Asar,
        ),
        asar::Error::Truncated => {
            GeeZipError::format("truncated ASAR archive", ArchiveFormat::Asar)
        }
        asar::Error::HashMismatch {
            file,
            block,
            expected,
            actual,
        } => GeeZipError::format(
            format!(
                "ASAR integrity mismatch for '{}'{} (expected {}, got {})",
                file.display(),
                block
                    .map(|idx| format!(", block #{idx}"))
                    .unwrap_or_default(),
                bytes_to_hex(&expected),
                bytes_to_hex(&actual)
            ),
            ArchiveFormat::Asar,
        ),
        asar::Error::FileAlreadyWritten(path) => GeeZipError::format(
            format!("duplicate ASAR file '{}'", path.display()),
            ArchiveFormat::Asar,
        ),
        asar::Error::InvalidHashAlgorithm(alg) => GeeZipError::format(
            format!("invalid ASAR hash algorithm '{alg}'"),
            ArchiveFormat::Asar,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按给定 JSON 和载荷写出一个最小 ASAR。
    fn write_asar(path: &Path, json: &str, payload: &[u8]) {
        let mut json_bytes = json.as_bytes().to_vec();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(0);
        }
        let header_size = json_bytes.len() + 8;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&(header_size as u32).to_le_bytes());
        bytes.extend_from_slice(&((header_size - 4) as u32).to_le_bytes());
        bytes.extend_from_slice(&(json.as_bytes().len() as u32).to_le_bytes());
        bytes.extend_from_slice(&json_bytes);
        bytes.extend_from_slice(payload);
        std::fs::write(path, bytes).unwrap();
    }

    /// 验证 packed 条目读取和大小校验。
    #[test]
    fn reads_packed_entry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.asar");
        write_asar(
            &path,
            r#"{"files":{"hello.txt":{"size":5,"offset":"0"}}}"#,
            b"hello",
        );
        let mut reader = AsarReader::new(&path);
        let entries = reader.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "hello.txt");
        let mut output = Vec::new();
        let written = reader.extract(&entries[0], &mut output).unwrap();
        assert_eq!(written, 5);
        assert_eq!(output, b"hello");
    }

    /// 验证 unpacked 条目从同目录的 `.asar.unpacked` 读取。
    #[test]
    fn reads_unpacked_entry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.asar");
        write_asar(
            &path,
            r#"{"files":{"big.bin":{"size":5,"unpacked":true}}}"#,
            b"",
        );
        let unpacked = directory.path().join("sample.asar.unpacked");
        std::fs::create_dir_all(&unpacked).unwrap();
        std::fs::write(unpacked.join("big.bin"), b"world").unwrap();
        let mut reader = AsarReader::new(&path);
        let entries = reader.entries().unwrap();
        let mut output = Vec::new();
        reader.extract(&entries[0], &mut output).unwrap();
        assert_eq!(output, b"world");
    }

    /// 验证截断和损坏头部被拒绝。
    #[test]
    fn rejects_truncated_or_corrupt_header() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad.asar");
        std::fs::write(&path, b"short").unwrap();
        assert!(AsarReader::new(&path).entries().is_err());

        let path = directory.path().join("bad-header.asar");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&96u32.to_le_bytes());
        bytes.extend_from_slice(&500u32.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(AsarReader::new(&path).entries().is_err());
    }
}
