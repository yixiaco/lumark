use super::super::*;
use super::components::{AppIcon, entry_icon, entry_icon_color, hint_text, icon_button};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;

/// 归档视图在名称、大小、修改时间之外追加的列。
#[derive(Clone, Copy, Default)]
struct ArchiveColumns {
    compressed: bool,
    algorithm: bool,
    crc32: bool,
    path: bool,
}

const COMPRESSED_WIDTH: f32 = 80.;
const ALGORITHM_WIDTH: f32 = 76.;
const CRC_WIDTH: f32 = 72.;
const PATH_WIDTH: f32 = 150.;

/// 按文件表可用宽度选择归档列；窄窗口优先隐藏路径、压缩算法和 CRC32。
///
/// 阈值按“名称至少保留 160 像素”估算，保证 960 像素最小窗口下仍能显示
/// 压缩后大小和 CRC32；隐藏哪些列只影响显示，不影响条目数据。
fn archive_columns(width: f32) -> ArchiveColumns {
    ArchiveColumns {
        compressed: width >= 530.,
        crc32: width >= 600.,
        algorithm: width >= 680.,
        path: width >= 840.,
    }
}

/// 右键菜单按位置和项目类型区分的动作集合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileMenuKind {
    /// 归档内的文件夹。
    ArchiveFolder,
    /// 归档内的文件。
    ArchiveFile,
    /// 文件系统中的文件夹。
    Folder,
    /// 文件系统中的压缩包。
    ArchiveFileSystem,
    /// 文件系统中的可执行文件。
    ExecutableFile,
    /// 文件系统中的普通文件。
    File,
}

/// 归档条目拖出窗口时携带的数据；解析外部拖放时同步准备临时文件。
#[derive(Clone)]
struct ArchiveDragItem {
    archive: PathBuf,
    password: Option<zeroize::Zeroizing<String>>,
    selected: Vec<(String, bool)>,
    output: PathBuf,
    label: String,
}

