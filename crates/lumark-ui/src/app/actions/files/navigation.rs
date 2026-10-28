//! 文件浏览刷新、文件夹大小和路径导航动作。

use super::*;

impl LumarkApp {
    pub(crate) fn refresh(&mut self) {
        self.dropped_sources = None;
        self.folder_sizes.clear();
        self.folder_size_pending.clear();
        if matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.archive_password = None;
        }
        match self.browser.refresh() {
            Ok(()) => {
                self.status = format!("{} {}", self.browser.entries.len(), tr("个项目", "items"));
                self.remember_directory();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 在后台递归计算文件夹总大小，并把结果缓存到当前浏览会话。
    pub(crate) fn request_folder_size(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.folder_sizes.contains_key(&path) || self.folder_size_pending.contains(&path) {
            return;
        }
        self.folder_size_pending.insert(path.clone());
        self.status = format!(
            "{}{}",
            tr("正在计算文件夹大小：", "Calculating folder size: "),
            path.display()
        );
        let probe = path.clone();
        cx.spawn(async move |this, cx| {
            let size = cx
                .background_executor()
                .spawn(async move { directory_size(&probe) })
                .await;
            let _ = this.update_in(cx, |app, _, cx| {
                app.folder_size_pending.remove(&path);
                app.folder_sizes.insert(path.clone(), size);
                app.status = format!(
                    "{}{}",
                    tr("文件夹大小：", "Folder size: "),
                    format_size(size)
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// 返回过滤后的搜索结果或浏览器当前条目。
    pub(crate) fn go_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = input_value(&self.location_input, cx);
        let trimmed = value.trim().trim_matches('"');
        if trimmed.is_empty() {
            self.status = tr("请输入文件夹路径", "Enter a folder path").to_string();
            return;
        }
        let path = PathBuf::from(trimmed);
        if path.is_file() && is_archive_path(&path) {
            self.open_archive_with_auto_password(path, ArchiveOpenIntent::Browse, window, cx);
            return;
        } else if path.is_dir() {
            match self.browser.navigate(&path) {
                Ok(()) => {
                    self.search_results = None;
                    self.remember_directory();
                    self.status =
                        format!("{} {}", self.browser.entries.len(), tr("个项目", "items"));
                }
                Err(error) => self.status = error.to_string(),
            }
        } else {
            self.status = format!(
                "{}{}",
                tr("路径不存在：", "Path does not exist: "),
                path.display()
            );
            return;
        }
        set_input(
            &self.location_input,
            self.browser.current_display().as_str(),
            window,
            cx,
        );
    }

    /// 导航到目录或归档路径，并同步位置输入框。
    pub(crate) fn navigate_to(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.archive_password = None;
        match self.browser.navigate(path) {
            Ok(()) => {
                self.search_results = None;
                self.remember_directory();
                self.status = format!("{} {}", self.browser.entries.len(), tr("个项目", "items"));
                set_input(
                    &self.location_input,
                    self.browser.current_display().as_str(),
                    window,
                    cx,
                );
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}
