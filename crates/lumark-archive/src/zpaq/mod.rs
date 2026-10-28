//! ZPAQ 归档后端：基于 `zpaq_rs` 内嵌的 JIDAC 引擎实现读写。
//!
//! 该模块统一封装加密探测、列表、测试、密码校验、创建和解压；所有引擎
//! 调用都通过 [`command`] 串行执行。创建使用标准 JIDAC 格式（带索引和
//! 时间戳），解压先落到输出目录内的暂存区，再按覆盖策略移动到目标位置。

mod command;
mod create;
mod extract;
mod listing;
mod worker;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use zpaq_rs::ZpaqCommandOutput;

use crate::error::ArchiveError;
use crate::format::{ArchiveFormat, EncryptionInfo, ExtractReport};
use crate::service::ExtractProgress;

pub(crate) use create::create;
pub(crate) use extract::extract;

/// 配置用于超大文件批次的可杀死工作进程。
///
/// 应用启动时把当前可执行文件传进来；未配置时所有批次回退到进程内执行。
pub fn configure_zpaq_worker(path: PathBuf) {
    worker::configure(path);
}

/// 运行内置工作进程入口，返回进程退出码。
///
/// 由应用在收到 `--zpaq-worker` 参数时调用，从 stdin 读取任务并执行一条
/// `zpaq` 命令。
pub fn run_zpaq_worker() -> i32 {
    worker::run_stdio()
}

/// 未加密 ZPAQ 归档固定以 JIDAC 标签或旧版 `zPQ` 头开始。
const ZPAQ_TAG: [u8; 4] = *b"7kSt";
/// 旧版 ZPAQ 流式头的前三个字节。
const ZPAQ_LEGACY_TAG: [u8; 3] = *b"zPQ";
/// 加密探测只需读取的归档头长度。
const ZPAQ_HEADER_LEN: usize = 4;

/// 一次压缩最多写入的源字节数。
pub(super) const ZPAQ_BATCH_BYTES: u64 = 32 * 1024 * 1024;
/// 一次压缩最多接受的源文件数，避免 `-to` 前缀扫描退化。
pub(super) const ZPAQ_BATCH_FILES: usize = 64;
/// 选择性解压时最多传给引擎的顶层路径数，超过后退回全量解压。
pub(super) const ZPAQ_MAX_FILTERS: usize = 4096;

/// 探测 ZPAQ 归档是否加密。
///
/// 加密归档以 32 字节随机 salt 开头，因此除明文标签外一律视为加密；
/// 这与 zpaq 自身的密码判断保持一致。
pub(crate) fn probe_encryption(path: &Path) -> Result<EncryptionInfo, ArchiveError> {
    let encrypted = is_encrypted(path)?;
    Ok(EncryptionInfo {
        encrypted,
        header_encrypted: encrypted,
        format: Some(ArchiveFormat::Zpaq),
    })
}

/// 读取归档头判断是否为明文 ZPAQ 归档。
fn is_encrypted(path: &Path) -> Result<bool, ArchiveError> {
    let mut file = File::open(path)?;
    let mut header = [0u8; ZPAQ_HEADER_LEN];
    let read = file.read(&mut header)?;
    if read < 4 {
        return Ok(false);
    }
    if header == ZPAQ_TAG {
        return Ok(false);
    }
    if header[..3] == ZPAQ_LEGACY_TAG && header[3] >= 1 {
        return Ok(false);
    }
    Ok(true)
}

/// 列出 ZPAQ 归档内容。
pub(crate) fn list(
    path: &Path,
    password: Option<&str>,
) -> Result<crate::format::ArchiveInfo, ArchiveError> {
    let (info, _) = list_with_entries(path, password)?;
    Ok(info)
}

