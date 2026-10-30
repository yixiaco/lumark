//! ASAR 归档创建：按 Electron 格式写出 header 与 integrity。
//!
//! ASAR 的 header 位于载荷之前，而条目大小只有在读完源文件后才知道，因此这里
//! 采用两遍写入：先把载荷流式写入匿名临时文件并计算 SHA-256/分块哈希，再写
//! header 和载荷。这样既不会把整个归档读入内存，也能保持原子替换语义。

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::ArchiveError;
use crate::format::{CompressionOptions, CreateReport};
use crate::path::ArchivePath;
use crate::service::{CreateProgress, build_source_plan, ensure_not_cancelled};

/// ASAR integrity 的分块大小，与 Electron 工具链保持一致。
const BLOCK_SIZE: usize = 4 * 1024 * 1024;
/// 读取源文件和复制载荷时使用的缓冲区大小。
const COPY_BUFFER: usize = 1024 * 1024;

/// ASAR header 根节点。
#[derive(Serialize)]
struct AsarRoot {
    /// 根目录下的条目。
    files: BTreeMap<String, AsarNode>,
}

/// ASAR header 的目录或文件节点。
#[derive(Serialize)]
#[serde(untagged)]
enum AsarNode {
    /// 目录节点。
    Directory {
        /// 子目录/文件。
        files: BTreeMap<String, AsarNode>,
    },
    /// 文件节点。
    File {
        /// 原始大小。
        size: u64,
        /// 相对载荷起点的偏移，Electron 使用字符串保存。
        offset: String,
        /// SHA-256 完整性信息。
        integrity: AsarIntegrity,
    },
}

/// ASAR 单文件完整性信息。
#[derive(Serialize)]
struct AsarIntegrity {
    /// 哈希算法，ASAR 目前只支持 SHA256。
    algorithm: &'static str,
    /// 整文件哈希。
    hash: String,
    /// 分块大小。
    #[serde(rename = "blockSize")]
    block_size: usize,
    /// 每个分块的哈希。
    blocks: Vec<String>,
}

/// 一个已写入临时载荷的待登记文件。
struct PendingFile {
    /// 归档内相对路径。
    path: String,
    /// 原始大小。
    size: u64,
    /// 相对载荷起点的偏移。
    offset: u64,
    /// 完整性信息。
    integrity: AsarIntegrity,
}

