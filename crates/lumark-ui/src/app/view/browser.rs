use super::super::*;
use super::components::{AppIcon, card, entry_icon, icon_button};
use gpui_kit::base::TestSupportExt as _;

impl LumarkApp {
    /// 渲染主文件浏览器，包括虚拟列表和可选的详情检查器。
    ///
    /// 选择按稳定条目 ID 跟踪，而不是行索引，因此排序、过滤和虚拟列表回收
    /// 都不会让选择静默跳到其他文件。
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
        let details = self.details_visible && window_width >= 1180.;
        let main_width = (window_width - 220. - 32. - if details { 252. } else { 0. }).max(420.);
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
        let row_height = if tile_view { px(92.) } else { px(34.) };
        let item_sizes = Rc::new(vec![
            gpui_kit::size(px(main_width), row_height);
            virtual_rows
        ]);
        let selected_count = self.browser.selection.count();
        let overflow_target = self
            .browser
            .selection
            .focus()
            .map(str::to_owned)
            .or_else(|| self.displayed_entry_at(0).map(|entry| entry.id))
            .unwrap_or_default();
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
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        v_flex()
                            .gap(px(0.))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().text_lg().font_semibold().child("文件"))
                                    .child(
                                        div()
                                            .px_2()
                                            .py(px(1.))
                                            .rounded_full()
                                            .bg(cx.theme().muted)
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{entry_count} 项")),
                                    )
                                    .when(selected_count > 0, |this| {
                                        this.child(
                                            div()
                                                .px_2()
                                                .py(px(1.))
                                                .rounded_full()
                                                .bg(cx.theme().accent)
                                                .text_color(cx.theme().accent_foreground)
                                                .text_xs()
                                                .child(format!("已选 {selected_count}")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if search_active {
                                        format!("搜索结果 · {total_size} 字节")
                                    } else {
                                        self.browser.current_display()
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .ml_auto()
                            .gap_1()
                            .child(
                                Button::new("column-layout")
                                    .ghost()
                                    .small()
                                    .label(match self.column_layout {
                                        0 => "紧凑列",
                                        2 => "宽松列",
                                        _ => "标准列",
                                    })
                                    .tooltip("切换列宽")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cycle_column_layout();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("browser-delete", AppIcon::Delete, "删除所选")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_delete();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("browser-more", AppIcon::More, "更多操作").on_click(
                                    cx.listener(
                                        move |this, event: &gpui_kit::ClickEvent, _, cx| {
                                            this.show_file_context_menu(
                                                &overflow_target,
                                                event.position(),
                                            );
                                            cx.notify();
                                        },
                                    ),
                                ),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .min_w_0()
                    .items_stretch()
                    .gap_3()
                    .child(
                        card(cx)
                            .w(px((main_width - 12.).max(360.)))
                            .flex_shrink_0()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                h_flex()
                                    .w_full()
                                    .h(px(38.))
                                    .px_4()
                                    .gap_2()
                                    .items_center()
                                    .bg(cx.theme().list_head)
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if tile_view {
                                        div().child("名称与类型")
                                    } else {
                                        h_flex()
                                            .w_full()
                                            .gap_2()
                                            .child(div().flex_1().child("名称"))
                                            .child(div().w(px(size_width)).child("大小"))
                                            .child(div().w(px(modified_width)).child("修改时间"))
                                            .child(div().w(px(kind_width)).child("类型"))
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
                                        .child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left_0()
                                                .right_0()
                                                .h(px(1.))
                                                .bg(cx.theme().border),
                                        ),
                                )
                            }),
                    )
                    .when(details, |this| this.child(self.render_details(cx))),
            )
    }

    /// 渲染所选项目的元数据、压缩信息和加密状态，不阻塞文件系统读取。
    pub(crate) fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected_entry();
        let has_selection = selected.is_some();
        v_flex()
            .id("details-panel")
            .w(px(252.))
            .flex_shrink_0()
            .min_h_0()
            .overflow_y_scroll()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .child(
                h_flex()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(AppIcon::Doc.element(16.))
                    .child(div().text_sm().font_semibold().child("详细信息")),
            )
            .when_some(selected, |this, entry| {
                this.child(
                    v_flex()
                        .p_4()
                        .gap_3()
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .child(
                                    div()
                                        .size(px(48.))
                                        .flex_shrink_0()
                                        .rounded_lg()
                                        .bg(cx.theme().accent)
                                        .text_color(cx.theme().accent_foreground)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(entry_icon(&entry).element(26.)),
                                )
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .child(entry.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(entry.kind.clone()),
                                        ),
                                ),
                        )
                        .child(div().h(px(1.)).bg(cx.theme().border))
                        .child(detail_line(cx, "位置", &entry.path.display().to_string()))
                        .child(detail_line(cx, "大小", &format_size(entry.size)))
                        .when_some(entry.modified, |this, modified| {
                            this.child(detail_line(
                                cx,
                                "修改时间",
                                &modified.format("%Y-%m-%d %H:%M:%S").to_string(),
                            ))
                        })
                        .when(entry.encrypted, |this| {
                            this.child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .rounded_md()
                                    .bg(cx.theme().warning)
                                    .text_color(cx.theme().warning_foreground)
                                    .p_2()
                                    .child(AppIcon::FileLock.element(15.))
                                    .child(div().text_xs().font_semibold().child("加密条目")),
                            )
                        })
                        .when_some(entry.archive_entry.as_ref(), |this, archive| {
                            this.child(div().h(px(1.)).bg(cx.theme().border))
                                .child(detail_line(
                                    cx,
                                    "压缩后",
                                    &archive
                                        .compressed_size
                                        .map(format_size)
                                        .unwrap_or_else(|| "—".to_string()),
                                ))
                                .child(detail_line(cx, "压缩方法", &archive.method))
                                .child(detail_line(
                                    cx,
                                    "CRC32",
                                    &archive
                                        .crc32
                                        .map(|value| format!("{value:08X}"))
                                        .unwrap_or_else(|| "—".to_string()),
                                ))
                                .child(detail_line(
                                    cx,
                                    "加密",
                                    if archive.encrypted { "是" } else { "否" },
                                ))
                        }),
                )
            })
            .when(!has_selection, |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .p_5()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            div()
                                .size(px(44.))
                                .rounded_full()
                                .bg(cx.theme().muted)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(AppIcon::Search.element(20.)),
                        )
                        .child(div().text_sm().font_semibold().child("选择一个项目"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .text_center()
                                .child("这里会显示大小、时间、CRC 和压缩方法。"),
                        ),
                )
            })
    }

    /// 渲染文件行右键菜单。
    ///
    /// 透明全窗口层会在外部点击时关闭菜单；菜单自身会遮挡该处理器，
    /// 确保命令仍可操作。
    pub(crate) fn render_file_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let position = self
            .context_menu_position
            .unwrap_or_else(|| gpui_kit::point(px(120.), px(120.)));
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.show_context_menu = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .absolute()
                    .left(position.x)
                    .top(position.y)
                    .w(px(208.))
                    .p_1()
                    .gap(px(2.))
                    .occlude()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .shadow_lg()
                    .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(|_, _, _, _| {}))
                    .child(context_action(
                        "context-open",
                        AppIcon::FolderOpen,
                        "打开",
                        false,
                        cx,
                        |this, window, cx| {
                            this.show_context_menu = false;
                            this.open_selected(window, cx);
                        },
                    ))
                    .child(context_action(
                        "context-extract",
                        AppIcon::Zip,
                        "解压到…",
                        false,
                        cx,
                        |this, window, cx| {
                            this.show_context_menu = false;
                            this.open_extract_dialog(window, cx);
                        },
                    ))
                    .child(context_action(
                        "context-compress",
                        AppIcon::Compression,
                        "压缩所选项目",
                        false,
                        cx,
                        |this, window, cx| {
                            this.show_context_menu = false;
                            this.open_compress_dialog(window, cx);
                        },
                    ))
                    .child(div().mx_2().my_1().h(px(1.)).bg(cx.theme().border))
                    .child(context_action(
                        "context-rename",
                        AppIcon::Edit,
                        "重命名",
                        false,
                        cx,
                        |this, window, cx| {
                            this.show_context_menu = false;
                            this.start_rename(window, cx);
                        },
                    ))
                    .child(context_action(
                        "context-copy-path",
                        AppIcon::Copy,
                        "复制路径",
                        false,
                        cx,
                        |this, _window, cx| {
                            this.show_context_menu = false;
                            this.copy_selected_path(cx);
                        },
                    ))
                    .child(context_action(
                        "context-reveal",
                        AppIcon::Search,
                        "在资源管理器中显示",
                        false,
                        cx,
                        |this, _window, _cx| {
                            this.show_context_menu = false;
                            this.reveal_selected();
                        },
                    ))
                    .child(div().mx_2().my_1().h(px(1.)).bg(cx.theme().border))
                    .child(context_action(
                        "context-delete",
                        AppIcon::Delete,
                        "删除",
                        true,
                        cx,
                        |this, _window, _cx| {
                            this.show_context_menu = false;
                            this.request_delete();
                        },
                    )),
            )
    }
}

