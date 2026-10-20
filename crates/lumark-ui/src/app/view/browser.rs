use super::super::*;
use super::components::{AppIcon, entry_icon, entry_icon_color, hint_text, icon_button};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;

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
        let tile_view = self.tile_view;
        let tile_columns = if tile_view {
            ((main_width - 16.0) / 172.0).floor().max(1.0) as usize
        } else {
            1
        };
        let virtual_rows = if tile_view {
            entry_count.div_ceil(tile_columns)
        } else {
            entry_count
        };
        let row_height = if tile_view { px(92.) } else { px(31.) };
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
                            Button::new("search-result-clear")
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
                            .child(div().w(px(modified_width)).child("修改时间"))
                            .child(div().w(px(kind_width)).child("类型"))
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
                        .child(detail_row(cx, "大小", &format_size(entry.size)))
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
fn render_file_row(
    app: &mut LumarkApp,
    cx: &mut Context<LumarkApp>,
    virtual_index: usize,
    row_height: gpui_kit::Pixels,
    size_width: f32,
    modified_width: f32,
    kind_width: f32,
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
    let modified = entry
        .modified
        .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".to_string());
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
                .child(format_size(entry.size)),
        )
        .child(
            div()
                .w(px(modified_width))
                .flex_shrink_0()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child(modified),
        )
        .child(
            div()
                .w(px(kind_width))
                .flex_shrink_0()
                .text_xs()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_color(cx.theme().muted_foreground)
                .child(entry.kind),
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
        .context_menu(file_context_menu(cx.entity()))
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
            Some(
                v_flex()
                    .id(format!("file-tile-{id}"))
                    .test_support()
                    .w(px(164.))
                    .h(px(82.))
                    .p_3()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(if selected {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .bg(if selected {
                        cx.theme().list_active
                    } else {
                        cx.theme().background
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().list_hover))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(icon_color)
                                    .child(entry_icon(&entry).element(17.)),
                            )
                            .child(
                                div()
                                    .id(format!("file-tile-name-{id}"))
                                    .test_support()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .font_semibold()
                                    .child(entry.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} · {}", entry.kind, format_size(entry.size))),
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
                    .context_menu(file_context_menu(cx.entity())),
            )
        }))
}

/// 构建文件行使用的官方右键菜单。
///
/// 菜单项通过 `PopupMenuItem::on_click` 直接调用应用动作，避免再维护一套
/// 自绘菜单和透明关闭层；菜单的定位、键盘导航、分隔线和阴影由组件库负责。
fn file_context_menu(
    app: gpui_kit::Entity<LumarkApp>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, _| {
        menu.item(popup_action(
            "打开",
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
        .item(popup_action(
            "压缩所选项目",
            AppIcon::Compression,
            &app,
            window,
            |app, window, cx| app.open_compress_dialog(window, cx),
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
            "删除",
            AppIcon::Delete,
            &app,
            window,
            |app, _, _| app.request_delete(),
        ))
    }
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
