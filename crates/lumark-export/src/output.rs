//! 导出落盘公共工具（E-0）：临时文件 + 原子替换，避免中途失败留下半成品。

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::ExportError;

/// 原子写文件：先写同目录临时文件，再改名为目标路径。
///
/// 目标已存在时会被替换；失败时清理临时文件，不破坏旧文件。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let Some(parent) = path.parent() else {
        return Err(ExportError::Io(format!(
            "输出路径没有父目录：{}",
            path.display()
        )));
    };
    std::fs::create_dir_all(parent)
        .map_err(|error| ExportError::Io(format!("创建输出目录失败：{error}")))?;
    let temporary = temporary_path(path);
    {
        let mut file = std::fs::File::create(&temporary)
            .map_err(|error| ExportError::Io(format!("创建临时文件失败：{error}")))?;
        file.write_all(bytes)
            .map_err(|error| ExportError::Io(format!("写入临时文件失败：{error}")))?;
        file.sync_all()
            .map_err(|error| ExportError::Io(format!("刷新临时文件失败：{error}")))?;
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(ExportError::Io(format!(
            "替换输出文件失败（{}）：{error}",
            path.display()
        )));
    }
    Ok(())
}

/// 生成同目录临时文件名，保留原扩展名便于识别。
fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    path.with_file_name(format!(".{name}.part"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写入、覆盖并确认没有残留临时文件。
    #[test]
    fn writes_and_replaces_atomically() {
        let directory = tempfile::tempdir().expect("临时目录");
        let path = directory.path().join("model.glb");
        write_atomic(&path, b"first").expect("首次写入");
        assert_eq!(std::fs::read(&path).expect("读取"), b"first");
        write_atomic(&path, b"second").expect("覆盖");
        assert_eq!(std::fs::read(&path).expect("读取"), b"second");
        assert!(!directory.path().join(".model.glb.part").exists());
    }

    /// 非法父目录返回错误。
    #[test]
    fn reports_invalid_parent() {
        let error = write_atomic(Path::new(""), b"x").expect_err("空路径应失败");
        assert!(matches!(error, ExportError::Io(_)));
    }
}
