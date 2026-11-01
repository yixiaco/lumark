//! 归档条目拖出和临时解压。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use lumark_archive::OverwritePolicy as ArchiveOverwritePolicy;
use lumark_archive::{ArchiveEntry, ArchiveService, ExtractionOptions};
use lumark_platform_windows::virtual_drag::{VirtualFile, VirtualFileSource};

use super::*;
use crate::i18n::tr;

/// 归档条目拖出窗口时携带的数据；解析外部拖放时同步准备临时文件。
#[derive(Clone)]
pub(crate) struct ArchiveDragItem {
    /// 拖出来源归档路径。
    pub(crate) archive: PathBuf,
    /// 归档密码。
    pub(crate) password: Option<zeroize::Zeroizing<String>>,
    /// 选中的归档内路径。
    pub(crate) selected: Vec<(String, bool)>,
    /// 临时解压输出目录。
    #[allow(dead_code)]
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
///
/// 外部拖拽使用 Windows 原生拖拽光标；GPUI 预览会让图标停在应用边缘，
/// 因此这里保留一个不可见的 1×1 视图，只用于触发 GPUI 拖拽事件。
pub(super) struct ArchiveDragPreview;

/// 一个虚拟拖出条目：相对名称、归档内路径和是否为目录。
#[allow(dead_code)]
struct DragEntry {
    /// 归档内完整路径；合成目录为空。
    archive_path: Option<String>,
    /// 提供给 Windows 的描述符。
    virtual_file: VirtualFile,
}

/// 把归档条目映射成 Windows 虚拟文件数据源。
///
/// 目录会在启动拖动时递归展开，但真正的解压只发生在资源管理器请求某个文件
/// 内容时；因此取消拖动不会产生临时文件。
#[allow(dead_code)]
pub(crate) struct ArchiveVirtualFileSource {
    /// 来源归档。
    archive: PathBuf,
    /// 归档密码。
    password: Option<zeroize::Zeroizing<String>>,
    /// 按相对名称排序后的条目。
    entries: Vec<DragEntry>,
    /// 首次读取时一次性解压全部选中文件的缓存目录。
    extracted: Mutex<Option<tempfile::TempDir>>,
}

#[allow(dead_code)]
impl ArchiveVirtualFileSource {
    /// 从选中路径和归档完整条目构建数据源；没有有效条目时返回 `None`。
    pub(crate) fn new(
        archive: PathBuf,
        password: Option<zeroize::Zeroizing<String>>,
        selected: &[(String, bool)],
        entries: &[ArchiveEntry],
    ) -> Option<Self> {
        let selected = top_level_archive_paths(selected);
        if selected.is_empty() {
            return None;
        }
        let base = common_parent(&selected);
        let mut items: BTreeMap<String, DragEntry> = BTreeMap::new();
        for (path, is_dir) in &selected {
            let relative = strip_relative(path, &base);
            if *is_dir {
                items.entry(relative.clone()).or_insert_with(|| DragEntry {
                    archive_path: None,
                    virtual_file: VirtualFile {
                        name: relative.clone(),
                        size: 0,
                        is_dir: true,
                        modified: None,
                    },
                });
                let prefix = format!("{path}/");
                for entry in entries
                    .iter()
                    .filter(|entry| entry.path.starts_with(&prefix))
                {
                    let relative = strip_relative(&entry.path, &base);
                    items.insert(
                        relative.clone(),
                        DragEntry {
                            archive_path: (!entry.is_dir).then(|| entry.path.clone()),
                            virtual_file: VirtualFile {
                                name: relative,
                                size: entry.size,
                                is_dir: entry.is_dir,
                                modified: entry.modified.map(std::time::SystemTime::from),
                            },
                        },
                    );
                }
            } else {
                let entry = entries.iter().find(|entry| entry.path == *path);
                items.insert(
                    relative.clone(),
                    DragEntry {
                        archive_path: Some(path.clone()),
                        virtual_file: VirtualFile {
                            name: relative,
                            size: entry.map_or(0, |entry| entry.size),
                            is_dir: false,
                            modified: entry
                                .and_then(|entry| entry.modified)
                                .map(std::time::SystemTime::from),
                        },
                    },
                );
            }
        }
        // 为深层文件补齐父目录描述符，资源管理器会按描述符创建目录。
        let names: Vec<String> = items.keys().cloned().collect();
        for name in names {
            let mut parts: Vec<&str> = name.split('/').collect();
            parts.pop();
            let mut current = String::new();
            for part in parts {
                if !current.is_empty() {
                    current.push('/');
                }
                current.push_str(part);
                items.entry(current.clone()).or_insert_with(|| DragEntry {
                    archive_path: None,
                    virtual_file: VirtualFile {
                        name: current.clone(),
                        size: 0,
                        is_dir: true,
                        modified: None,
                    },
                });
            }
        }
        Some(Self {
            archive,
            password,
            entries: items.into_values().collect(),
            extracted: Mutex::new(None),
        })
    }
}

impl VirtualFileSource for ArchiveVirtualFileSource {
    /// 返回按相对名称排序的描述符列表。
    fn files(&self) -> Vec<VirtualFile> {
        self.entries
            .iter()
            .map(|entry| entry.virtual_file.clone())
            .collect()
    }

