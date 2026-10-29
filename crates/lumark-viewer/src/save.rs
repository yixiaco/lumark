//! 文本保存：原子替换同一目录中的临时文件，并还原编码/换行。

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::encoding::EncodingKind;
use crate::text::Eol;

/// 保存结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    /// 写入字节数。
    pub bytes_written: u64,
    /// 是否使用了 BOM。
    pub bom: bool,
}

/// 保存失败。
#[derive(Debug, Error)]
pub enum SaveError {
    /// 文件系统错误。
    #[error("保存文件失败：{0}")]
    Io(#[from] std::io::Error),
    /// 目标文件在编辑期间被外部修改。
    #[error("文件已被外部修改，请重新打开后再保存")]
    ChangedOnDisk,
}

/// 把文本按原编码和换行风格写回文件。
///
/// 先写同目录临时文件再替换；Windows 上目标存在时先删除再重命名，失败会
/// 保留临时文件并返回错误，避免只写了一半就覆盖原文件。
pub fn save_text(
    path: &Path,
    text: &str,
    encoding: EncodingKind,
    eol: Eol,
    had_bom: bool,
    expected_len: Option<u64>,
    expected_modified: Option<std::time::SystemTime>,
) -> Result<SaveOutcome, SaveError> {
    let metadata = std::fs::metadata(path)?;
    if let Some(expected) = expected_len
        && metadata.len() != expected
    {
        return Err(SaveError::ChangedOnDisk);
    }
    if let Some(expected) = expected_modified
        && metadata.modified().ok() != Some(expected)
    {
        return Err(SaveError::ChangedOnDisk);
    }

    let normalized = eol.normalize(text);
    let bytes = encoding.encode(&normalized, had_bom);
    let temporary = temporary_path(path);
    std::fs::write(&temporary, &bytes)?;
    if let Err(error) = replace_file(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(SaveError::Io(error));
    }
    Ok(SaveOutcome {
        bytes_written: bytes.len() as u64,
        bom: had_bom,
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("lumark-viewer");
    path.with_file_name(format!(".{name}.lumark-save"))
}

fn replace_file(temporary: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // std::fs::rename 在 Windows 上不会覆盖已存在的目标。
        if target.exists() {
            std::fs::remove_file(target)?;
        }
    }
    std::fs::rename(temporary, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_with_original_encoding_and_crlf() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.txt");
        std::fs::write(&path, b"old").unwrap();
        let outcome = save_text(
            &path,
            "第一行\n第二行",
            EncodingKind::Utf8,
            Eol::Crlf,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(outcome.bytes_written, 20);
        let written = std::fs::read(&path).unwrap();
        assert!(written.windows(2).any(|window| window == b"\r\n"));
        assert_eq!(
            String::from_utf8(written).unwrap(),
            "第一行\r\n第二行".to_string()
        );
    }

    #[test]
    fn detects_external_change() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.txt");
        std::fs::write(&path, b"old").unwrap();
        let error = save_text(
            &path,
            "new",
            EncodingKind::Utf8,
            Eol::Lf,
            false,
            Some(999),
            None,
        )
        .unwrap_err();
        assert!(matches!(error, SaveError::ChangedOnDisk));
    }
}