/// 渲染详情列表中的一个虚拟行。
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
    let row_id = id.clone();
    let mouse_id = id.clone();
    let double_id = id.clone();
    let context_id = id.clone();
    let selected = app.browser.selection.is_selected(&id);
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
        .px_4()
        .border_b_1()
        .border_color(cx.theme().border)
        .cursor_pointer()
        .when(selected, |this| this.bg(cx.theme().list_active))
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().list_hover))
        })
        .child(
            div()
                .size(px(24.))
                .flex_shrink_0()
                .rounded_md()
                .bg(if selected {
                    cx.theme().accent
                } else {
                    cx.theme().muted
                })
                .text_color(if selected {
                    cx.theme().accent_foreground
                } else {
                    cx.theme().muted_foreground
                })
                .flex()
                .items_center()
                .justify_center()
                .child(entry_icon(&entry).element(14.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_sm()
                .font_semibold()
                .child(entry.name.clone()),
        )
        .child(
            div()
                .w(px(size_width))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format_size(entry.size)),
        )
        .child(
            div()
                .w(px(modified_width))
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child(modified),
        )
        .child(
            div()
                .w(px(kind_width))
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
            cx.listener(move |this, event: &gpui_kit::MouseDownEvent, _, cx| {
                this.browser.selection.select_only(&context_id);
                this.show_file_context_menu(&row_id, event.position);
                cx.notify();
            }),
        )
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
                                    .size(px(28.))
                                    .rounded_md()
                                    .bg(cx.theme().accent)
                                    .text_color(cx.theme().accent_foreground)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(entry_icon(&entry).element(16.)),
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
                        cx.listener(move |this, event: &gpui_kit::MouseDownEvent, _, cx| {
                            this.browser.selection.select_only(&context_id);
                            this.show_file_context_menu(&id, event.position);
                            cx.notify();
                        }),
                    ),
            )
        }))
}

/// 渲染详情面板中的带标签值。
fn detail_line(cx: &Context<LumarkApp>, label: &str, value: &str) -> gpui_kit::Div {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_sm()
                .overflow_hidden()
                .whitespace_normal()
                .child(value.to_string()),
        )
}

/// 构建带图标且点击区域一致的右键菜单操作。
fn context_action(
    id: &'static str,
    icon: AppIcon,
    label: &'static str,
    danger: bool,
    cx: &mut Context<LumarkApp>,
    action: impl Fn(&mut LumarkApp, &mut Window, &mut Context<LumarkApp>) + 'static,
) -> impl IntoElement {
    let mut button = Button::new(id)
        .ghost()
        .w_full()
        .justify_start()
        .icon(icon.element(15.))
        .label(label);
    if danger {
        button = button.danger();
    }
    button.on_click(cx.listener(move |this, _, window, cx| {
        action(this, window, cx);
        cx.notify();
    }))
}