    /// 只解压被请求的单个文件，并移动到流指定的临时路径。
    ///
    /// 首次请求时把本次拖拽的全部文件一次性解压到缓存目录：资源管理器会为
    /// 多选/目录拖放连续请求多个流，逐文件重新解析 ASAR 等大 header 会让
    /// 投放阶段长时间无响应。
    fn extract(&self, index: usize, destination: &Path) -> Result<(), String> {
        let Some(entry) = self.entries.get(index) else {
            return Err("拖出条目索引越界".to_string());
        };
        if entry.virtual_file.is_dir {
            return Ok(());
        }
        let Some(archive_path) = &entry.archive_path else {
            return Err("拖出目录没有可解压内容".to_string());
        };
        let mut cache = self
            .extracted
            .lock()
            .map_err(|_| "拖出缓存锁已损坏".to_string())?;
        if cache.is_none() {
            let directory = tempfile::Builder::new()
                .prefix("lumark-drag-batch-")
                .tempdir()
                .map_err(|error| error.to_string())?;
            let selected_paths: Vec<String> = self
                .entries
                .iter()
                .filter_map(|entry| entry.archive_path.clone())
                .collect();
            if !selected_paths.is_empty() {
                let options = ExtractionOptions {
                    output_dir: directory.path().to_path_buf(),
                    selected_paths,
                    overwrite: true,
                    overwrite_policy: ArchiveOverwritePolicy::Overwrite,
                    password: self.password.clone(),
                    ..Default::default()
                };
                ArchiveService::extract(&self.archive, &options, |_| {})
                    .map_err(|error| error.to_string())?;
            }
            *cache = Some(directory);
        }
        let directory = cache
            .as_ref()
            .ok_or_else(|| "拖出缓存目录未初始化".to_string())?;
        let extracted = lumark_archive::safe_join(directory.path(), archive_path)
            .map_err(|error| error.to_string())?;
        if extracted != destination {
            std::fs::copy(&extracted, destination)
                .map_err(|error| format!("复制拖出文件失败：{error}"))?;
        }
        Ok(())
    }
}

/// 计算选中顶层路径父目录的公共前缀。
#[allow(dead_code)]
fn common_parent(selected: &[(String, bool)]) -> String {
    let mut parents: Vec<Vec<&str>> = selected
        .iter()
        .map(|(path, _)| {
            let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
            parts.pop();
            parts
        })
        .collect();
    let Some(mut prefix) = parents.pop() else {
        return String::new();
    };
    for parts in parents {
        let shared = prefix
            .iter()
            .zip(parts.iter())
            .take_while(|(left, right)| left == right)
            .count();
        prefix.truncate(shared);
    }
    prefix.join("/")
}

/// 去掉公共父目录前缀，得到拖出时的相对名称。
#[allow(dead_code)]
fn strip_relative(path: &str, base: &str) -> String {
    if base.is_empty() {
        return path.trim_start_matches('/').to_string();
    }
    path.strip_prefix(base)
        .map(|value| value.trim_start_matches('/'))
        .unwrap_or(path)
        .to_string()
}

impl Render for ArchiveDragPreview {
    /// 不在应用内绘制拖拽预览，交给 Windows 原生拖拽光标。
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(1.)).h(px(1.)).opacity(0.)
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
        (BrowserLocation::GameArchives { .. }, _) => return None,
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
        format!("{} {}", selected.len(), tr("个项目", "items"))
    };
    Some(ArchiveDragItem {
        archive: archive.clone(),
        password: app.archive_operation_password(),
        selected,
        output: app.temp_root.join(format!("drag-{}", uuid::Uuid::new_v4())),
        label,
    })
}