/// 创建 ASAR 归档。
pub(crate) fn create<F>(
    output: &Path,
    sources: &[std::path::PathBuf],
    options: &CompressionOptions,
    progress: &mut F,
) -> Result<CreateReport, ArchiveError>
where
    F: FnMut(CreateProgress),
{
    if options.password.is_some() {
        return Err(ArchiveError::InvalidRequest(
            "ASAR 不支持密码；请改用 ZIP/7Z 加密".into(),
        ));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let plan = build_source_plan(sources, options)?;
    let entries_total = plan.files.len();
    let bytes_total = plan.total_bytes;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let mut payload = tempfile::tempfile_in(parent)?;
    let mut pending = Vec::with_capacity(plan.files.len());
    let mut known_files: HashSet<String> = HashSet::new();
    let mut known_dirs: HashSet<String> = HashSet::new();
    let mut offset = 0u64;
    let mut done = 0u64;
    let mut last_report = Instant::now() - std::time::Duration::from_secs(1);

    for (relative, source, _) in plan.files {
        ensure_not_cancelled(&options.cancel)?;
        let parsed = ArchivePath::parse(&relative)?;
        let normalized = parsed.display();
        register_path(&normalized, &mut known_files, &mut known_dirs)?;
        let mut input = File::open(&source)?;
        let file_total = input.metadata()?.len();
        let mut file_done = 0u64;
        let mut hasher = Sha256::new();
        let mut block_hasher = Sha256::new();
        let mut block_filled = 0usize;
        let mut blocks = Vec::new();
        let mut buffer = vec![0u8; COPY_BUFFER];
        loop {
            ensure_not_cancelled(&options.cancel)?;
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            payload.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            let mut consumed = 0usize;
            while consumed < read {
                let take = (read - consumed).min(BLOCK_SIZE - block_filled);
                block_hasher.update(&buffer[consumed..consumed + take]);
                block_filled += take;
                consumed += take;
                if block_filled == BLOCK_SIZE {
                    blocks.push(hex(&block_hasher.finalize_reset()));
                    block_filled = 0;
                }
            }
            file_done += read as u64;
            done += read as u64;
            if last_report.elapsed() >= std::time::Duration::from_millis(100) {
                last_report = Instant::now();
                progress(CreateProgress {
                    current: normalized.clone(),
                    bytes_done: done,
                    bytes_total,
                    current_bytes_done: file_done,
                    current_bytes_total: file_total,
                    output_bytes: offset,
                    entries_done: pending.len(),
                    entries_total,
                });
            }
        }
        if block_filled > 0 {
            blocks.push(hex(&block_hasher.finalize_reset()));
        }
        pending.push(PendingFile {
            path: normalized.clone(),
            size: file_done,
            offset,
            integrity: AsarIntegrity {
                algorithm: "SHA256",
                hash: hex(&hasher.finalize()),
                block_size: BLOCK_SIZE,
                blocks,
            },
        });
        offset = offset.saturating_add(file_done);
        progress(CreateProgress {
            current: normalized,
            bytes_done: done,
            bytes_total,
            current_bytes_done: file_total,
            current_bytes_total: file_total,
            output_bytes: offset,
            entries_done: pending.len(),
            entries_total,
        });
    }

    let root = build_tree(&pending)?;
    let json = serde_json::to_vec(&root)
        .map_err(|error| ArchiveError::Corrupt(format!("ASAR header 序列化失败：{error}")))?;
    let aligned = json.len().div_ceil(4) * 4;
    payload.seek(SeekFrom::Start(0))?;

    let file = File::create(output)?;
    let mut writer = BufWriter::new(file);
    let header_size = aligned
        .checked_add(8)
        .ok_or_else(|| ArchiveError::Corrupt("ASAR header 过大".into()))?;
    let payload_offset = header_size + 8;
    writer.write_all(&4u32.to_le_bytes())?;
    writer.write_all(&(header_size as u32).to_le_bytes())?;
    writer.write_all(&((aligned + 4) as u32).to_le_bytes())?;
    writer.write_all(&(json.len() as u32).to_le_bytes())?;
    writer.write_all(&json)?;
    writer.write_all(&vec![0u8; aligned - json.len()])?;
    writer.flush()?;

    let mut buffer = vec![0u8; COPY_BUFFER];
    let mut copied = 0u64;
    loop {
        ensure_not_cancelled(&options.cancel)?;
        let read = payload.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        copied += read as u64;
        progress(CreateProgress {
            current: "写入 ASAR 载荷".to_string(),
            bytes_done: done,
            bytes_total,
            current_bytes_done: copied,
            current_bytes_total: offset,
            output_bytes: payload_offset as u64 + copied,
            entries_done: pending.len(),
            entries_total,
        });
    }
    writer.flush()?;
    drop(writer);

    Ok(CreateReport {
        files: pending.len(),
        bytes: done,
        output: output.to_path_buf(),
        cleanup_errors: Vec::new(),
    })
}

/// 校验路径唯一性和文件/目录冲突。
fn register_path(
    path: &str,
    files: &mut HashSet<String>,
    dirs: &mut HashSet<String>,
) -> Result<(), ArchiveError> {
    if !files.insert(path.to_string()) {
        return Err(ArchiveError::InvalidRequest(format!(
            "ASAR 条目重复：{path}"
        )));
    }
    if dirs.contains(path) {
        return Err(ArchiveError::InvalidRequest(format!(
            "ASAR 条目同时作为文件和目录：{path}"
        )));
    }
    let components: Vec<&str> = path.split('/').collect();
    let mut current = String::new();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(component);
        if files.contains(&current) {
            return Err(ArchiveError::InvalidRequest(format!(
                "ASAR 文件与目录冲突：{current}"
            )));
        }
        dirs.insert(current.clone());
    }
    Ok(())
}

/// 按文件路径构造 header 的目录树。
fn build_tree(files: &[PendingFile]) -> Result<AsarRoot, ArchiveError> {
    let mut root = BTreeMap::new();
    for file in files {
        let components: Vec<&str> = file.path.split('/').collect();
        let mut level = &mut root;
        for component in components.iter().take(components.len() - 1) {
            let node = level
                .entry(component.to_string())
                .or_insert_with(|| AsarNode::Directory {
                    files: BTreeMap::new(),
                });
            let AsarNode::Directory { files } = node else {
                return Err(ArchiveError::InvalidRequest(format!(
                    "ASAR 路径冲突：{}",
                    file.path
                )));
            };
            level = files;
        }
        level.insert(
            components.last().copied().unwrap_or_default().to_string(),
            AsarNode::File {
                size: file.size,
                offset: file.offset.to_string(),
                integrity: AsarIntegrity {
                    algorithm: file.integrity.algorithm,
                    hash: file.integrity.hash.clone(),
                    block_size: file.integrity.block_size,
                    blocks: file.integrity.blocks.clone(),
                },
            },
        );
    }
    Ok(AsarRoot { files: root })
}

/// 把字节切片转换为小写十六进制。
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