impl ArchiveDragItem {
    /// 把归档条目解压到窗口临时目录，返回可交给原生拖放的真实路径。
    fn materialize_paths(&self) -> Option<Vec<PathBuf>> {
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
struct ArchiveDragPreview {
    label: String,
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
fn system_icon_for(app: &mut LumarkApp, entry: &BrowserEntry) -> Option<Arc<gpui_kit::Image>> {
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
    if let Some(cached) = app.system_icon_cache.get(&key) {
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
    app.system_icon_cache.insert(key, image.clone());
    image
}

/// 在 GPUI 的拖拽起始回调中同步准备临时文件，并交给 Windows 原生拖放循环。
fn start_archive_native_drag(item: &ArchiveDragItem, window: &Window) {
    use raw_window_handle::RawWindowHandle;

    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return;
    };
    let Some(paths) = item.materialize_paths() else {
        return;
    };
    let _ = platform::drag::start_file_drag(raw.hwnd.get(), &paths);
}

/// 为当前拖拽条目构建归档选择快照；未选中时只拖当前条目。
fn archive_drag_item(app: &LumarkApp, entry: &BrowserEntry) -> Option<ArchiveDragItem> {
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

/// 根据当前位置和条目类型选择右键菜单。
fn file_menu_kind(location: &BrowserLocation, entry: &BrowserEntry) -> FileMenuKind {
    match location {
        BrowserLocation::Archive { .. } => {
            if entry.is_dir {
                FileMenuKind::ArchiveFolder
            } else {
                FileMenuKind::ArchiveFile
            }
        }
        BrowserLocation::FileSystem(_) => {
            if entry.is_dir {
                FileMenuKind::Folder
            } else if is_archive_path(&entry.path) {
                FileMenuKind::ArchiveFileSystem
            } else if is_executable_like(&entry.path) {
                FileMenuKind::ExecutableFile
            } else {
                FileMenuKind::File
            }
        }
    }
}

impl LumarkApp {
    /// 渲染主文件表，并在搜索结果激活时显示一条结果摘要。
    ///
    /// 选择按稳定条目 ID 跟踪，而不是行索引，因此排序、过滤和虚拟列表回收
    /// 都不会让选择静默跳到其他文件。详细信息已移到侧栏下半部分，
    /// 文件表因此可以使用窗口的全部剩余宽度。
    pub(crate) fn render_browser(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entry_count = self.displayed_count();
        let search_active = self.search_results.is_some();
        let window_width = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .width
            .as_f32();
        let sidebar_width = if self.sidebar_visible { 252. } else { 0. };
        let main_width = (window_width - sidebar_width - 24.).max(420.);
        let (size_width, modified_width, kind_width) = column_layout_widths(self.column_layout);
        let inside_archive = matches!(&self.browser.location, BrowserLocation::Archive { .. });
        let archive_columns = if inside_archive {
            archive_columns(main_width)
        } else {
            ArchiveColumns::default()
        };
        let tile_view = self.tile_view;
        let tile_columns = if tile_view {
            ((main_width - 24.0) / 132.0).floor().max(1.0) as usize
        } else {
            1
        };
        let virtual_rows = if tile_view {
            entry_count.div_ceil(tile_columns)
        } else {
            entry_count
        };
        let row_height = if tile_view { px(132.) } else { px(31.) };
        let item_sizes = Rc::new(vec![
            gpui_kit::size(px(main_width), row_height);
            virtual_rows
        ]);
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();

        v_flex()
            .id("browser-page")
            .w_full()
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(cx.theme().table)
            .track_focus(&self.browser_focus)
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if keystroke.modifiers.control && keystroke.key.eq_ignore_ascii_case("a") {
                        this.select_all_displayed();
                        cx.notify();
                    } else if keystroke.modifiers.control && keystroke.key.eq_ignore_ascii_case("c")
                    {
                        if this.browser_focus.is_focused(window) {
                            this.copy_selected_items(cx);
                        }
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("escape") {
                        if search_active {
                            this.clear_search();
                        } else {
                            this.browser.selection.clear();
                        }
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("enter") {
                        this.open_selected(window, cx);
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("delete") {
                        this.request_delete();
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("f2") {
                        this.start_rename(window, cx);
                        cx.notify();
                    }
                }),
            )
            .on_drop(
                cx.listener(|this, paths: &gpui_kit::ExternalPaths, window, cx| {
                    let paths = paths.0.to_vec();
                    this.handle_external_drop(&paths, window, cx);
                    cx.notify();
                }),
            )
            .flex_1()
            .when(search_active, |this| {
                this.child(
                    h_flex()
                        .h(px(30.))
                        .w_full()
                        .flex_shrink_0()
                        .px_3()
                        .gap_2()
                        .items_center()
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .text_xs()
                        .child(AppIcon::Search.element(13.))
                        .child(format!(
                            "搜索结果 · {entry_count} 项 · {}",
                            format_size(total_size)
                        ))
                        .child(
                            button("search-result-clear")
                                .link()
                                .xsmall()
                                .ml_auto()
                                .label("清除搜索")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_search();
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .h(px(30.))
                    .flex_shrink_0()
                    .gap_2()
                    .px_3()
                    .items_center()
                    .bg(cx.theme().list_head)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(22.)).flex_shrink_0())
                    .child(if tile_view {
                        div().flex_1().child("名称与类型").into_any_element()
                    } else {
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child("名称"))
                            .child(div().w(px(size_width)).child("大小"))
                            .when(archive_columns.compressed, |this| {
                                this.child(div().w(px(COMPRESSED_WIDTH)).child("压缩后大小"))
                            })
                            .child(div().w(px(modified_width)).child("修改时间"))
                            .when(archive_columns.algorithm, |this| {
                                this.child(div().w(px(ALGORITHM_WIDTH)).child("压缩算法"))
                            })
                            .when(archive_columns.crc32, |this| {
                                this.child(div().w(px(CRC_WIDTH)).child("CRC32"))
                            })
                            .when(archive_columns.path, |this| {
                                this.child(div().w(px(PATH_WIDTH)).child("路径"))
                            })
                            .when(!inside_archive, |this| {
                                this.child(div().w(px(kind_width)).child("类型"))
                            })
                            .into_any_element()
                    }),
            )
            .when(entry_count == 0, |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .p_8()
                        .child(
                            div()
                                .size(px(52.))
                                .rounded_full()
                                .bg(cx.theme().muted)
                                .text_color(cx.theme().muted_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(AppIcon::FolderOpen.element(24.)),
                        )
                        .child(div().font_semibold().child(if search_active {
                            "没有匹配的项目"
                        } else {
                            "此文件夹为空"
                        }))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("拖入文件即可开始压缩，或切换到其他位置。"),
                        ),
                )
            })
            .when(entry_count > 0, |this| {
                let scroll = self.file_list_scroll.clone();
                this.child(
                    v_flex()
                        .flex_1()
                        .w_full()
                        .min_h_0()
                        .relative()
                        .overflow_hidden()
                        .child(
                            gpui_kit::base::v_virtual_list(
                                cx.entity(),
                                "file-list",
                                item_sizes,
                                move |app, range, _window, cx| {
                                    range
                                        .map(|virtual_index| {
                                            if tile_view {
                                                render_tile_row(
                                                    app,
                                                    cx,
                                                    virtual_index,
                                                    tile_columns,
                                                    row_height,
                                                )
                                                .into_any_element()
                                            } else {
                                                render_file_row(
                                                    app,
                                                    cx,
                                                    virtual_index,
                                                    row_height,
                                                    size_width,
                                                    modified_width,
                                                    kind_width,
                                                    archive_columns,
                                                    inside_archive,
                                                )
                                                .into_any_element()
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                },
                            )
                            .track_scroll(&scroll),
                        )
                        .vertical_scrollbar(&scroll),
                )
            })
    }

    /// 渲染侧栏下半部分的信息检查器。
    ///
    /// 选中项目时显示大小、时间、路径和归档条目元数据；未选中时显示
    /// 当前文件夹摘要，对应参考图片中“详细信息”区域的两种状态。
    pub(crate) fn render_details(
        &self,
        max_height: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected_entry();
        let has_selection = selected.is_some();
        let display = self.browser.current_display();
        let folder_name = Path::new(&display)
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| display.clone());
        let entry_count = self.displayed_count();
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        v_flex()
            .id("details-panel")
            .w_full()
            .flex_shrink_0()
            .max_h(px(max_height))
            .overflow_hidden()
            .bg(cx.theme().popover)
            .border_t_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                h_flex()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_1()
                    .items_center()
                    .bg(cx.theme().sidebar_primary)
                    .text_color(cx.theme().sidebar_foreground)
                    .child(AppIcon::Doc.element(13.))
                    .child(div().text_xs().font_semibold().child("详细信息"))
                    .child(
                        icon_button("details-hide", AppIcon::Close, "隐藏详细信息")
                            .ml_auto()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_details();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("details-body")
                    .p_3()
                    .gap_2()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when_some(selected, |this, entry| {
                        let icon_color = entry_icon_color(cx, &entry);
                        let size_text = if entry.is_dir {
                            self.folder_sizes
                                .get(&entry.path)
                                .map(|size| format_size(*size))
                                .unwrap_or_else(|| "可计算".to_string())
                        } else {
                            format_size(entry.size)
                        };
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_start()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(icon_color)
                                        .child(entry_icon(&entry).element(17.)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .whitespace_normal()
                                        .child(entry.name.clone()),
                                ),
                        )
                        .child(detail_row(cx, "类型", &entry.kind))
                        .child(detail_row(cx, "大小", &size_text))
                        .when_some(entry.modified, |this, modified| {
                            this.child(detail_row(
                                cx,
                                "修改时间",
                                &modified.format("%Y-%m-%d %H:%M:%S").to_string(),
                            ))
                        })
                        .child(detail_row(cx, "位置", &entry.path.display().to_string()))
                        .when(entry.encrypted, |this| {
                            this.child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .rounded_sm()
                                    .bg(cx.theme().warning)
                                    .text_color(cx.theme().warning_foreground)
                                    .px_2()
                                    .py_1()
                                    .child(AppIcon::FileLock.element(12.))
                                    .child(div().text_xs().child("加密条目")),
                            )
                        })
                        .when_some(
                            entry.archive_entry.as_ref(),
                            |this, archive| {
                                this.child(div().h(px(1.)).bg(cx.theme().border))
                                    .child(detail_row(
                                        cx,
                                        "压缩后",
                                        &archive
                                            .compressed_size
                                            .map(format_size)
                                            .unwrap_or_else(|| "—".to_string()),
                                    ))
                                    .child(detail_row(cx, "压缩方法", &archive.method))
                                    .child(detail_row(
                                        cx,
                                        "CRC32",
                                        &archive
                                            .crc32
                                            .map(|value| format!("{value:08X}"))
                                            .unwrap_or_else(|| "—".to_string()),
                                    ))
                                    .child(detail_row(
                                        cx,
                                        "加密",
                                        if archive.encrypted { "是" } else { "否" },
                                    ))
                            },
                        )
                    })
                    .when(!has_selection, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_start()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(cx.theme().warning_foreground)
                                        .child(AppIcon::Folder.element(17.)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .whitespace_normal()
                                        .child(folder_name),
                                ),
                        )
                        .child(detail_row(cx, "类型", "文件夹"))
                        .child(detail_row(cx, "项目", &format!("{entry_count} 项")))
                        .child(detail_row(cx, "总计", &format_size(total_size)))
                        .child(detail_row(cx, "位置", &display))
                        .child(hint_text(cx, "选择一个项目可查看 CRC 和压缩方法。"))
                    }),
            )
    }
}

/// 渲染详情列表中的一个紧凑虚拟行。
#[allow(clippy::too_many_arguments)]
fn render_file_row(
    app: &mut LumarkApp,
    cx: &mut Context<LumarkApp>,
    virtual_index: usize,
    row_height: gpui_kit::Pixels,
    size_width: f32,
    modified_width: f32,
    kind_width: f32,
    columns: ArchiveColumns,
    inside_archive: bool,
) -> impl IntoElement {
    let Some(entry) = app.displayed_entry_at(virtual_index) else {
        return h_flex()
            .id(("empty-file-row", virtual_index))
            .into_any_element();
    };
    let id = entry.id.clone();
    let mouse_id = id.clone();
    let double_id = id.clone();
    let context_id = id.clone();
    let selected = app.browser.selection.is_selected(&id);
    let icon_color = entry_icon_color(cx, &entry);
    let menu_kind = file_menu_kind(&app.browser.location, &entry);
    let drag_item = inside_archive
        .then(|| archive_drag_item(app, &entry))
        .flatten();
    let modified = entry
        .modified
        .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".to_string());
    let compressed = entry
        .archive_entry
        .as_ref()
        .and_then(|archive| archive.compressed_size)
        .map(format_size)
        .unwrap_or_else(|| "—".to_string());
    let algorithm = entry
        .archive_entry
        .as_ref()
        .map(|archive| archive.method.clone())
        .unwrap_or_else(|| "—".to_string());
    let crc32 = entry
        .archive_entry
        .as_ref()
        .and_then(|archive| archive.crc32)
        .map(|value| format!("{value:08X}"))
        .unwrap_or_else(|| "—".to_string());
    let inner_path = entry.path.display().to_string();
    let folder_size = app.folder_sizes.get(&entry.path).copied();
    let folder_pending = app.folder_size_pending.contains(&entry.path);
    let is_filesystem_dir =
        entry.is_dir && matches!(&app.browser.location, BrowserLocation::FileSystem(_));
    let folder_size_path = entry.path.clone();
    h_flex()
        .id(format!("file-row-{id}"))
        .test_support()
        .w_full()
        .h(row_height)
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .cursor_pointer()
        .when(selected, |this| this.bg(cx.theme().list_active))
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().list_hover))
        })
        .child(
            div()
                .w(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(icon_color)
                .child(entry_icon(&entry).element(15.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_sm()
                .child(entry.name.clone()),
        )
        .child(
            div()
                .w(px(size_width))
                .flex_shrink_0()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .when_some(folder_size, |this, size| this.child(format_size(size)))
                .when(folder_size.is_none() && folder_pending, |this| {
                    this.child("计算中…")
                })
                .when(
                    folder_size.is_none() && !folder_pending && is_filesystem_dir,
                    |this| {
                        this.child(
                            button(format!("folder-size-{id}"))
                                .link()
                                .xsmall()
                                .cursor_pointer()
                                .label("可计算")
                                .tooltip("计算文件夹内文件的总大小")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_folder_size(folder_size_path.clone(), cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                    },
                )
                .when(!is_filesystem_dir && folder_size.is_none(), |this| {
                    this.child(format_size(entry.size))
                }),
        )
        .when(columns.compressed, |this| {
            this.child(archive_cell(cx, COMPRESSED_WIDTH, compressed))
        })
        .child(
            div()
                .w(px(modified_width))
                .flex_shrink_0()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child(modified),
        )
        .when(columns.algorithm, |this| {
            this.child(archive_cell(cx, ALGORITHM_WIDTH, algorithm))
        })
        .when(columns.crc32, |this| {
            this.child(archive_cell(cx, CRC_WIDTH, crc32))
        })
        .when(columns.path, |this| {
            this.child(archive_cell(cx, PATH_WIDTH, inner_path.clone()))
        })
        .when(!inside_archive, |this| {
            this.child(
                div()
                    .w(px(kind_width))
                    .flex_shrink_0()
                    .text_xs()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_color(cx.theme().muted_foreground)
                    .child(entry.kind.clone()),
            )
        })
        .on_click(
            cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    this.browser.selection.select_only(&double_id);
                    this.open_selected(window, cx);
                    cx.notify();
                }
            }),
        )
        .on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                app_select_row(this, event, &mouse_id);
                this.browser_focus.focus(window, cx);
                cx.notify();
            }),
        )
        .on_mouse_down(
            gpui_kit::MouseButton::Right,
            cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                if !this.browser.selection.is_selected(&context_id) {
                    this.browser.selection.select_only(&context_id);
                }
                cx.notify();
            }),
        )
        .when_some(drag_item, |this, item| {
            this.on_drag(item, |item: &ArchiveDragItem, _, window, cx| {
                start_archive_native_drag(item, window);
                let preview = cx.new(|_| ArchiveDragPreview {
                    label: item.label.clone(),
                });
                window.defer(cx, |window, cx| {
                    cx.stop_active_drag(window);
                });
                preview
            })
        })
        .context_menu(file_context_menu(cx.entity(), menu_kind))
        .into_any_element()
}

