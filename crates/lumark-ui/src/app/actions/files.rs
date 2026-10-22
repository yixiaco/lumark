use super::super::*;

/// 临时解压的用途：复制到剪贴板或准备原生拖出。
#[derive(Clone, Copy)]
pub(crate) enum TempExtractMode {
    Copy,
    Drag { hwnd: isize },
}

/// 后台临时解压回传的事件。
enum TempExtractEvent {
    Progress(lumark_archive::ExtractProgress),
    Finished(Result<Vec<(PathBuf, bool)>, String>),
}

impl LumarkApp {
    /// 重新加载当前文件系统或归档位置，并清除过期搜索结果。
    pub(crate) fn refresh(&mut self) {
        self.dropped_sources = None;
        self.folder_sizes.clear();
        self.folder_size_pending.clear();
        if matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.archive_password = None;
        }
        match self.browser.refresh() {
            Ok(()) => {
                self.status = format!("{} 个项目", self.browser.entries.len());
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
        self.status = format!("正在计算文件夹大小：{}", path.display());
        let probe = path.clone();
        cx.spawn(async move |this, cx| {
            let size = cx
                .background_executor()
                .spawn(async move { directory_size(&probe) })
                .await;
            let _ = this.update_in(cx, |app, _, cx| {
                app.folder_size_pending.remove(&path);
                app.folder_sizes.insert(path.clone(), size);
                app.status = format!("文件夹大小：{}", format_size(size));
                cx.notify();
            });
        })
        .detach();
    }

    /// 返回过滤后的搜索结果或浏览器当前条目。
    pub(crate) fn displayed_entries(&self) -> Vec<BrowserEntry> {
        self.search_results
            .clone()
            .unwrap_or_else(|| self.browser.entries.clone())
    }

    /// 返回虚拟列表当前可见的行数。
    pub(crate) fn displayed_count(&self) -> usize {
        self.search_results
            .as_ref()
            .map(Vec::len)
            .unwrap_or(self.browser.entries.len())
    }

    /// 为稳定的虚拟行索引返回克隆后的显示条目。
    pub(crate) fn displayed_entry_at(&self, index: usize) -> Option<BrowserEntry> {
        self.search_results
            .as_ref()
            .map_or_else(
                || self.browser.entries.get(index),
                |entries| entries.get(index),
            )
            .cloned()
    }

    /// 按显示顺序返回用于范围选择的稳定 ID。
    pub(crate) fn displayed_ids(&self) -> Vec<String> {
        self.search_results
            .as_ref()
            .unwrap_or(&self.browser.entries)
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// 选择当前过滤视图中的全部项目。
    pub(crate) fn select_all_displayed(&mut self) {
        let ids = self.displayed_ids();
        self.browser.selection.select_all(&ids);
    }

    /// 导航到位置输入框中输入的路径。
    pub(crate) fn go_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = input_value(&self.location_input, cx);
        let trimmed = value.trim().trim_matches('"');
        if trimmed.is_empty() {
            self.status = "请输入文件夹路径".to_string();
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
                    self.status = format!("{} 个项目", self.browser.entries.len());
                }
                Err(error) => self.status = error.to_string(),
            }
        } else {
            self.status = format!("路径不存在：{}", path.display());
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
                self.status = format!("{} 个项目", self.browser.entries.len());
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

    /// 把已展开的目录树铺平为侧栏行。
    pub(crate) fn tree_rows(&self) -> Vec<TreeRow> {
        let mut roots = Vec::new();
        for letter in b'A'..=b'Z' {
            let path = PathBuf::from(format!("{}:\\", letter as char));
            if path.is_dir() {
                roots.push(path);
            }
        }
        let mut rows = Vec::new();
        for root in roots {
            self.append_tree_rows(&root, 0, &mut rows);
        }
        rows
    }

    /// 追加一个已展开树节点的可见后代。
    pub(crate) fn append_tree_rows(&self, path: &Path, depth: usize, rows: &mut Vec<TreeRow>) {
        let label = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| path.display().to_string());
        let expanded = self.tree_expanded.contains(path);
        rows.push(TreeRow {
            path: path.to_path_buf(),
            label,
            depth,
            expanded,
        });
        if expanded && let Some(children) = self.tree_children.get(path) {
            for child in children {
                self.append_tree_rows(child, depth + 1, rows);
            }
        }
    }

