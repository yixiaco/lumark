//! 文件表父行、文件行和空状态。

use super::*;
use crate::i18n::tr;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};

/// 判断文件表最上方是否需要显示“..”行。
///
/// 搜索结果不是目录列表，不插入返回行；已经位于根目录（`can_go_up`
/// 为假）时也不插入，避免出现点击后没有反应的条目。
pub(super) fn shows_parent_row(app: &LumarkApp) -> bool {
    app.search_results.is_none() && app.browser.can_go_up()
}

/// 渲染空目录或空搜索结果的状态面板。
///
/// 非根目录的空文件夹仍会先渲染“..”行，用户可以从该面板上方返回上一级。
pub(super) fn empty_state(_cx: &Context<LumarkApp>, search_active: bool) -> impl IntoElement {
    Empty::new().header(
        EmptyHeader::new()
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .child(AppIcon::FolderOpen.element(17.)),
            )
            .title(EmptyTitle::new().child(if search_active {
                tr("没有匹配的项目", "No matching items")
            } else {
                tr("此文件夹为空", "This folder is empty")
            }))
            .description(EmptyDescription::new().child(tr(
                "拖入文件即可开始压缩，或切换到其他位置。",
                "Drop files here to compress, or switch to another location.",
            ))),
    )
}

/// 渲染文件表最上方用于返回上一级的“..”行。
///
/// 该行只是导航入口，不进入选择集，也不参与拖出、复制、重命名或删除，
/// 因此不会把目录占位符当成真实条目。单击立即返回上一级；双击的第二下
/// 会落在重新渲染后的同一行上，忽略它可以避免一次跳两级。
#[allow(clippy::too_many_arguments)]
pub(super) fn render_parent_row(
    cx: &mut Context<LumarkApp>,
    row_height: gpui_kit::Pixels,
    widths: ColumnWidths,
    columns: ArchiveColumns,
    inside_archive: bool,
) -> impl IntoElement {
    h_flex()
        .id("parent-entry-row")
        .test_support()
        .w_full()
        .h(row_height)
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .hover(|style| style.bg(cx.theme().list_hover))
        .child(
            div()
                .w(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(StatusTone::Warning.color(cx))
                .child(AppIcon::FolderOpen.element(15.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_sm()
                .child(".."),
        )
        .child(muted_cell(cx, widths.size, "—"))
        .when(columns.compressed, |this| {
            this.child(muted_cell(cx, widths.compressed, "—"))
        })
        .child(muted_cell(cx, widths.modified, "—"))
        .when(columns.algorithm, |this| {
            this.child(muted_cell(cx, widths.algorithm, "—"))
        })
        .when(columns.crc32, |this| {
            this.child(muted_cell(cx, widths.crc32, "—"))
        })
        .when(columns.path, |this| {
            this.child(muted_cell(cx, widths.path, "—"))
        })
        .when(!inside_archive, |this| {
            this.child(muted_cell(cx, widths.kind, tr("文件夹", "Folder")))
        })
        .on_click(
            cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                if event.click_count() == 1 {
                    this.navigate_up(window, cx);
                    cx.notify();
                }
            }),
        )
}

/// 渲染详情列表中的一个紧凑虚拟行。
#[allow(clippy::too_many_arguments)]
pub(super) fn render_file_row(
    app: &mut LumarkApp,
    cx: &mut Context<LumarkApp>,
    virtual_index: usize,
    row_height: gpui_kit::Pixels,
    widths: ColumnWidths,
    columns: ArchiveColumns,
    inside_archive: bool,
) -> impl IntoElement {
    let app_entity = cx.entity();
    let parent_offset = usize::from(shows_parent_row(app));
    if parent_offset == 1 && virtual_index == 0 {
        return render_parent_row(cx, row_height, widths, columns, inside_archive)
            .into_any_element();
    }
    let Some(entry) = app.displayed_entry_at(virtual_index - parent_offset) else {
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
    let row_icon = if let Some(icon) = system_icon_for(app, &entry) {
        gpui_kit::img(icon).size(px(16.)).into_any_element()
    } else {
        entry_icon(&entry).element(15.).into_any_element()
    };
    h_flex()
        .id(format!("file-row-{id}"))
        .test_support()
        .w_full()
        .h(row_height)
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border)
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
                .child(row_icon),
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
                .w(px(widths.size))
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
            this.child(muted_cell(cx, widths.compressed, compressed))
        })
        .child(
            div()
                .w(px(widths.modified))
                .flex_shrink_0()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child(modified),
        )
        .when(columns.algorithm, |this| {
            this.child(muted_cell(cx, widths.algorithm, algorithm))
        })
        .when(columns.crc32, |this| {
            this.child(muted_cell(cx, widths.crc32, crc32))
        })
        .when(columns.path, |this| {
            this.child(muted_cell(cx, widths.path, inner_path.clone()))
        })
        .when(!inside_archive, |this| {
            this.child(
                div()
                    .w(px(widths.kind))
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
            this.on_drag(item, move |item: &ArchiveDragItem, _, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.start_archive_drag_extract(item.clone(), window, cx);
                });
                let preview = cx.new(|_| ArchiveDragPreview {
                    label: item.label.clone(),
                });
                window.defer(cx, |window, cx| {
                    cx.stop_active_drag(window);
                });
                preview
            })
        })
        .context_menu(file_context_menu(
            cx.entity(),
            app.browser.location.clone(),
            entry.clone(),
        ))
        .into_any_element()
}

/// 渲染文件行中一个固定宽度的只读文本单元。
///
/// 归档附加列和“..”行都使用它，保证列宽和次要文字颜色与真实条目一致。
pub(super) fn muted_cell(
    cx: &Context<LumarkApp>,
    width: f32,
    text: impl Into<String>,
) -> gpui_kit::Div {
    div()
        .w(px(width))
        .flex_shrink_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
