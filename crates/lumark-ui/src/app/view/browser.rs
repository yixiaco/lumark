//! 主文件表、搜索结果条和侧栏详情检查器入口。

use super::super::actions::TempExtractMode;
use super::super::*;
use super::components::{AppIcon, entry_icon, entry_icon_color, hint_text, icon_button};
use crate::i18n::tr;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;

use columns::*;
use drag::*;
use menu::*;
use rows::*;
use tiles::*;

mod columns;
mod details;
mod drag;
mod marquee;
mod menu;
mod preview;
mod preview_ue;
mod rows;
mod thumbnails;
mod tiles;

pub(crate) use drag::{ArchiveDragItem, path_bar_drop_handler};
use marquee::*;
pub(crate) use preview::DetailsPreview;

/// 文件表键盘移动方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionMove {
    /// 上一行（平铺模式跨一行）。
    Up,
    /// 下一行（平铺模式跨一行）。
    Down,
    /// 上一个条目/左一列。
    Left,
    /// 下一个条目/右一列。
    Right,
    /// 首个条目。
    Home,
    /// 末个条目。
    End,
    /// 向上一页。
    PageUp,
    /// 向下一页。
    PageDown,
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
        let show_parent = shows_parent_row(self);
        let row_count = entry_count + usize::from(show_parent);
        let main_width = self.browser_list_width(window);
        let table_width = self.browser_table_width(window);
        let widths = self.column_widths;
        let inside_archive = matches!(&self.browser.location, BrowserLocation::Archive { .. });
        let archive_columns = if inside_archive {
            archive_columns(table_width)
        } else {
            ArchiveColumns::default()
        };
        let tile_view = self.tile_view;
        let tile = TileMetrics::for_width(main_width, tile_view);
        let tile_columns = tile.columns;
        let virtual_rows = if tile_view {
            row_count.div_ceil(tile_columns)
        } else {
            row_count
        };
        let row_height = px(tile.row_height);
        let item_sizes = Rc::new(vec![
            gpui_kit::size(px(main_width), row_height);
            virtual_rows
        ]);
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        let marquee_rect = self.marquee.as_ref().and_then(|state| {
            self.current_marquee_layout(window).and_then(|layout| {
                layout.overlay_rect(
                    (state.start.x.as_f32(), state.start.y.as_f32()),
                    (state.current.x.as_f32(), state.current.y.as_f32()),
                )
            })
        });

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
                        this.request_delete(window, cx);
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("f2") {
                        this.start_rename(window, cx);
                        cx.notify();
                    } else if let Some(movement) = selection_move_for_key(&keystroke.key) {
                        if this.browser_focus.is_focused(window) {
                            this.move_selection(movement, keystroke.modifiers.shift, window, cx);
                        }
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
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, window, cx| {
                    this.browser_focus.focus(window, cx);
                    if this.start_marquee(event.position, event.modifiers, window) {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, window, cx| {
                    this.update_column_resize(event);
                    if this.marquee.is_some() {
                        if event.pressed_button == Some(gpui_kit::MouseButton::Left) {
                            this.update_marquee(event.position, window);
                        } else {
                            this.finish_marquee();
                        }
                    }
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.finish_column_resize();
                    this.finish_marquee();
                    cx.notify();
                }),
            )
            .flex_1()
            .when(self.game_session.is_some(), |this| {
                this.child(self.render_game_context_bar(cx))
            })
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
                        div()
                            .flex_1()
                            .child(tr("名称与类型", "Name and type"))
                            .into_any_element()
                    } else {
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(header_cell(tr("名称", "Name"), None, None, cx))
                            .child(header_cell(
                                tr("大小", "Size"),
                                Some(widths.size),
                                Some(ColumnKind::Size),
                                cx,
                            ))
                            .when(archive_columns.compressed, |this| {
                                this.child(header_cell(
                                    tr("压缩后大小", "Compressed size"),
                                    Some(widths.compressed),
                                    Some(ColumnKind::Compressed),
                                    cx,
                                ))
                            })
                            .child(header_cell(
                                tr("修改时间", "Modified"),
                                Some(widths.modified),
                                Some(ColumnKind::Modified),
                                cx,
                            ))
                            .when(archive_columns.algorithm, |this| {
                                this.child(header_cell(
                                    tr("压缩算法", "Method"),
                                    Some(widths.algorithm),
                                    Some(ColumnKind::Algorithm),
                                    cx,
                                ))
                            })
                            .when(archive_columns.crc32, |this| {
                                this.child(header_cell(
                                    "CRC32",
                                    Some(widths.crc32),
                                    Some(ColumnKind::Crc32),
                                    cx,
                                ))
                            })
                            .when(archive_columns.path, |this| {
                                this.child(header_cell(
                                    tr("路径", "Path"),
                                    Some(widths.path),
                                    Some(ColumnKind::Path),
                                    cx,
                                ))
                            })
                            .when(!inside_archive, |this| {
                                this.child(header_cell(
                                    tr("类型", "Type"),
                                    Some(widths.kind),
                                    Some(ColumnKind::Kind),
                                    cx,
                                ))
                            })
                            .into_any_element()
                    }),
            )
            .when(entry_count == 0, |this| {
                this.when(show_parent, |this| {
                    let parent = if tile_view {
                        h_flex()
                            .w_full()
                            .px_3()
                            .child(render_parent_tile(cx))
                            .into_any_element()
                    } else {
                        render_parent_row(cx, row_height, widths, archive_columns, inside_archive)
                            .into_any_element()
                    };
                    this.child(parent)
                })
                .child(empty_state(cx, search_active))
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
                                                    tile.tile_width,
                                                    row_height,
                                                )
                                                .into_any_element()
                                            } else {
                                                render_file_row(
                                                    app,
                                                    cx,
                                                    virtual_index,
                                                    row_height,
                                                    widths,
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
                        .when_some(marquee_rect, |this, (left, top, width, height)| {
                            this.child(
                                div()
                                    .absolute()
                                    .left(px(left))
                                    .top(px(top))
                                    .w(px(width))
                                    .h(px(height))
                                    .rounded(px(2.))
                                    .border_1()
                                    .border_color(cx.theme().primary)
                                    .bg(cx.theme().primary.opacity(0.08)),
                            )
                        })
                        .vertical_scrollbar(&scroll),
                )
            })
    }
}