/// 渲染一行平铺项目，每项对应文件或文件夹。
fn render_tile_row(
    app: &mut LumarkApp,
    cx: &mut Context<LumarkApp>,
    virtual_index: usize,
    tile_columns: usize,
    row_height: gpui_kit::Pixels,
) -> impl IntoElement {
    let start = virtual_index * tile_columns;
    let end = (start + tile_columns).min(app.displayed_count());
    h_flex()
        .id(("tile-row", virtual_index))
        .w_full()
        .h(row_height)
        .gap_2()
        .px_3()
        .children((start..end).filter_map(|index| {
            let entry = app.displayed_entry_at(index)?;
            let id = entry.id.clone();
            let mouse_id = id.clone();
            let double_id = id.clone();
            let context_id = id.clone();
            let selected = app.browser.selection.is_selected(&id);
            let icon_color = entry_icon_color(cx, &entry);
            let tile_icon = if entry.is_dir {
                AppIcon::Folder
            } else {
                entry_icon(&entry)
            };
            let menu_kind = file_menu_kind(&app.browser.location, &entry);
            let drag_item = matches!(&app.browser.location, BrowserLocation::Archive { .. })
                .then(|| archive_drag_item(app, &entry))
                .flatten();
            let tile_icon = if let Some(icon) = system_icon_for(app, &entry) {
                gpui_kit::img(icon).size(px(48.)).into_any_element()
            } else {
                tile_icon.element(48.).into_any_element()
            };
            Some(
                v_flex()
                    .id(format!("file-tile-{id}"))
                    .test_support()
                    .w(px(120.))
                    .h(px(120.))
                    .p_2()
                    .gap_2()
                    .items_center()
                    .rounded_lg()
                    .when(selected, |this| this.bg(cx.theme().list_active))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(cx.theme().list_hover))
                    })
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(icon_color)
                            .child(tile_icon),
                    )
                    .child(
                        div()
                            .id(format!("file-tile-name-{id}"))
                            .test_support()
                            .w_full()
                            .text_sm()
                            .whitespace_normal()
                            .line_clamp(2)
                            .child(entry.name.clone()),
                    )
                    .on_click(
                        cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                            if event.click_count() >= 2 {
                                this.browser.selection.select_only(&double_id);
                                this.open_selected(window, cx);
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                            app_select_row(this, event, &mouse_id);
                            this.browser_focus.focus(window, cx);
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        gpui_kit::MouseButton::Right,
                        cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                            if !this.browser.selection.is_selected(&context_id) {
                                this.browser.selection.select_only(&context_id);
                            }
                            cx.notify();
                        }),
                    )
                    .when_some(drag_item, |this, item| {
                        this.on_drag(item, |item: &ArchiveDragItem, _, window, cx| {
                            start_archive_native_drag(item, window);
                            let preview = cx.new(|_| ArchiveDragPreview {
                                label: item.label.clone(),
                            });
                            window.defer(cx, |window, cx| {
                                cx.stop_active_drag(window);
                            });
                            preview
                        })
                    })
                    .context_menu(file_context_menu(cx.entity(), menu_kind)),
            )
        }))
}

