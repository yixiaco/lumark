//! 归档条目拖出和临时解压。

use super::*;

/// 归档条目拖出窗口时携带的数据；解析外部拖放时同步准备临时文件。
#[derive(Clone)]
pub(super) struct ArchiveDragItem {
    /// 拖出来源归档路径。
    pub(super) archive: PathBuf,
    /// 归档密码。
    pub(super) password: Option<zeroize::Zeroizing<String>>,
    /// 选中的归档内路径。
    pub(super) selected: Vec<(String, bool)>,
    /// 临时解压输出目录。
    pub(super) output: PathBuf,
    /// 拖拽提示文本。
    pub(super) label: String,
}

impl ArchiveDragItem {
    /// 把归档条目解压到窗口临时目录，返回可交给原生拖放的真实路径。
    #[cfg(test)]
    pub(super) fn materialize_paths(&self) -> Option<Vec<PathBuf>> {
        extract_archive_members(
            &self.archive,
            self.password.as_ref().map(|value| value.as_str()),
            &self.selected,
            &self.output,
        )
        .ok()
        .map(|items| items.into_iter().map(|(path, _)| path).collect())
    }
}

/// 拖出归档条目时跟随鼠标的轻量预览。
pub(super) struct ArchiveDragPreview {
    /// 拖拽预览文本。
    pub(super) label: String,
}

impl Render for ArchiveDragPreview {
    /// 渲染文件名或“N 个项目”的拖放预览。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .items_center()
            .rounded_md()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_lg()
            .child(AppIcon::FileZip.element(16.))
            .child(div().text_sm().child(self.label.clone()))
    }
}

/// 读取并缓存 Windows Shell 文件类型图标；失败时返回 `None` 交给 IconPark 回退。
pub(super) fn system_icon_for(
    app: &LumarkApp,
    entry: &BrowserEntry,
) -> Option<Arc<gpui_kit::Image>> {
    if entry.encrypted {
        return None;
    }
    let inside_archive = matches!(&app.browser.location, BrowserLocation::Archive { .. });
    let key = match (&app.browser.location, entry.is_dir) {
        (BrowserLocation::Archive { .. }, true) => "archive-dir".to_string(),
        (BrowserLocation::Archive { .. }, false) => format!(
            "archive-ext:{}",
            entry
                .path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
        ),
        (BrowserLocation::FileSystem(_), _) => format!("path:{}", entry.path.display()),
    };
    if let Some(cached) = app.system_icon_cache.borrow().get(&key) {
        return cached.clone();
    }
    let query = if inside_archive {
        if entry.is_dir {
            PathBuf::from("folder")
        } else {
            PathBuf::from(&entry.name)
        }
    } else {
        entry.path.clone()
    };
    let image = platform::icons::file_icon_png(&query, entry.is_dir, inside_archive).map(|bytes| {
        let id = gpui_kit::hash(&bytes);
        Arc::new(gpui_kit::Image {
            format: gpui_kit::ImageFormat::Png,
            bytes,
            id,
        })
    });
    app.system_icon_cache
        .borrow_mut()
        .insert(key, image.clone());
    image
}

/// 为当前拖拽条目构建归档选择快照；未选中时只拖当前条目。
pub(super) fn archive_drag_item(app: &LumarkApp, entry: &BrowserEntry) -> Option<ArchiveDragItem> {
    let BrowserLocation::Archive { archive, .. } = &app.browser.location else {
        return None;
    };
    let entries = app.displayed_entries();
    let selected = if app.browser.selection.is_selected(&entry.id) {
        app.browser
            .selection
            .selected_ids()
            .into_iter()
            .filter_map(|id| entries.iter().find(|candidate| candidate.id == id).cloned())
            .collect::<Vec<_>>()
    } else {
        vec![entry.clone()]
    };
    let selected = top_level_archive_paths(
        &selected
            .iter()
            .map(|item| (item.path.to_string_lossy().replace('\\', "/"), item.is_dir))
            .collect::<Vec<_>>(),
    );
    if selected.is_empty() {
        return None;
    }
    let label = if selected.len() == 1 {
        entry.name.clone()
    } else {
        format!("{} 个项目", selected.len())
    };
    Some(ArchiveDragItem {
        archive: archive.clone(),
        password: app
            .archive_password
            .as_ref()
            .map(|value| zeroize::Zeroizing::new(value.to_string())),
        selected,
        output: app.temp_root.join(format!("drag-{}", uuid::Uuid::new_v4())),
        label,
    })
}

impl LumarkApp {
    /// 开始异步准备拖出文件；解压完成后自动关闭进度弹窗并启动原生拖放。
    pub(super) fn start_archive_drag_extract(
        &mut self,
        item: ArchiveDragItem,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        use raw_window_handle::RawWindowHandle;

        let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
            self.status = "无法获取窗口句柄".to_string();
            return;
        };
        let RawWindowHandle::Win32(raw) = handle.as_raw() else {
            return;
        };
        let password = item.password.map(|value| value.to_string());
        self.start_temp_extract(
            item.archive,
            password,
            item.selected,
            item.output,
            TempExtractMode::Drag {
                hwnd: raw.hwnd.get(),
            },
            cx,
        );
    }
}