impl LumarkApp {
    /// 按方向键移动文件表选择；`extend` 为 Shift 范围扩展。
    ///
    /// 平铺视图的上下移动跨一整行（列数），左右移动一个卡片；详情列表
    /// 四个方向都按单项移动，保证用户不必切换视图也能用方向键遍历。
    pub(crate) fn move_selection(
        &mut self,
        movement: SelectionMove,
        extend: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let ids = self.displayed_ids();
        if ids.is_empty() {
            return false;
        }
        let count = ids.len();
        let main_width = self.browser_list_width(window);
        let metrics = TileMetrics::for_width(main_width, self.tile_view);
        let columns = self.tile_columns_at(main_width).max(1);
        let rows_per_page = {
            let height = self.file_list_scroll.bounds().size.height.as_f32();
            ((height / metrics.row_height.max(1.0)).floor() as usize).max(1)
        };
        let page = rows_per_page * if self.tile_view { columns } else { 1 };
        let current = self
            .browser
            .selection
            .focus()
            .and_then(|focus| ids.iter().position(|id| id == focus));
        let target = match movement {
            SelectionMove::Up => current.map_or(count - 1, |index| index.saturating_sub(columns)),
            SelectionMove::Down => current.map_or(0, |index| (index + columns).min(count - 1)),
            SelectionMove::Left => current.map_or(0, |index| index.saturating_sub(1)),
            SelectionMove::Right => current.map_or(0, |index| (index + 1).min(count - 1)),
            SelectionMove::Home => 0,
            SelectionMove::End => count - 1,
            SelectionMove::PageUp => current.map_or(0, |index| index.saturating_sub(page)),
            SelectionMove::PageDown => {
                current.map_or(count - 1, |index| (index + page).min(count - 1))
            }
        };
        if extend {
            self.browser
                .selection
                .select_range(&ids, &ids[target], true);
        } else {
            self.browser.selection.select_only(ids[target].clone());
        }
        // 让目标行滚入视口；平铺视图把条目索引换算为行号。
        let parent_offset = usize::from(shows_parent_row(self));
        let virtual_row = target + parent_offset;
        let scroll_row = if self.tile_view {
            virtual_row / columns
        } else {
            virtual_row
        };
        self.file_list_scroll
            .scroll_to_item(scroll_row, gpui_kit::ScrollStrategy::Nearest);
        cx.notify();
        true
    }

    /// 给定文件表宽度时的平铺列数；详情列表固定为 1。
    pub(crate) fn tile_columns_at(&self, width: f32) -> usize {
        TileMetrics::for_width(width, self.tile_view).columns
    }

