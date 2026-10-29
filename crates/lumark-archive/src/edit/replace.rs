//! 内置查看器写回归档条目的公开入口。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use zeroize::Zeroizing;

use super::*;

impl ArchiveService {
    /// 用本地文件替换归档内的一个条目。
    ///
    /// 用作内置查看器保存归档条目的公开入口：先确认目标是文件而不是目录，
    /// 再复用 `edit` 的原子替换、校验、备份和取消逻辑。只有 ZIP/ZIPX/7Z
    /// 支持；其他格式返回明确的只读错误。
    pub fn replace_entry<F>(
        archive: &Path,
        entry: &str,
        source: &Path,
        password: Option<&str>,
        keep_backup: bool,
        cancel: Option<Arc<AtomicBool>>,
        mut progress: F,
    ) -> Result<EditReport, ArchiveError>
    where
        F: FnMut(EditProgress),
    {
        let info = Self::list(archive, password)?;
        let target = info
            .entries
            .iter()
            .find(|candidate| candidate.path == entry)
            .ok_or_else(|| ArchiveError::Corrupt(format!("归档中不存在条目：{entry}")))?;
        if target.is_dir {
            return Err(ArchiveError::InvalidRequest(
                "不能把文件保存到归档目录条目".into(),
            ));
        }
        if !matches!(
            info.format,
            Some(ArchiveFormat::Zip | ArchiveFormat::Zipx | ArchiveFormat::SevenZip)
        ) {
            return Err(ArchiveError::UnsupportedFormat(
                info.format
                    .unwrap_or(ArchiveFormat::Unknown)
                    .label()
                    .to_string(),
            ));
        }
        if !source.is_file() {
            return Err(ArchiveError::InvalidRequest(
                "保存源文件不存在或不是普通文件".into(),
            ));
        }
        let options = EditOptions {
            source_password: password.map(|value| Zeroizing::new(value.to_string())),
            output_password: password.map(|value| Zeroizing::new(value.to_string())),
            additions: vec![(entry.to_string(), source.to_path_buf())],
            keep_backup,
            cancel,
            ..Default::default()
        };
        Self::edit(archive, &options, &mut progress)
    }
}