/// 列出归档内容并保留原始路径，供解压时构造引擎过滤参数。
fn list_with_entries(
    path: &Path,
    password: Option<&str>,
) -> Result<(crate::format::ArchiveInfo, Vec<listing::ListedEntry>), ArchiveError> {
    let encrypted = is_encrypted(path)?;
    let archive = path_string(path)?;
    let output = run_with_auth(&["list", archive], encrypted, password)?;
    let entries = listing::parse(&output.stdout)?;
    let compressed_size = std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let info = listing::archive_info(&entries, encrypted, compressed_size);
    Ok((info, entries))
}

/// 校验密码是否正确；未加密归档始终返回成功。
pub(crate) fn verify_password(path: &Path, password: &str) -> Result<bool, ArchiveError> {
    if !is_encrypted(path)? {
        return Ok(true);
    }
    match list(path, Some(password)) {
        Ok(_) => Ok(true),
        Err(ArchiveError::PasswordRequired | ArchiveError::WrongPassword) => Ok(false),
        Err(error) => Err(error),
    }
}

/// 可取消的完整测试；测试通过工作进程执行，取消时直接终止引擎。
pub(crate) fn test_cancellable<F>(
    path: &Path,
    password: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
    mut progress: F,
) -> Result<ExtractReport, ArchiveError>
where
    F: FnMut(ExtractProgress),
{
    let (info, _) = list_with_entries(path, password)?;
    let encrypted = is_encrypted(path)?;
    let archive = path_string(path)?.to_string();
    let mut args = vec!["extract".to_string(), archive, "-test".to_string()];
    if encrypted {
        let Some(password) = password else {
            return Err(ArchiveError::PasswordRequired);
        };
        args.push("-key".to_string());
        args.push(password.to_string());
    }
    progress(ExtractProgress {
        current: "正在测试 ZPAQ 归档".to_string(),
        bytes_done: 0,
        bytes_total: info.total_size,
        entries_done: 0,
        entries_total: info.entries.iter().filter(|entry| !entry.is_dir).count(),
    });
    worker::run_command(&args, encrypted, &cancel)?;
    let files = info.entries.iter().filter(|entry| !entry.is_dir).count();
    let report = ExtractReport {
        files,
        bytes: info.total_size,
        skipped: 0,
    };
    progress(ExtractProgress {
        current: "ZPAQ 测试完成".to_string(),
        bytes_done: report.bytes,
        bytes_total: report.bytes,
        entries_done: report.files,
        entries_total: report.files,
    });
    Ok(report)
}

/// 按加密状态决定是否附加 `-key` 后执行命令。
///
/// 未加密归档忽略调用方传入的密码，避免密码库里的旧记录让明文归档打不开。
pub(super) fn run_with_auth(
    args: &[&str],
    encrypted: bool,
    password: Option<&str>,
) -> Result<ZpaqCommandOutput, ArchiveError> {
    if encrypted {
        let Some(password) = password else {
            return Err(ArchiveError::PasswordRequired);
        };
        let mut args = args.to_vec();
        args.push("-key");
        args.push(password);
        command::run(&args, true)
    } else {
        command::run(args, false).map_err(|error| match error {
            ArchiveError::PasswordRequired => {
                ArchiveError::Corrupt("ZPAQ 归档头无效，可能不是 ZPAQ 文件".to_string())
            }
            other => other,
        })
    }
}

/// 按加密状态附加 `-key`，并通过可杀死的 ZPAQ 工作进程执行命令。
pub(super) fn run_with_auth_worker(
    args: &[String],
    encrypted: bool,
    password: Option<&str>,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<(), ArchiveError> {
    let mut args = args.to_vec();
    if encrypted {
        let Some(password) = password else {
            return Err(ArchiveError::PasswordRequired);
        };
        args.push("-key".to_string());
        args.push(password.to_string());
    }
    worker::run_command(&args, encrypted, cancel)
}

/// 把路径转换为引擎可接收的 UTF-8 字符串。
pub(super) fn path_string(path: &Path) -> Result<&str, ArchiveError> {
    path.to_str().ok_or_else(|| {
        ArchiveError::InvalidRequest(format!("ZPAQ 路径包含非 UTF-8 字符：{}", path.display()))
    })
}