    /// 文件表视口宽度：由当前窗口宽度与侧栏宽度计算。
    ///
    /// 使用窗口尺寸而不是上一帧的列表 bounds，避免窗口缩放后的第一帧
    /// 仍按旧宽度计算平铺列数（列表和框选都会短暂错位）。
    pub(crate) fn browser_list_width(&self, window: &Window) -> f32 {
        let window_width = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .width
            .as_f32();
        let sidebar_width = if self.sidebar_visible {
            self.sidebar_width
        } else {
            0.0
        };
        (window_width - sidebar_width).max(240.0)
    }

    /// 文件表表格可用宽度（视口减去左右内边距）。
    fn browser_table_width(&self, window: &Window) -> f32 {
        (self.browser_list_width(window) - TILE_PADDING_X * 2.0).max(240.0)
    }

    /// 按当前窗口、滚动位置和视图模式生成框选布局。
    fn current_marquee_layout(&self, window: &Window) -> Option<MarqueeLayout> {
        let bounds = self.file_list_scroll.bounds();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return None;
        }
        let parent_row = shows_parent_row(self);
        let tile = self.tile_view;
        let row_count = self.displayed_count() + usize::from(parent_row);
        let metrics = TileMetrics::for_width(self.browser_list_width(window), tile);
        Some(MarqueeLayout {
            left: bounds.origin.x.as_f32(),
            top: bounds.origin.y.as_f32(),
            width: bounds.size.width.as_f32(),
            height: bounds.size.height.as_f32(),
            scroll_y: self.file_list_scroll.offset().y.as_f32(),
            row_height: metrics.row_height,
            parent_row,
            row_count,
            tile_columns: metrics.columns,
            tile_width: metrics.tile_width,
            tile_pitch: metrics.pitch,
            tile_padding: metrics.padding,
            tile,
        })
    }

    /// 在文件表空白区域开始框选；命中行/卡片时返回 `false`。
    pub(crate) fn start_marquee(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        modifiers: gpui_kit::Modifiers,
        window: &Window,
    ) -> bool {
        let Some(layout) = self.current_marquee_layout(window) else {
            return false;
        };
        if !layout.starts_blank(position.x.as_f32(), position.y.as_f32()) {
            return false;
        }
        let additive = modifiers.control || modifiers.shift;
        let base_ids = if additive {
            self.browser.selection.selected_ids()
        } else {
            self.browser.selection.clear();
            Vec::new()
        };
        self.marquee = Some(MarqueeState {
            start: position,
            current: position,
            base_ids,
        });
        true
    }

    /// 更新框选矩形并同步选择；没有进行中的框选时返回 `false`。
    pub(crate) fn update_marquee(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &Window,
    ) -> bool {
        let Some(state) = self.marquee.as_ref() else {
            return false;
        };
        let start = (state.start.x.as_f32(), state.start.y.as_f32());
        let base_ids = state.base_ids.clone();
        let Some(layout) = self.current_marquee_layout(window) else {
            return false;
        };
        let direction = layout.autoscroll_direction(position.y.as_f32());
        if direction != 0 {
            self.scroll_marquee(direction, layout.row_height);
        }
        let layout = self.current_marquee_layout(window).unwrap_or(layout);
        let entries = self.displayed_entries();
        let mut ids = base_ids;
        ids.extend(
            layout
                .selected_entries(start, (position.x.as_f32(), position.y.as_f32()))
                .into_iter()
                .filter_map(|index| entries.get(index).map(|entry| entry.id.clone())),
        );
        ids.sort();
        ids.dedup();
        self.browser.selection.select_ids(ids, false);
        if let Some(state) = self.marquee.as_mut() {
            state.current = position;
        }
        true
    }

    /// 结束框选；返回是否确实结束了进行中的框选。
    pub(crate) fn finish_marquee(&mut self) -> bool {
        self.marquee.take().is_some()
    }

    /// 框选期间按行调整滚动位置，并限制在可滚动范围内。
    fn scroll_marquee(&mut self, direction: i32, row_height: f32) {
        let max = self.file_list_scroll.max_offset().y;
        let mut offset = self.file_list_scroll.offset();
        let next = (offset.y.as_f32() + direction as f32 * row_height).clamp(-max.as_f32(), 0.0);
        offset.y = px(next);
        self.file_list_scroll.set_offset(offset);
    }

    /// 根据鼠标移动更新正在拖动的列宽。
    pub(super) fn update_column_resize(&mut self, event: &gpui_kit::MouseMoveEvent) {
        let Some(resize) = self.column_resize else {
            return;
        };
        let delta = event.position.x.as_f32() - resize.start_x;
        self.column_widths
            .clamp_column(resize.column, resize.start_width + delta);
    }

    /// 结束列宽拖动并把结果写入设置。
    pub(super) fn finish_column_resize(&mut self) {
        if self.column_resize.take().is_none() {
            return;
        }
        let widths = self.column_widths;
        if let Err(error) = self
            .settings
            .update(|settings| settings.column_widths = Some(widths))
        {
            self.status = error.to_string();
        }
    }
}

