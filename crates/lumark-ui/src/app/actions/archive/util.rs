//! 归档动作共享的默认名、线程和运行时模块路径。

use super::*;

/// 定位自解压运行时模块。
///
/// 安装包把 `lumark-sfx.exe` 放在主程序同目录；`LUMARK_SFX_STUB` 环境变量
/// 优先，便于开发环境和自动化测试指向构建产物。
pub(crate) fn sfx_stub_path() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("LUMARK_SFX_STUB") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("lumark-sfx.exe");
    candidate.is_file().then_some(candidate)
}

/// 返回密码动作对应的归档路径，供重新提交密码库匹配时使用。
pub(crate) fn password_action_archive(action: &PendingPasswordAction) -> &Path {
    match action {
        PendingPasswordAction::OpenArchive { archive, .. }
        | PendingPasswordAction::ExtractDialog { archive }
        | PendingPasswordAction::OpenEntry { archive, .. }
        | PendingPasswordAction::Test { archive } => archive,
    }
}

/// 返回默认归档名：单选使用文件 stem，多选优先父目录名，根目录退回第一项。
pub(crate) fn default_archive_name(sources: &[PathBuf]) -> String {
    let Some(first) = sources.first() else {
        return "archive".to_string();
    };
    let parent_name = first.parent().and_then(Path::file_name);
    let source_name = if first.is_dir() {
        first.file_name()
    } else {
        first.file_stem()
    };
    let archive_base = if sources.len() > 1 {
        parent_name.or(source_name)
    } else {
        source_name
    };
    archive_base
        .and_then(|value| value.to_str())
        .unwrap_or("archive")
        .to_string()
}

/// 返回适合当前机器的默认压缩线程数。
///
/// 7Z 的 LZMA2 多线程编码会按线程数分配工作缓冲，这里限制在 8 以内，
/// 兼顾速度与内存占用；ZIP 的 Deflate 仍由压缩库决定是否并行。
pub(crate) fn default_compress_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
        .clamp(1, 8)
}