impl LumarkApp {
    /// 开始异步准备拖出文件；解压完成后自动关闭进度弹窗并启动原生拖放。
    #[allow(dead_code)]
    pub(super) fn start_archive_drag_extract(
        &mut self,
        item: ArchiveDragItem,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        use raw_window_handle::RawWindowHandle;

        let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
            self.status = tr("无法获取窗口句柄", "Unable to get window handle").to_string();
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

    /// 在真正离开窗口时按当前选择重建拖拽项。
    ///
    /// 行的 `on_drag` 负载是在渲染时生成的；如果用户在按下后立即拖动，
    /// 该负载可能还是上一帧的单选。离开窗口时重新读取选择可以保证多选和
    /// 文件名与当前界面一致。
    fn refresh_drag_item(&self, fallback: &ArchiveDragItem) -> ArchiveDragItem {
        let entries = self.displayed_entries();
        let selected = self
            .browser
            .selection
            .selected_ids()
            .into_iter()
            .filter_map(|id| entries.iter().find(|entry| entry.id == id).cloned())
            .map(|entry| {
                (
                    entry.path.to_string_lossy().replace('\\', "/"),
                    entry.is_dir,
                )
            })
            .collect::<Vec<_>>();
        let selected = top_level_archive_paths(&selected);
        let selected = if selected.is_empty() {
            fallback.selected.clone()
        } else {
            selected
        };
        let label = if selected.len() == 1 {
            fallback.label.clone()
        } else {
            format!("{} {}", selected.len(), tr("个项目", "items"))
        };
        ArchiveDragItem {
            archive: fallback.archive.clone(),
            password: fallback.password.clone(),
            selected,
            output: fallback.output.clone(),
            label,
        }
    }

    /// 指针离开窗口时把手势交给 Windows 原生拖拽。
    ///
    /// 只触发一次；真正的解压由资源管理器在投放后通过 `IStream` 请求数据时
    /// 执行。若虚拟拖出启动失败，回退到原来的“先解压再拖出”流程。
    pub(super) fn handle_archive_drag_move(
        &mut self,
        item: &ArchiveDragItem,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.virtual_drag_active {
            return;
        }
        let viewport = window.viewport_size();
        if position.x >= px(0.)
            && position.x <= viewport.width
            && position.y >= px(0.)
            && position.y <= viewport.height
        {
            return;
        }
        let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
            self.status = tr("无法获取窗口句柄", "Unable to get window handle").to_string();
            return;
        };
        let raw_window_handle::RawWindowHandle::Win32(raw) = handle.as_raw() else {
            return;
        };
        let hwnd = raw.hwnd.get();
        let item = self.refresh_drag_item(item);
        self.virtual_drag_active = true;
        self.status = tr(
            "松开鼠标后解压到目标文件夹",
            "Release the mouse to extract into the target folder",
        )
        .to_string();
        cx.stop_active_drag(window);
        let app = cx.entity();
        window.on_next_frame(move |window, cx| {
            let gesture = platform::virtual_drag::start_drag_gesture(hwnd);
            app.update(cx, |app, cx| {
                app.finish_external_drag(item, gesture, window, cx);
            });
        });
    }