    /// 展开或折叠目录节点，并按需加载子目录。
    pub(crate) fn toggle_tree_path(&mut self, path: PathBuf) {
        if self.tree_expanded.remove(&path) {
            return;
        }
        if !self.tree_children.contains_key(&path) {
            let mut children = match std::fs::read_dir(&path) {
                Ok(entries) => entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        entry
                            .file_type()
                            .ok()
                            .filter(|file_type| file_type.is_dir())
                            .map(|_| entry.path())
                    })
                    .collect::<Vec<_>>(),
                Err(error) => {
                    self.status = format!("无法读取 {}：{error}", path.display());
                    return;
                }
            };
            children.sort_by(|left, right| {
                left.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(
                        &right
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase(),
                    )
            });
            self.tree_children.insert(path.clone(), children);
        }
        self.tree_expanded.insert(path);
    }

    /// 对当前文件系统或归档执行名称搜索。
    pub(crate) fn run_search(&mut self, cx: &mut Context<Self>) {
        let pattern = input_value(&self.search_input, cx);
        if pattern.trim().is_empty() {
            self.search_results = None;
            self.status = "已清除搜索".to_string();
            return;
        }
        let results = self.browser.search(&lumark_browser::SearchQuery {
            pattern,
            recursive: true,
            case_sensitive: false,
            mode: self.search_mode,
            ..Default::default()
        });
        self.status = format!("找到 {} 个项目", results.len());
        self.search_results = Some(results);
    }

    /// 清除当前搜索结果并恢复正常文件列表。
    pub(crate) fn clear_search(&mut self) {
        self.search_results = None;
        self.status = format!("{} 个项目", self.browser.entries.len());
    }

    /// 切换详情检查器并持久化偏好。
    pub(crate) fn toggle_details(&mut self) {
        self.details_visible = !self.details_visible;
        let visible = self.details_visible;
        let _ = self
            .settings
            .update(|settings| settings.details_visible = visible);
    }

    /// 循环切换文件列表列宽并持久化所选布局。
    pub(crate) fn cycle_column_layout(&mut self) {
        self.column_layout = (self.column_layout + 1) % 3;
        let layout = self.column_layout;
        let _ = self
            .settings
            .update(|settings| settings.column_layout = layout);
    }

    /// 根据数量与类型，把拖入文件路由到打开、压缩或复制操作。
    pub(crate) fn handle_external_drop(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        if matches!(&self.browser.location, BrowserLocation::Archive { .. }) {
            self.add_files_to_current_archive(paths);
            return;
        }
        if paths.len() == 1 && paths[0].is_file() && is_archive_path(&paths[0]) {
            self.open_archive_with_auto_password(
                paths[0].clone(),
                ArchiveOpenIntent::Browse,
                window,
                cx,
            );
        } else if paths.len() == 1 && paths[0].is_dir() {
            self.navigate_to(paths[0].clone(), window, cx);
        } else {
            self.dropped_sources = Some(paths.to_vec());
            self.open_compress_dialog(window, cx);
            self.status = format!("已接收 {} 个拖入项目", paths.len());
        }
    }

    /// 暂存拖入文件并请求用户选择更新模式。
    ///
    /// 归档重建代价高，不能按一次拖放直接覆盖；这里先确认同名条目的处理
    /// 方式，确认后才提交后台编辑任务。
    pub(crate) fn add_files_to_current_archive(&mut self, paths: &[PathBuf]) {
        if !matches!(&self.browser.location, BrowserLocation::Archive { .. }) {
            return;
        }
        let additions: Vec<(String, PathBuf)> = paths
            .iter()
            .filter(|path| path.is_file())
            .filter_map(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .map(|name| (name.to_string(), path.clone()))
            })
            .collect();
        if additions.is_empty() {
            self.status = "当前仅支持将文件拖入归档；文件夹请先压缩".to_string();
            return;
        }
        self.status = format!("准备向归档添加 {} 个文件", additions.len());
        self.pending_archive_additions = additions;
        self.show_archive_add_dialog = true;
    }

    /// 按所选更新模式提交归档添加任务。
    pub(crate) fn confirm_archive_add(&mut self, cx: &mut Context<Self>) {
        self.show_archive_add_dialog = false;
        let additions = std::mem::take(&mut self.pending_archive_additions);
        let BrowserLocation::Archive { archive, .. } = &self.browser.location else {
            self.status = "当前不在归档位置".to_string();
            cx.notify();
            return;
        };
        let archive = archive.clone();
        let options = EditOptions {
            source_password: self.archive_password.clone(),
            additions,
            update_mode: self.archive_add_mode,
            keep_backup: self.settings.get().archive_edit_backup,
            ..Default::default()
        };
        self.submit_archive_edit(archive, options, PendingEditAction::Add);
        cx.notify();
    }

    /// 放弃本次添加，保持归档内容不变。
    pub(crate) fn cancel_archive_add(&mut self, cx: &mut Context<Self>) {
        self.show_archive_add_dialog = false;
        self.pending_archive_additions.clear();
        self.status = "已取消添加文件".to_string();
        cx.notify();
    }

    /// 把归档编辑加入队列，并在替换成功前保持原文件不变。
    pub(crate) fn submit_archive_edit(
        &mut self,
        archive: PathBuf,
        options: EditOptions,
        action: PendingEditAction,
    ) {
        if !self.pending_edit_jobs.is_empty() {
            self.status = "请等待当前归档更新完成".to_string();
            return;
        }
        if !matches!(&self.browser.location, BrowserLocation::Archive { .. }) {
            self.status = "当前不在归档位置".to_string();
            return;
        }
        let id = self.tasks.submit(TaskRequest::Edit { archive, options });
        self.pending_edit_jobs.insert(
            id,
            PendingEditJob {
                archive: match &self.browser.location {
                    BrowserLocation::Archive { archive, .. } => archive.clone(),
                    BrowserLocation::FileSystem(_) => unreachable!(),
                },
                action,
            },
        );
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = "正在重建归档…".to_string();
    }

    /// 报告归档编辑完成并刷新归档视图。
    pub(crate) fn finish_edit_action(&mut self, action: PendingEditAction) {
        match action {
            PendingEditAction::Add => {}
            PendingEditAction::Delete => {
                self.pending_delete_paths.clear();
                self.show_delete_confirm = false;
                self.browser.selection.clear();
            }
            PendingEditAction::Rename => {
                self.show_rename = false;
                self.rename_target = None;
            }
        }
    }

    /// 返回当前选择对应的去重路径集合。
    pub(crate) fn selected_paths(&self) -> Vec<PathBuf> {
        let ids = self.browser.selection.selected_ids();
        self.displayed_entries()
            .into_iter()
            .filter(|entry| ids.contains(&entry.id))
            .map(|entry| entry.path)
            .collect()
    }

    /// 根据位置打开回收站删除或归档删除确认框。
    pub(crate) fn request_delete(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status = "请先选择要删除的项目".to_string();
            return;
        }
        self.pending_delete_paths = paths;
        self.show_delete_confirm = true;
    }

    /// 通过回收站删除文件，或提交归档删除编辑。
    pub(crate) fn confirm_delete(&mut self) {
        if self.pending_delete_paths.is_empty() {
            self.show_delete_confirm = false;
            return;
        }
        if let BrowserLocation::Archive { archive, .. } = &self.browser.location {
            let archive = archive.clone();
            let delete_paths = self
                .pending_delete_paths
                .iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect();
            let options = EditOptions {
                source_password: self.archive_password.clone(),
                delete_paths,
                keep_backup: self.settings.get().archive_edit_backup,
                ..Default::default()
            };
            self.submit_archive_edit(archive, options, PendingEditAction::Delete);
            return;
        }
        match platform::recycle(&self.pending_delete_paths) {
            Ok(()) => {
                self.status = format!("已将 {} 个项目移入回收站", self.pending_delete_paths.len());
                self.pending_delete_paths.clear();
                self.show_delete_confirm = false;
                self.browser.selection.clear();
                self.refresh();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 打开新建文件夹对话框并清空名称输入框。
    pub(crate) fn start_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.status = "只能在文件系统目录中新建文件夹".to_string();
            return;
        }
        set_input(&self.new_folder_name, "", window, cx);
        self.show_new_folder = true;
    }

    /// 在文件系统位置或当前归档中创建经过校验的文件夹。
    pub(crate) fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = input_value(&self.new_folder_name, cx);
        let Some(parent) = self.current_directory() else {
            self.status = "当前不在文件系统目录".to_string();
            return;
        };
        if !valid_file_name(&name) {
            self.status = "文件夹名称无效".to_string();
            return;
        }
        match std::fs::create_dir(parent.join(name.trim())) {
            Ok(()) => {
                self.show_new_folder = false;
                self.status = "已创建文件夹".to_string();
                self.refresh();
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

    /// 打开重命名对话框并预填所选项目名称。
    pub(crate) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.status = "请先选择要重命名的项目".to_string();
            return;
        };
        if !matches!(
            &self.browser.location,
            BrowserLocation::FileSystem(_) | BrowserLocation::Archive { .. }
        ) {
            self.status = "归档内重命名尚未启用，请先解压".to_string();
            return;
        }
        self.rename_target = Some(entry.path.clone());
        set_input(&self.rename_name, &entry.name, window, cx);
        self.show_rename = true;
    }

    /// 校验并应用文件系统重命名或归档条目重命名。
    pub(crate) fn confirm_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.rename_target.clone() else {
            self.show_rename = false;
            return;
        };
        let name = input_value(&self.rename_name, cx);
        if !valid_file_name(&name) {
            self.status = "名称无效".to_string();
            return;
        }
        let destination = target.with_file_name(name.trim());
        if let BrowserLocation::Archive { archive, .. } = &self.browser.location {
            let archive = archive.clone();
            let old = target.to_string_lossy().replace('\\', "/");
            let new = {
                let mut parts: Vec<String> = old.split('/').map(ToString::to_string).collect();
                if let Some(last) = parts.last_mut() {
                    *last = name.trim().to_string();
                }
                parts.join("/")
            };
            let options = EditOptions {
                source_password: self.archive_password.clone(),
                rename: vec![(old, new)],
                keep_backup: self.settings.get().archive_edit_backup,
                ..Default::default()
            };
            self.submit_archive_edit(archive, options, PendingEditAction::Rename);
            return;
        }
        match std::fs::rename(&target, &destination) {
            Ok(()) => {
                self.show_rename = false;
                self.rename_target = None;
                self.status = "已重命名".to_string();
                self.refresh();
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

    /// 复制所选文件或归档条目为资源管理器可粘贴的真实文件。
    ///
    /// 归档条目没有真实路径，因此先解压到当前窗口的临时目录，再把临时文件
    /// 写入 `CF_HDROP`；临时目录随窗口关闭一起删除，多个归档窗口互不影响。
    pub(crate) fn copy_selected_items(&mut self, cx: &mut Context<Self>) {
        let entries = self.displayed_entries();
        let selected = self
            .browser
            .selection
            .selected_ids()
            .into_iter()
            .filter_map(|id| entries.iter().find(|entry| entry.id == id).cloned())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            self.status = "请选择要复制的项目".to_string();
            return;
        }

        if let BrowserLocation::Archive { archive, .. } = self.browser.location.clone() {
            let members = selected
                .iter()
                .map(|entry| {
                    (
                        entry.path.to_string_lossy().replace('\\', "/"),
                        entry.is_dir,
                    )
                })
                .collect::<Vec<_>>();
            let output = self.temp_root.join(format!("copy-{}", Uuid::new_v4()));
            let password = self
                .archive_password
                .as_ref()
                .map(|value| value.to_string());
            if self.temp_extract.is_some() {
                self.status = "正在准备临时文件，请稍候".to_string();
                return;
            }
            self.start_temp_extract(
                archive,
                password,
                members,
                output,
                TempExtractMode::Copy,
                cx,
            );
            return;
        }

        let paths = selected
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        match platform::clipboard::set_clipboard_files(&paths) {
            Ok(()) => self.status = format!("已复制 {} 个项目", paths.len()),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 在后台解压归档条目到临时目录，并在主窗口显示可自动关闭的进度弹窗。
    pub(crate) fn start_temp_extract(
        &mut self,
        archive: PathBuf,
        password: Option<String>,
        members: Vec<(String, bool)>,
        output: PathBuf,
        mode: TempExtractMode,
        cx: &mut Context<Self>,
    ) {
        if self.temp_extract.is_some() {
            return;
        }
        let title = match mode {
            TempExtractMode::Copy => "正在准备复制文件",
            TempExtractMode::Drag { .. } => "正在准备拖出文件",
        };
        self.temp_extract = Some(TempExtractDialog {
            title: title.to_string(),
            message: "正在读取归档…".to_string(),
            progress: 0.,
        });
        self.status = title.to_string();
        cx.notify();

        let cleanup = output.clone();
        let (sender, receiver) = std::sync::mpsc::channel::<TempExtractEvent>();
        cx.background_executor()
            .spawn(async move {
                let result = extract_archive_members_with_progress(
                    &archive,
                    password.as_deref(),
                    &members,
                    &output,
                    |progress| {
                        let _ = sender.send(TempExtractEvent::Progress(progress));
                    },
                );
                let _ = sender.send(TempExtractEvent::Finished(result));
            })
            .detach();

        cx.spawn(async move |this, cx| {
            loop {
                let mut latest_progress = None;
                let mut finished = None;
                loop {
                    match receiver.try_recv() {
                        Ok(TempExtractEvent::Progress(progress)) => {
                            latest_progress = Some(progress);
                        }
                        Ok(TempExtractEvent::Finished(result)) => {
                            finished = Some(result);
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            finished = Some(Err("临时解压任务已中断".to_string()));
                            break;
                        }
                    }
                }
                if let Some(progress) = latest_progress
                    && this
                        .update_in(cx, |app, _, cx| {
                            if let Some(dialog) = &mut app.temp_extract {
                                dialog.progress = extract_progress_percent(&progress);
                                dialog.message = extract_progress_message(&progress);
                            }
                            cx.notify();
                        })
                        .is_err()
                {
                    break;
                }
                let Some(result) = finished else {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(80))
                        .await;
                    continue;
                };
                match mode {
                    TempExtractMode::Copy => {
                        let cleanup = cleanup.clone();
                        let _ = this.update_in(cx, |app, _, cx| {
                            app.temp_extract = None;
                            match result {
                                Ok(items) => {
                                    let paths =
                                        items.into_iter().map(|(path, _)| path).collect::<Vec<_>>();
                                    match platform::clipboard::set_clipboard_files(&paths) {
                                        Ok(()) => {
                                            app.status = format!(
                                                "已复制 {} 个项目，可在资源管理器中粘贴",
                                                paths.len()
                                            );
                                        }
                                        Err(error) => {
                                            app.status = error.to_string();
                                            let _ = std::fs::remove_dir_all(&cleanup);
                                        }
                                    }
                                }
                                Err(error) => {
                                    app.status = format!("准备复制文件失败：{error}");
                                    let _ = std::fs::remove_dir_all(&cleanup);
                                }
                            }
                            cx.notify();
                        });
                    }
                    TempExtractMode::Drag { hwnd } => {
                        let weak = this.clone();
                        let _ = this.update_in(cx, |app, window, cx| {
                            app.temp_extract = None;
                            match result {
                                Ok(items) => {
                                    let paths =
                                        items.into_iter().map(|(path, _)| path).collect::<Vec<_>>();
                                    app.status = format!("正在拖出 {} 个项目", paths.len());
                                    cx.notify();
                                    window.on_next_frame(move |window, _cx| {
                                        // 等进度弹窗先完整绘制一帧消失，再进入会阻塞 UI 的
                                        // 原生拖放循环，避免弹窗残留在拖拽画面上。
                                        window.on_next_frame(move |_window, cx| {
                                            let dropped =
                                                platform::drag::start_file_drag(hwnd, &paths)
                                                    .unwrap_or(false);
                                            let _ = weak.update(cx, |app, cx| {
                                                app.status = if dropped {
                                                    format!("已拖出 {} 个项目", paths.len())
                                                } else {
                                                    "拖出已取消".to_string()
                                                };
                                                cx.notify();
                                            });
                                        });
                                    });
                                }
                                Err(error) => {
                                    app.status = format!("准备拖出文件失败：{error}");
                                    cx.notify();
                                }
                            }
                        });
                    }
                }
                break;
            }
        })
        .detach();
    }

    /// 把所选文件系统路径或归档内部路径复制到剪贴板。
    pub(crate) fn copy_selected_path(&mut self, cx: &mut Context<Self>) {
        let path = self
            .selected_entry()
            .map(|entry| entry.path)
            .or_else(|| self.current_directory())
            .or_else(|| match &self.browser.location {
                BrowserLocation::Archive { archive, .. } => Some(archive.clone()),
                _ => None,
            });
        if let Some(path) = path {
            cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
            self.status = "路径已复制".to_string();
        }
    }

    /// 在 Windows 资源管理器中定位所选文件系统项目。
    pub(crate) fn reveal_selected(&mut self) {
        let path = match &self.browser.location {
            BrowserLocation::FileSystem(directory) => self
                .selected_entry()
                .map(|entry| entry.path)
                .unwrap_or_else(|| directory.clone()),
            BrowserLocation::Archive { archive, .. } => archive.clone(),
        };
        match platform::reveal(&path) {
            Ok(()) => self.status = "已在资源管理器中定位".to_string(),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 当浏览器不在归档内时返回当前文件系统目录。
    pub(crate) fn current_directory(&self) -> Option<PathBuf> {
        match &self.browser.location {
            BrowserLocation::FileSystem(path) => Some(path.clone()),
            BrowserLocation::Archive { .. } => None,
        }
    }

    /// 把当前目录保存为下次会话的起始位置。
    pub(crate) fn remember_directory(&self) {
        if let BrowserLocation::FileSystem(path) = &self.browser.location {
            let _ = self.settings.update(|settings| {
                settings.last_directory = Some(path.clone());
            });
        }
    }

    /// 导航到父目录或归档根目录。
    pub(crate) fn go_parent(&mut self) {
        if let Err(error) = self.browser.go_parent() {
            self.status = error.to_string();
        } else if matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.archive_password = None;
        }
    }

    /// 返回上一级，并把新位置同步到地址输入框。
    ///
    /// 工具栏的“上一级”按钮和文件表最上方的“..”行共用这个入口，
    /// 保证两个位置对归档根目录的处理保持一致。
    pub(crate) fn navigate_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.go_parent();
        set_input(
            &self.location_input,
            self.browser.current_display().as_str(),
            window,
            cx,
        );
    }

    /// 打开所选目录、归档、归档条目或普通文件。
    pub(crate) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.status = "请先选择一个项目".to_string();
            return;
        };
        if entry.is_dir {
            if let Err(error) = self.browser.enter_entry(&entry.id) {
                self.status = error.to_string();
            } else {
                self.search_results = None;
                set_input(
                    &self.location_input,
                    self.browser.current_display().as_str(),
                    window,
                    cx,
                );
            }
            return;
        }
        if matches!(&self.browser.location, BrowserLocation::Archive { .. }) {
            self.open_archive_entry(entry, window, cx);
            return;
        }
        if is_archive_path(&entry.path) {
            self.open_archive_with_auto_password(entry.path, ArchiveOpenIntent::Browse, window, cx);
            return;
        }
        let _ = std::process::Command::new("explorer.exe")
            .arg(&entry.path)
            .spawn();
    }

    /// 用内置只读文本查看器打开所选文件。
    ///
    /// 归档内的文件先按打开流程解压到临时目录，再由查看器窗口读取；
    /// 文件系统里的文件直接读取，失败原因写入状态栏。
    pub(crate) fn open_selected_in_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.status = "请先选择一个文件".to_string();
            return;
        };
        if entry.is_dir {
            self.status = "内置查看器只能打开文件".to_string();
            return;
        }
        if matches!(&self.browser.location, BrowserLocation::Archive { .. }) {
            self.view_archive_entry(entry, window, cx);
            return;
        }
        self.open_text_viewer(entry.path, entry.name, window, cx);
    }

    /// 在新的 Lumark 窗口中浏览所选文件夹。
    pub(crate) fn open_directory_window(&mut self, cx: &mut Context<Self>) {
        let Some(directory) = self
            .selected_entry()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.path)
        else {
            self.status = "请先选择一个文件夹".to_string();
            return;
        };
        if !matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.status = "只能在文件系统中打开新窗口".to_string();
            return;
        }
        self.spawn_window(Some(directory), None, cx);
    }

    /// 仅选中一个项目时返回聚焦的浏览器条目。
    pub(crate) fn selected_entry(&self) -> Option<BrowserEntry> {
        let id = self.browser.selection.focus()?;
        self.displayed_entries()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    /// 返回用户选择的归档路径（如果有）。
    pub(crate) fn selected_archive(&self) -> Option<PathBuf> {
        if let BrowserLocation::Archive { archive, .. } = &self.browser.location {
            return Some(archive.clone());
        }
        self.selected_entry()
            .filter(|entry| !entry.is_dir && is_archive_path(&entry.path))
            .map(|entry| entry.path)
    }

    /// 返回当前选择中可压缩的源路径。
    ///
    /// 当前文件表选择优先于拖放/命令行预留的源列表，避免上一次拖放留下的
    /// 单文件源覆盖用户新选中的多个项目。
    pub(crate) fn selected_sources(&self) -> Vec<PathBuf> {
        let ids = self.browser.selection.selected_ids();
        let mut paths: Vec<PathBuf> = self
            .displayed_entries()
            .iter()
            .filter(|entry| ids.contains(&entry.id))
            .map(|entry| entry.path.clone())
            .collect();
        if !paths.is_empty() {
            return paths;
        }
        if let Some(paths) = &self.dropped_sources {
            return paths.clone();
        }
        if let BrowserLocation::FileSystem(path) = &self.browser.location {
            paths.push(path.clone());
        }
        paths
    }

    /// 切换工作区页面。
    pub(crate) fn switch_screen(&mut self, screen: Screen) {
        self.screen = screen;
    }
}

/// 递归统计文件夹内普通文件的总字节数。
///
/// 跳过符号链接以避免目录环和重复计数；无权限读取的子项直接跳过，
/// 因此计算总会返回一个可展示的近似值而不是让界面长时间无反馈。
fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            total += directory_size(&entry.path());
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
        }
    }
    total
}

/// 把归档解压进度换算为弹窗进度条百分比。
fn extract_progress_percent(progress: &lumark_archive::ExtractProgress) -> f32 {
    if progress.entries_total > 0 {
        progress.entries_done as f32 * 100. / progress.entries_total as f32
    } else if progress.bytes_total > 0 {
        progress.bytes_done as f32 * 100. / progress.bytes_total as f32
    } else {
        0.
    }
}

/// 生成临时解压弹窗的状态文本。
fn extract_progress_message(progress: &lumark_archive::ExtractProgress) -> String {
    if progress.entries_total > 0 {
        format!(
            "正在解压 {}/{}：{}",
            progress.entries_done, progress.entries_total, progress.current
        )
    } else if !progress.current.is_empty() {
        format!("正在解压：{}", progress.current)
    } else {
        "正在解压…".to_string()
    }
}
