//! 文件表平铺视图。

use super::*;

/// 渲染平铺视图中的“..”卡片。
///
/// 与列表行一致，这张卡片只负责返回上一级，不进入选择集，也不参与拖出、
/// 复制、重命名或删除。
pub(super) fn render_parent_tile(cx: &mut Context<LumarkApp>) -> impl IntoElement {
    v_flex()
        .id("parent-entry-tile")
        .test_support()
        .w(px(120.))
        .h(px(120.))
        .p_2()
        .gap_2()
        .items_center()
        .rounded_lg()
        .hover(|style| style.bg(cx.theme().list_hover))
        .child(
            div()
                .flex_1()
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(StatusTone::Warning.color(cx))
                .child(AppIcon::FolderOpen.element(48.)),
        )
        .child(
            div()
                .id("parent-entry-tile-name")
                .w_full()
                .text_sm()
                .text_center()
                .whitespace_normal()
                .line_clamp(2)
                .child(".."),
        )
        .on_click(
            cx.listener(move |this, _event: &gpui_kit::ClickEvent, window, cx| {
                // 此处就是点击就返回，不要进行点击次数判断
                this.navigate_up(window, cx);
                cx.notify();
            }),
        )
}

/// 渲染一行平铺项目，每项对应文件或文件夹；卡片宽度按可用宽度均分。
pub(super) fn render_tile_row(
    app: &LumarkApp,
    cx: &mut Context<LumarkApp>,
    virtual_index: usize,
    order: &DisplayedOrder,
    layout: &ListLayout,
    drag_selection: &Rc<Vec<(String, bool)>>,
) -> impl IntoElement {
    let parent_offset = usize::from(layout.parent_row);
    let row_count = order.len() + parent_offset;
    let start = virtual_index * layout.tile_columns;
    let end = (start + layout.tile_columns).min(row_count);
    h_flex()
        .id(("tile-row", virtual_index))
        .w_full()
        .h(layout.row_height)
        .gap_2()
        .px_3()
        .children((start..end).filter_map(|index| {
            if parent_offset == 1 && index == 0 {
                return Some(render_parent_tile(cx).into_any_element());
            }
            let entry = order
                .source_index(index - parent_offset)
                .and_then(|source_index| app.displayed_source().get(source_index))?
                .clone();
            let id = entry.id.clone();
            let mouse_id = id.clone();
            let double_id = id.clone();
            let context_id = id.clone();
            let selected = app.browser.selection.is_selected(&id);
            let tile_icon_kind = entry_icon(app, &entry);
            let icon_color = entry_icon_color(cx, &entry, tile_icon_kind);
            let tile_icon = if entry.is_dir {
                AppIcon::Folder
            } else {
                tile_icon_kind
            };
            let drag_item = layout
                .inside_archive
                .then(|| archive_drag_item(app, &entry, drag_selection))
                .flatten();
            // 图片文件优先生成真实缩略图；缩略图未就绪或失败时保留类型图标。
            let tile_icon = if let Some(thumbnail) = app.tile_thumbnail(&entry, cx) {
                gpui_kit::img(thumbnail)
                    .size(px(64.))
                    .object_fit(gpui_kit::ObjectFit::Contain)
                    .into_any_element()
            } else if let Some(icon) = system_icon_for(app, &entry) {
                gpui_kit::img(icon).size(px(48.)).into_any_element()
            } else {
                tile_icon.element(48.).into_any_element()
            };
            Some(
                v_flex()
                    .id(format!("file-tile-{id}"))
                    .test_support()
                    .w(px(layout.tile_width))
                    .flex_shrink_0()
                    .h(px(120.))
                    .p_2()
                    .gap_2()
                    .items_center()
                    .rounded_lg()
                    .when(selected, |this| this.bg(cx.theme().list_active))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(cx.theme().list_hover))
                    })
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
                        h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .id(format!("file-tile-name-{id}"))
                                    .test_support()
                                    .min_w_0()
                                    .text_sm()
                                    .text_center()
                                    .whitespace_normal()
                                    .line_clamp(2)
                                    .child(entry.name.clone()),
                            )
                            .when(entry.encrypted, |this| {
                                this.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(StatusTone::Danger.color(cx))
                                        .child(AppIcon::FileLock.element(12.)),
                                )
                            }),
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
                        let move_item = item.clone();
                        this.on_drag(item, move |_item: &ArchiveDragItem, _, _, cx| {
                            cx.new(|_| ArchiveDragPreview)
                        })
                        .on_drag_move(cx.listener(
                            move |this,
                                  event: &gpui_kit::DragMoveEvent<ArchiveDragItem>,
                                  window,
                                  cx| {
                                this.handle_archive_drag_move(
                                    &move_item,
                                    event.event.position,
                                    window,
                                    cx,
                                );
                            },
                        ))
                    })
                    .context_menu(file_context_menu(
                        cx.entity(),
                        app.browser.location.clone(),
                        entry.clone(),
                    ))
                    .into_any_element(),
            )
        }))
}