/// 构建文件行使用的官方右键菜单。
///
/// 菜单项通过 `PopupMenuItem::on_click` 直接调用应用动作，避免再维护一套
/// 自绘菜单和透明关闭层；菜单的定位、键盘导航、分隔线和阴影由组件库负责。
/// 归档内和文件系统里的条目拥有不同的动作集合：归档内不能新建文件夹、
/// 也不提供新窗口入口；文件系统内的压缩包才提供解压动作。
fn file_context_menu(
    app: gpui_kit::Entity<LumarkApp>,
    kind: FileMenuKind,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, _| match kind {
        FileMenuKind::ArchiveFolder | FileMenuKind::ArchiveFile => {
            let icon = if kind == FileMenuKind::ArchiveFolder {
                AppIcon::FolderOpen
            } else {
                AppIcon::Doc
            };
            menu.item(popup_action(
                "打开",
                icon,
                &app,
                window,
                |app, window, cx| app.open_selected(window, cx),
            ))
            .item(popup_action(
                "使用内置查看器打开",
                AppIcon::Doc,
                &app,
                window,
                |app, window, cx| app.open_selected_in_viewer(window, cx),
            ))
            .item(popup_action(
                "解压到…",
                AppIcon::Zip,
                &app,
                window,
                |app, window, cx| app.open_extract_dialog(window, cx),
            ))
            .separator()
            .item(popup_action(
                "复制文件",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_items(cx),
            ))
            .item(popup_action(
                "复制路径",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_path(cx),
            ))
            .item(popup_action(
                "打开压缩包所在目录",
                AppIcon::Search,
                &app,
                window,
                |app, _, _| app.reveal_selected(),
            ))
            .separator()
            .item(popup_action(
                "重命名",
                AppIcon::Edit,
                &app,
                window,
                |app, window, cx| app.start_rename(window, cx),
            ))
            .item(popup_action(
                "删除",
                AppIcon::Delete,
                &app,
                window,
                |app, _, _| app.request_delete(),
            ))
        }
        FileMenuKind::Folder => menu
            .item(popup_action(
                "打开",
                AppIcon::FolderOpen,
                &app,
                window,
                |app, window, cx| app.open_selected(window, cx),
            ))
            .item(popup_action(
                "在新窗口打开",
                AppIcon::Grid,
                &app,
                window,
                |app, _, cx| app.open_directory_window(cx),
            ))
            .separator()
            .item(popup_action(
                "压缩为 Lumark 归档",
                AppIcon::Compression,
                &app,
                window,
                |app, window, cx| app.open_compress_dialog(window, cx),
            ))
            .separator()
            .item(popup_action(
                "新建文件夹",
                AppIcon::FolderPlus,
                &app,
                window,
                |app, window, cx| app.start_new_folder(window, cx),
            ))
            .item(popup_action(
                "复制文件",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_items(cx),
            ))
            .item(popup_action(
                "复制路径",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_path(cx),
            ))
            .item(popup_action(
                "在资源管理器中显示",
                AppIcon::Search,
                &app,
                window,
                |app, _, _| app.reveal_selected(),
            ))
            .separator()
            .item(popup_action(
                "重命名",
                AppIcon::Edit,
                &app,
                window,
                |app, window, cx| app.start_rename(window, cx),
            ))
            .item(popup_action(
                "删除",
                AppIcon::Delete,
                &app,
                window,
                |app, _, _| app.request_delete(),
            )),
        FileMenuKind::ArchiveFileSystem => menu
            .item(popup_action(
                "用 Lumark 打开",
                AppIcon::FolderOpen,
                &app,
                window,
                |app, window, cx| app.open_selected(window, cx),
            ))
            .item(popup_action(
                "解压到…",
                AppIcon::Zip,
                &app,
                window,
                |app, window, cx| app.open_extract_dialog(window, cx),
            ))
            .separator()
            .item(popup_action(
                "复制文件",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_items(cx),
            ))
            .item(popup_action(
                "复制路径",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_path(cx),
            ))
            .item(popup_action(
                "在资源管理器中显示",
                AppIcon::Search,
                &app,
                window,
                |app, _, _| app.reveal_selected(),
            ))
            .separator()
            .item(popup_action(
                "重命名",
                AppIcon::Edit,
                &app,
                window,
                |app, window, cx| app.start_rename(window, cx),
            ))
            .item(popup_action(
                "删除",
                AppIcon::Delete,
                &app,
                window,
                |app, _, _| app.request_delete(),
            )),
        FileMenuKind::ExecutableFile | FileMenuKind::File => menu
            .item(popup_action(
                if kind == FileMenuKind::ExecutableFile {
                    "运行"
                } else {
                    "打开"
                },
                AppIcon::Doc,
                &app,
                window,
                |app, window, cx| app.open_selected(window, cx),
            ))
            .item(popup_action(
                "使用内置查看器打开",
                AppIcon::Doc,
                &app,
                window,
                |app, window, cx| app.open_selected_in_viewer(window, cx),
            ))
            .item(popup_action(
                "压缩为 Lumark 归档",
                AppIcon::Compression,
                &app,
                window,
                |app, window, cx| app.open_compress_dialog(window, cx),
            ))
            .separator()
            .item(popup_action(
                "复制文件",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_items(cx),
            ))
            .item(popup_action(
                "复制路径",
                AppIcon::Copy,
                &app,
                window,
                |app, _, cx| app.copy_selected_path(cx),
            ))
            .item(popup_action(
                "在资源管理器中显示",
                AppIcon::Search,
                &app,
                window,
                |app, _, _| app.reveal_selected(),
            ))
            .separator()
            .item(popup_action(
                "重命名",
                AppIcon::Edit,
                &app,
                window,
                |app, window, cx| app.start_rename(window, cx),
            ))
            .item(popup_action(
                "删除",
                AppIcon::Delete,
                &app,
                window,
                |app, _, _| app.request_delete(),
            )),
    }
}