    /// 原生拖拽结束后解析投放目录，并把解压提交为带进度窗口的后台任务。
    fn finish_external_drag(
        &mut self,
        item: ArchiveDragItem,
        gesture: Result<platform::virtual_drag::DragGestureOutcome, platform::PlatformError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.virtual_drag_active = false;
        let outcome = match gesture {
            Ok(outcome) => outcome,
            Err(error) => {
                self.status = format!(
                    "{}{}",
                    tr("拖拽手势启动失败：", "Unable to start drag gesture: "),
                    error
                );
                cx.notify();
                return;
            }
        };
        if outcome.cancelled {
            self.status = tr("已取消拖出", "Drag cancelled").to_string();
            cx.notify();
            return;
        }
        let Some(target) = platform::external_drop::current_target() else {
            self.status = tr(
                "已忽略：目标不是桌面或文件管理器",
                "Ignored: target is not the desktop or a file manager",
            )
            .to_string();
            cx.notify();
            return;
        };
        if item.selected.is_empty() {
            self.status = tr("没有可解压的条目", "No entries to extract").to_string();
            cx.notify();
            return;
        }
        let archive_parent = item
            .archive
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let archive_name = item.archive.file_name().and_then(|value| value.to_str());
        if target == archive_parent
            && archive_name.is_some_and(|name| {
                item.selected
                    .iter()
                    .any(|(path, _)| path.split('/').next() == Some(name))
            })
        {
            self.status = tr(
                "已忽略：解压目标会覆盖归档自身",
                "Ignored: extraction would overwrite the archive itself",
            )
            .to_string();
            cx.notify();
            return;
        }
        let policy = match self.settings.get().overwrite_policy {
            lumark_settings::OverwritePolicy::Overwrite => ArchiveOverwritePolicy::Overwrite,
            lumark_settings::OverwritePolicy::Skip => ArchiveOverwritePolicy::Skip,
            lumark_settings::OverwritePolicy::Rename => ArchiveOverwritePolicy::Rename,
            lumark_settings::OverwritePolicy::Ask => ArchiveOverwritePolicy::Rename,
        };
        let has_conflict = item.selected.iter().any(|(path, _)| {
            lumark_archive::safe_join(&target, path).is_ok_and(|destination| destination.exists())
        });
        if has_conflict {
            self.status = tr(
                "目标已有同名文件，请选择覆盖、跳过或重命名",
                "Same-named files exist; choose overwrite, skip or rename",
            )
            .to_string();
            self.open_drag_conflict_dialog(item, target, window, cx);
            return;
        }
        self.submit_drag_extract(item, target, policy, cx);
    }