/// 把键盘按键映射为文件表移动方向；非方向键返回 `None`。
pub(crate) fn selection_move_for_key(key: &str) -> Option<SelectionMove> {
    Some(match key.to_ascii_lowercase().as_str() {
        "up" => SelectionMove::Up,
        "down" => SelectionMove::Down,
        "left" => SelectionMove::Left,
        "right" => SelectionMove::Right,
        "home" => SelectionMove::Home,
        "end" => SelectionMove::End,
        "pageup" => SelectionMove::PageUp,
        "pagedown" => SelectionMove::PageDown,
        _ => return None,
    })
}

/// 渲染一个可拖动列宽的表头单元格。
fn header_cell(
    label: &'static str,
    width: Option<f32>,
    column: Option<ColumnKind>,
    cx: &mut Context<LumarkApp>,
) -> gpui_kit::AnyElement {
    let mut cell = h_flex().relative().h_full().items_center().min_w_0();
    if let Some(width) = width {
        cell = cell.w(px(width));
    } else {
        cell = cell.flex_1();
    }
    cell = cell.child(div().truncate().child(label));
    if let (Some(width), Some(column)) = (width, column) {
        cell = cell.child(
            div()
                .id(("column-resize", column as usize))
                .absolute()
                .right(px(-3.))
                .top_0()
                .bottom_0()
                .w(px(6.))
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(move |this, event: &gpui_kit::MouseDownEvent, _, cx| {
                        this.column_resize = Some(ColumnResize {
                            column,
                            start_x: event.position.x.as_f32(),
                            start_width: width,
                        });
                        cx.notify();
                    }),
                ),
        );
    }
    cell.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::columns::archive_columns;
    use super::drag::ArchiveDragItem;
    use super::menu::{FileMenuKind, file_menu_kind};
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
        let iso = entry("image.iso", false, None);

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
        assert_eq!(file_menu_kind(&outside, &iso), FileMenuKind::IsoFile);
        assert_eq!(file_menu_kind(&outside, &plain), FileMenuKind::File);
        assert_eq!(
            file_menu_kind(&outside, &executable),
            FileMenuKind::ExecutableFile
        );
    }

    /// 真实 PE 文件按只读归档处理，菜单提供浏览/解压/运行入口。
    #[test]
    fn pe_file_is_classified_as_pe_archive() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("setup.exe");
        let mut bytes = vec![0u8; 0x100];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        std::fs::write(&file, bytes).unwrap();
        let executable = BrowserEntry {
            id: "fs:setup.exe".to_string(),
            name: "setup.exe".to_string(),
            path: file,
            is_dir: false,
            size: 0,
            modified: None,
            kind: String::new(),
            archive_entry: None,
            encrypted: false,
        };
        assert_eq!(
            file_menu_kind(
                &BrowserLocation::FileSystem(directory.path().to_path_buf()),
                &executable
            ),
            FileMenuKind::PeFile
        );
    }

    /// 伪装归档在文件系统里必须归类为可打开归档，菜单才提供打开/解压/还原入口。
    #[test]
    fn disguised_archive_is_classified_as_archive_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("hello.txt");
        std::fs::write(&source, b"hello").unwrap();
        let output = directory.path().join("cover.jpg");
        let options = lumark_archive::CompressionOptions {
            disguise: Some(lumark_archive::DisguiseOptions {
                mask: lumark_archive::DisguiseMask::PresetImage,
                original_name: None,
            }),
            ..Default::default()
        };
        lumark_archive::ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();
        let disguised = BrowserEntry {
            id: "fs:cover.jpg".to_string(),
            name: "cover.jpg".to_string(),
            path: output,
            is_dir: false,
            size: 0,
            modified: None,
            kind: String::new(),
            archive_entry: None,
            encrypted: false,
        };
        assert_eq!(
            file_menu_kind(
                &BrowserLocation::FileSystem(directory.path().to_path_buf()),
                &disguised
            ),
            FileMenuKind::ArchiveFileSystem
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