/// 渲染归档附加列中的一个只读文本单元。
fn archive_cell(cx: &Context<LumarkApp>, width: f32, text: String) -> gpui_kit::Div {
    div()
        .w(px(width))
        .flex_shrink_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// 创建一个带图标、点击后调用应用动作的官方菜单项。
fn popup_action(
    label: &'static str,
    icon: AppIcon,
    app: &gpui_kit::Entity<LumarkApp>,
    window: &mut Window,
    action: impl Fn(&mut LumarkApp, &mut Window, &mut Context<LumarkApp>) + 'static,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(icon.element(14.))
        .on_click(
            window.listener_for(app, move |app, _: &gpui_kit::ClickEvent, window, cx| {
                action(app, window, cx);
                cx.notify();
            }),
        )
}

/// 渲染详情检查器中的带标签值，标签固定宽度保证多行值对齐。
fn detail_row(cx: &Context<LumarkApp>, label: &str, value: &str) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .child(
            div()
                .w(px(44.))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .whitespace_normal()
                .child(value.to_string()),
        )
}

#[cfg(test)]
mod tests {
    use super::{ArchiveDragItem, FileMenuKind, archive_columns, file_menu_kind};
    use lumark_archive::ArchiveEntry;
    use lumark_browser::{BrowserEntry, BrowserLocation};
    use std::path::PathBuf;