    /// 目标已有同名文件时让用户选择覆盖、跳过或重命名。
    fn open_drag_conflict_dialog(
        &mut self,
        item: ArchiveDragItem,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity();
        let target_label = target.display().to_string();
        window.open_dialog(cx, move |dialog, _, cx| {
            let overwrite_item = item.clone();
            let overwrite_target = target.clone();
            let skip_item = item.clone();
            let skip_target = target.clone();
            let rename_item = item.clone();
            let rename_target = target.clone();
            let app_overwrite = app.clone();
            let app_skip = app.clone();
            let app_rename = app.clone();
            let app_cancel = app.clone();
            dialog
                .title(tr(
                    "目标已有同名文件",
                    "The destination already contains same-named files",
                ))
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}：{}", tr("解压到", "Extract to"), target_label)),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("drag-conflict-cancel")
                                .secondary()
                                .cursor(gpui_kit::CursorStyle::PointingHand)
                                .label(tr("取消", "Cancel"))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    app_cancel.update(cx, |app, cx| {
                                        app.status =
                                            tr("已取消解压", "Extraction cancelled").to_string();
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            button("drag-conflict-skip")
                                .secondary()
                                .cursor(gpui_kit::CursorStyle::PointingHand)
                                .label(tr("跳过", "Skip"))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let item = skip_item.clone();
                                    let target = skip_target.clone();
                                    app_skip.update(cx, |app, cx| {
                                        app.submit_drag_extract(
                                            item,
                                            target,
                                            ArchiveOverwritePolicy::Skip,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            button("drag-conflict-rename")
                                .secondary()
                                .cursor(gpui_kit::CursorStyle::PointingHand)
                                .label(tr("重命名", "Rename"))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let item = rename_item.clone();
                                    let target = rename_target.clone();
                                    app_rename.update(cx, |app, cx| {
                                        app.submit_drag_extract(
                                            item,
                                            target,
                                            ArchiveOverwritePolicy::Rename,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            button("drag-conflict-overwrite")
                                .primary()
                                .cursor(gpui_kit::CursorStyle::PointingHand)
                                .label(tr("覆盖", "Overwrite"))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let item = overwrite_item.clone();
                                    let target = overwrite_target.clone();
                                    app_overwrite.update(cx, |app, cx| {
                                        app.submit_drag_extract(
                                            item,
                                            target,
                                            ArchiveOverwritePolicy::Overwrite,
                                            cx,
                                        );
                                    });
                                }),
                        ),
                )
        });
    }

    /// 提交拖放解压任务；进度窗口由任务事件自动打开。
    fn submit_drag_extract(
        &mut self,
        item: ArchiveDragItem,
        target: PathBuf,
        policy: ArchiveOverwritePolicy,
        cx: &mut Context<Self>,
    ) {
        let options = ExtractionOptions {
            output_dir: target.clone(),
            selected_paths: item.selected.iter().map(|(path, _)| path.clone()).collect(),
            overwrite: policy != ArchiveOverwritePolicy::Skip,
            overwrite_policy: policy,
            password: item.password,
            ..Default::default()
        };
        let id = self.tasks.submit(lumark_tasks::TaskRequest::Extract {
            archive: item.archive,
            options,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = format!(
            "{}{}",
            tr("正在解压到：", "Extracting to: "),
            target.display()
        );
        cx.notify();
    }
}

/// 生成路径栏的内部拖放处理器。
///
/// 释放时把归档条目解压到当前显示的真实文件夹；归档内部路径没有可写目标，
/// 由动作层忽略并提示。
pub(crate) fn path_bar_drop_handler(
    app: gpui_kit::Entity<LumarkApp>,
) -> impl Fn(&ArchiveDragItem, &mut Window, &mut gpui_kit::App) + 'static {
    move |item, window, cx| {
        let item = item.clone();
        app.update(cx, |app, cx| {
            app.handle_archive_drop_on_path_bar(item, window, cx);
            cx.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_archive::{ArchiveFormat, CompressionOptions};

    /// 创建一个包含目录和文本文件的测试 ZIP 并返回归档与条目。
    fn archive_with_directory() -> (PathBuf, tempfile::TempDir, Vec<ArchiveEntry>) {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        std::fs::create_dir_all(source.join("docs")).unwrap();
        std::fs::write(source.join("docs").join("hello.txt"), b"hello").unwrap();
        let archive = directory.path().join("sample.zip");
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format: ArchiveFormat::Zip,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let info = ArchiveService::list(&archive, None).unwrap();
        (archive, directory, info.entries)
    }

    /// 验证目录展开、相对名称和父目录补全。
    #[test]
    fn expands_directory_and_names_relative_to_parent() {
        let (archive, _guard, entries) = archive_with_directory();
        let source = ArchiveVirtualFileSource::new(
            archive,
            None,
            &[("source/docs".to_string(), true)],
            &entries,
        )
        .unwrap();
        let names: Vec<String> = source.files().into_iter().map(|file| file.name).collect();
        assert_eq!(names, vec!["docs", "docs/hello.txt"]);
        assert!(source.files()[0].is_dir);
    }

    /// 验证首次请求时批量解压，并只把被请求的文件复制到流路径。
    #[test]
    fn extracts_requested_file_from_batch_cache() {
        let (archive, guard, entries) = archive_with_directory();
        let source = ArchiveVirtualFileSource::new(
            archive,
            None,
            &[("source/docs".to_string(), true)],
            &entries,
        )
        .unwrap();
        let index = source
            .files()
            .iter()
            .position(|file| file.name.ends_with("hello.txt"))
            .unwrap();
        let destination = guard.path().join("payload");
        source.extract(index, &destination).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"hello");
    }

    /// 验证公共父目录和相对名称计算。
    #[test]
    fn computes_common_parent_and_relative_names() {
        let selected = vec![
            ("docs/a.txt".to_string(), false),
            ("docs/b.txt".to_string(), false),
        ];
        let base = common_parent(&selected);
        assert_eq!(base, "docs");
        assert_eq!(strip_relative("docs/a.txt", &base), "a.txt");

        let selected = vec![("docs".to_string(), true)];
        assert_eq!(common_parent(&selected), "");
        assert_eq!(strip_relative("docs/a.txt", ""), "docs/a.txt");
    }
}
