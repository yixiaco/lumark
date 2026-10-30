//! ZIP 家族修复动作：生成新文件并提交后台任务。

use std::path::{Path, PathBuf};

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 修复当前选中的 ZIP 家族归档。
    pub(crate) fn repair_selected_archive(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.status = tr("请先选择一个压缩包", "Select an archive first").to_string();
            cx.notify();
            return;
        };
        if entry.is_dir || !matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.status = tr(
                "只能在文件系统中修复压缩包",
                "Archives can only be repaired in the file system",
            )
            .to_string();
            cx.notify();
            return;
        }
        let format = ArchiveService::detect(&entry.path);
        if !format.is_zip_family() {
            self.status = format!(
                "{}{}",
                tr("该格式暂不支持修复：", "Repair is not supported for: "),
                format.label()
            );
            cx.notify();
            return;
        }
        let output = repair_output_path(&entry.path);
        let id = self.tasks.submit(TaskRequest::Repair {
            archive: entry.path.clone(),
            output: output.clone(),
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = format!(
            "{}{} → {}",
            tr("开始修复：", "Starting repair: "),
            entry.path.display(),
            output.display()
        );
        cx.notify();
    }
}

/// 生成不覆盖原归档的修复输出路径。
fn repair_output_path(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "archive".to_string());
    let extension = input
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "zip".to_string());
    let mut candidate = parent.join(format!("{stem}.repaired.{extension}"));
    let mut index = 1;
    while candidate.exists() {
        candidate = parent.join(format!("{stem}.repaired-{index}.{extension}"));
        index += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::repair_output_path;

    /// 输出名不会覆盖已有文件。
    #[test]
    fn repair_output_avoids_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.zip");
        let output = repair_output_path(&path);
        assert_eq!(output, directory.path().join("sample.repaired.zip"));
    }
}