    /// 验证归档列随宽度收窄，窄窗口先隐藏路径和 CRC32。
    #[test]
    fn archive_columns_shrink_with_width() {
        let wide = archive_columns(1200.);
        assert!(wide.compressed && wide.algorithm && wide.crc32 && wide.path);
        let medium = archive_columns(700.);
        assert!(medium.compressed && medium.algorithm && medium.crc32);
        assert!(!medium.path);
        let narrow = archive_columns(620.);
        assert!(narrow.compressed && narrow.crc32);
        assert!(!narrow.algorithm && !narrow.path);
        let tiny = archive_columns(500.);
        assert!(!tiny.compressed && !tiny.algorithm && !tiny.crc32 && !tiny.path);
    }

    /// 验证归档内、文件系统和压缩包文件使用不同菜单。
    #[test]
    fn menu_kind_depends_on_location_and_entry_type() {
        let inside = BrowserLocation::Archive {
            archive: PathBuf::from("demo.zip"),
            inner: String::new(),
        };
        let outside = BrowserLocation::FileSystem(PathBuf::from("C:/downloads"));
        let folder = entry("folder", true, None);
        let archive_file = entry("demo.zip", false, None);
        let plain = entry("notes.txt", false, None);
        let inside_file = entry("docs/readme.txt", false, Some("deflate"));
        let executable = entry("setup.exe", false, None);

        assert_eq!(
            file_menu_kind(&inside, &folder),
            FileMenuKind::ArchiveFolder
        );
        assert_eq!(
            file_menu_kind(&inside, &inside_file),
            FileMenuKind::ArchiveFile
        );
        assert_eq!(file_menu_kind(&outside, &folder), FileMenuKind::Folder);
        assert_eq!(
            file_menu_kind(&outside, &archive_file),
            FileMenuKind::ArchiveFileSystem
        );
        assert_eq!(file_menu_kind(&outside, &plain), FileMenuKind::File);
        assert_eq!(
            file_menu_kind(&outside, &executable),
            FileMenuKind::ExecutableFile
        );
    }

