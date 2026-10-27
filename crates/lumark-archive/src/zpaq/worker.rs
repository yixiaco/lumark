//! 大文件批次的独立工作进程。
//!
//! JIDAC 引擎的单次 `zpaq add` 调用是阻塞的，进程内无法在单个超大文件
//! 压缩到一半时取消。这里把含超大文件的批次交给可杀死的 `lumark-app
//! --zpaq-worker` 子进程执行：父进程轮询取消标志，必要时直接终止子进程。
//! 普通小文件批次仍在进程内执行，避免为每个批次支付进程启动开销。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ArchiveError;

use super::command;

/// 触发独立工作进程的单文件大小阈值。
pub(super) const ZPAQ_WORKER_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// 父进程等待子进程时的轮询间隔。
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 已配置的工作进程可执行文件；未配置时全部操作回退到进程内。
static WORKER: OnceLock<PathBuf> = OnceLock::new();

/// 一次工作进程任务：只包含一条完整的 `zpaq` 命令参数。
#[derive(Debug, Serialize, Deserialize)]
struct WorkerJob {
    args: Vec<String>,
}

/// 判断一个批次是否需要交给可杀死的工作进程。
pub(super) fn needs_worker(sizes: impl IntoIterator<Item = u64>) -> bool {
    sizes
        .into_iter()
        .any(|bytes| bytes >= ZPAQ_WORKER_FILE_BYTES)
}

/// 配置工作进程可执行文件；重复配置保留第一次的结果。
pub(super) fn configure(path: PathBuf) {
    let _ = WORKER.set(path);
}

/// 工作进程入口：从 stdin 读取任务，执行一条 zpaq 命令。
///
/// 退出码：0 成功，2 引擎错误，3 密码错误，4 任务格式错误。
pub(super) fn run_stdio() -> i32 {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("无法读取 ZPAQ 工作进程任务");
        return 4;
    }
    let Ok(job) = serde_json::from_str::<WorkerJob>(&input) else {
        eprintln!("ZPAQ 工作进程任务格式无效");
        return 4;
    };
    let args = job.args.iter().map(String::as_str).collect::<Vec<_>>();
    let password_supplied = job.args.iter().any(|argument| argument == "-key");
    match command::run(&args, password_supplied) {
        Ok(_) => 0,
        Err(ArchiveError::PasswordRequired | ArchiveError::WrongPassword) => {
            eprintln!("password incorrect");
            3
        }
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}

/// 通过工作进程执行一个批次；未配置工作进程或启动失败时回退到进程内。
///
/// 取消时直接终止子进程，随后由上层删除半成品归档；这解决了单个超大
/// 文件在进程内阻塞调用中无法中断的问题。
pub(super) fn run_batch(
    args: &[String],
    password_supplied: bool,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<(), ArchiveError> {
    let Some(executable) = WORKER.get() else {
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        return command::run(&refs, password_supplied).map(|_| ());
    };
    let job = WorkerJob {
        args: args.to_vec(),
    };
    let payload = serde_json::to_vec(&job)
        .map_err(|error| ArchiveError::Corrupt(format!("ZPAQ 工作进程任务编码失败：{error}")))?;
    let mut child = match Command::new(executable)
        .arg("--zpaq-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            // 工作进程缺失或无法启动时不能让压缩失败，回退到进程内执行。
            let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            return command::run(&refs, password_supplied).map(|_| ());
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&payload)
            .map_err(|error| ArchiveError::Corrupt(format!("ZPAQ 工作进程写入失败：{error}")))?;
    }
    loop {
        if is_cancelled(cancel) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ArchiveError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(WORKER_POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ArchiveError::Io(error));
            }
        }
    }
    let output = child.wait_with_output().map_err(ArchiveError::Io)?;
    if output.status.success() {
        return Ok(());
    }
    if output.status.code() == Some(3) {
        return Err(if password_supplied {
            ArchiveError::WrongPassword
        } else {
            ArchiveError::PasswordRequired
        });
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if message.is_empty() {
        return Err(ArchiveError::Corrupt("ZPAQ 工作进程异常终止".to_string()));
    }
    if let Some(message) = message.strip_prefix("归档文件损坏：") {
        Err(ArchiveError::Corrupt(message.to_string()))
    } else if let Some(message) = message.strip_prefix("无法继续：") {
        Err(ArchiveError::InvalidRequest(message.to_string()))
    } else {
        Err(ArchiveError::Corrupt(message))
    }
}

/// 返回取消标志当前是否已置位。
fn is_cancelled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只有达到阈值的文件才需要交给工作进程。
    #[test]
    fn selects_batches_with_large_files() {
        assert!(!needs_worker([1, 1024, ZPAQ_WORKER_FILE_BYTES - 1]));
        assert!(needs_worker([1, ZPAQ_WORKER_FILE_BYTES]));
        assert!(!needs_worker([]));
    }

    /// 任务参数应能通过 JSON 往返，供父子进程传递路径和密码。
    #[test]
    fn serializes_worker_job_round_trip() {
        let job = WorkerJob {
            args: vec![
                "add".to_string(),
                "a.zpaq".to_string(),
                "-key".to_string(),
                "secret".to_string(),
            ],
        };
        let encoded = serde_json::to_vec(&job).expect("应能编码");
        let decoded: WorkerJob = serde_json::from_slice(&encoded).expect("应能解码");
        assert_eq!(decoded.args, job.args);
    }
}
