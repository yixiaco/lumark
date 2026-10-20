use super::super::*;

impl LumarkApp {
    /// Reloads the active filesystem or archive location and clears stale search results.
    pub(crate) fn refresh(&mut self) {
        self.dropped_sources = None;
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

    /// Returns the filtered search result or the browser's current entries.
    pub(crate) fn displayed_entries(&self) -> Vec<BrowserEntry> {
        self.search_results
            .clone()
            .unwrap_or_else(|| self.browser.entries.clone())
    }

    /// Returns the number of rows currently visible to the virtual list.
    pub(crate) fn displayed_count(&self) -> usize {
        self.search_results
            .as_ref()
            .map(Vec::len)
            .unwrap_or(self.browser.entries.len())
    }

    /// Returns a cloned display entry for a stable virtual-row index.
    pub(crate) fn displayed_entry_at(&self, index: usize) -> Option<BrowserEntry> {
        self.search_results
            .as_ref()
            .map_or_else(
                || self.browser.entries.get(index),
                |entries| entries.get(index),
            )
            .cloned()
    }

    /// Returns stable IDs in display order for range selection.
    pub(crate) fn displayed_ids(&self) -> Vec<String> {
        self.search_results
            .as_ref()
            .unwrap_or(&self.browser.entries)
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// Selects every item in the current filtered view.
    pub(crate) fn select_all_displayed(&mut self) {
        let ids = self.displayed_ids();
        self.browser.selection.select_all(&ids);
    }

    /// Navigates to the path entered in the location field.
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

    /// Navigates to a directory or archive path and synchronizes the location input.
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

    /// Flattens the expanded directory tree into rows for the sidebar.
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

    /// Appends visible descendants of one expanded tree node.
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

    /// Expands or collapses a directory node and lazily loads its children.
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

    /// Executes the current name search against the active filesystem or archive.
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

    /// Clears the active search result and returns to the normal browser listing.
    pub(crate) fn clear_search(&mut self) {
        self.search_results = None;
        self.status = format!("{} 个项目", self.browser.entries.len());
    }

    /// Toggles the details inspector and persists the preference.
    pub(crate) fn toggle_details(&mut self) {
        self.details_visible = !self.details_visible;
        let visible = self.details_visible;
        let _ = self
            .settings
            .update(|settings| settings.details_visible = visible);
    }

    /// Cycles file-list column widths and persists the chosen layout.
    pub(crate) fn cycle_column_layout(&mut self) {
        self.column_layout = (self.column_layout + 1) % 3;
        let layout = self.column_layout;
        let _ = self
            .settings
            .update(|settings| settings.column_layout = layout);
    }

    /// Routes dropped files to open, compress, or copy actions based on their count and type.
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

    /// Processes a command request forwarded by the single-instance shell integration.
    pub(crate) fn process_request(
        &mut self,
        arguments: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut mode = "open";
        let mut paths = Vec::new();
        for argument in arguments {
            match argument.as_str() {
                "--open" => mode = "open",
                "--extract" => mode = "extract",
                "--compress" => mode = "compress",
                value if value.starts_with('-') => {}
                _ => paths.push(PathBuf::from(argument)),
            }
        }
        if mode == "compress" && !paths.is_empty() {
            self.dropped_sources = Some(paths);
            self.open_compress_dialog(window, cx);
        } else if let Some(path) = paths.first().filter(|path| path.is_file()) {
            let intent = if mode == "extract" {
                ArchiveOpenIntent::Extract
            } else {
                ArchiveOpenIntent::Browse
            };
            self.open_archive_with_auto_password(path.clone(), intent, window, cx);
        }
    }

    /// Adds dropped files to the open archive through the background edit pipeline.
    pub(crate) fn add_files_to_current_archive(&mut self, paths: &[PathBuf]) {
        let BrowserLocation::Archive { archive, .. } = &self.browser.location else {
            return;
        };
        let archive = archive.clone();
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
        let options = EditOptions {
            source_password: self.archive_password.clone(),
            additions,
            keep_backup: self.settings.get().archive_edit_backup,
            ..Default::default()
        };
        self.submit_archive_edit(archive, options, PendingEditAction::Add);
    }

    /// Queues an archive edit and keeps the original file intact until the replacement succeeds.
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

    /// Reports a completed archive edit and refreshes the archive view.
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

    /// Returns the distinct paths represented by the current selection.
    pub(crate) fn selected_paths(&self) -> Vec<PathBuf> {
        let ids = self.browser.selection.selected_ids();
        self.displayed_entries()
            .into_iter()
            .filter(|entry| ids.contains(&entry.id))
            .map(|entry| entry.path)
            .collect()
    }

    /// Opens the appropriate recycle-bin or archive-removal confirmation.
    pub(crate) fn request_delete(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status = "请先选择要删除的项目".to_string();
            return;
        }
        self.pending_delete_paths = paths;
        self.show_delete_confirm = true;
    }

    /// Selects the clicked row and opens its context menu at the pointer.
    pub(crate) fn show_file_context_menu(
        &mut self,
        target_id: &str,
        position: gpui_kit::Point<gpui_kit::Pixels>,
    ) {
        if !self.browser.selection.is_selected(target_id) {
            self.browser.selection.select_only(target_id);
        }
        self.context_menu_position = Some(position);
        self.show_context_menu = true;
    }

    /// Deletes files through the recycle bin or submits an archive removal edit.
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

    /// Opens the new-folder dialog with a clean name field.
    pub(crate) fn start_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.status = "只能在文件系统目录中新建文件夹".to_string();
            return;
        }
        set_input(&self.new_folder_name, "", window, cx);
        self.show_new_folder = true;
    }

    /// Creates a validated folder in a filesystem location or the current archive.
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

    /// Opens the rename dialog prefilled with the selected item's name.
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

    /// Validates and applies a filesystem rename or archive-entry rename.
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

    /// Copies the selected filesystem paths or archive-internal paths to the clipboard.
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

    /// Reveals the selected filesystem item in Windows Explorer.
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

    /// Returns the active filesystem directory when the browser is not inside an archive.
    pub(crate) fn current_directory(&self) -> Option<PathBuf> {
        match &self.browser.location {
            BrowserLocation::FileSystem(path) => Some(path.clone()),
            BrowserLocation::Archive { .. } => None,
        }
    }

    /// Persists the current directory as the next session's starting location.
    pub(crate) fn remember_directory(&self) {
        if let BrowserLocation::FileSystem(path) = &self.browser.location {
            let _ = self.settings.update(|settings| {
                settings.last_directory = Some(path.clone());
            });
        }
    }

    /// Adds a bounded diagnostic line to the task-center log.
    pub(crate) fn push_task_log(&mut self, message: String) {
        self.task_log.push(message);
        if self.task_log.len() > 100 {
            self.task_log.remove(0);
        }
    }

    /// Navigates to the parent directory or the archive root.
    pub(crate) fn go_parent(&mut self) {
        if let Err(error) = self.browser.go_parent() {
            self.status = error.to_string();
        } else if matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.archive_password = None;
        }
    }

    /// Opens the selected directory, archive, archive entry, or regular file.
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

    /// Returns the focused browser entry when exactly one item is selected.
    pub(crate) fn selected_entry(&self) -> Option<BrowserEntry> {
        let id = self.browser.selection.focus()?;
        self.displayed_entries()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    /// Returns the archive path selected by the user, if any.
    pub(crate) fn selected_archive(&self) -> Option<PathBuf> {
        if let BrowserLocation::Archive { archive, .. } = &self.browser.location {
            return Some(archive.clone());
        }
        self.selected_entry()
            .filter(|entry| !entry.is_dir && is_archive_path(&entry.path))
            .map(|entry| entry.path)
    }

    /// Returns compressible source paths from the current selection.
    pub(crate) fn selected_sources(&self) -> Vec<PathBuf> {
        if let Some(paths) = &self.dropped_sources {
            return paths.clone();
        }
        let ids = self.browser.selection.selected_ids();
        let mut paths: Vec<PathBuf> = self
            .displayed_entries()
            .iter()
            .filter(|entry| ids.contains(&entry.id))
            .map(|entry| entry.path.clone())
            .collect();
        if paths.is_empty()
            && let BrowserLocation::FileSystem(path) = &self.browser.location
        {
            paths.push(path.clone());
        }
        paths
    }

    /// Changes the workspace screen while guarding uncommitted password edits.
    pub(crate) fn switch_screen(&mut self, screen: Screen) {
        if self.screen == Screen::Passwords
            && self.password_draft_dirty
            && screen != Screen::Passwords
        {
            self.pending_screen = Some(screen);
            self.show_discard_password_draft = true;
            return;
        }
        self.screen = screen;
        if screen == Screen::Passwords {
            self.ensure_password_draft();
        }
    }
}