    /// 构造一个只填充菜单判定所需字段的条目。
    fn entry(name: &str, is_dir: bool, method: Option<&str>) -> BrowserEntry {
        BrowserEntry {
            id: format!("archive:{name}"),
            name: name.rsplit('/').next().unwrap_or(name).to_string(),
            path: PathBuf::from(name),
            is_dir,
            size: 0,
            modified: None,
            kind: method.unwrap_or("").to_string(),
            archive_entry: method.map(|method| ArchiveEntry {
                id: 0,
                path: name.to_string(),
                is_dir,
                size: 0,
                compressed_size: Some(0),
                modified: None,
                crc32: Some(0),
                method: method.to_string(),
                encrypted: false,
            }),
            encrypted: false,
        }
    }

    /// 验证拖出归档目录时会先落盘临时文件，并把真实路径交给原生拖放。
    #[test]
    fn archive_drag_payload_extracts_selected_files() {
        use lumark_archive::{ArchiveService, CompressionOptions};
        use tempfile::tempdir;

        let directory = tempdir().unwrap();
        let source = directory.path().join("docs");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("readme.txt"), b"hello").unwrap();
        let archive = directory.path().join("docs.zip");
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format: lumark_archive::ArchiveFormat::Zip,
                path_mode: lumark_archive::PathMode::Relative,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let item = ArchiveDragItem {
            archive,
            password: None,
            selected: vec![("docs".to_string(), true)],
            output: directory.path().join("drag"),
            label: "docs".to_string(),
        };
        let payload = item.materialize_paths().expect("拖出数据应可生成");
        assert_eq!(payload.len(), 1);
        assert!(payload[0].exists());
    }
}
