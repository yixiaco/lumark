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
            cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                if event.click_count() == 1 {
                    this.navigate_up(window, cx);
                    cx.notify();
                }
            }),
        )
}

/// 渲染一行平铺项目，每项对应文件或文件夹。
pub(super) fn render_tile_row(
    app: &mut LumarkApp,
    cx: &mut Context<LumarkApp>,
    virtual_index: usize,
    tile_columns: usize,
    row_height: gpui_kit::Pixels,
) -> impl IntoElement {
    let app_entity = cx.entity();
    let parent_offset = usize::from(shows_parent_row(app));
    let row_count = app.displayed_count() + parent_offset;
    let start = virtual_index * tile_columns;
    let end = (start + tile_columns).min(row_count);
    h_flex()
        .id(("tile-row", virtual_index))
        .w_full()
        .h(row_height)
        .gap_2()
        .px_3()
        .children((start..end).filter_map(|index| {
            if parent_offset == 1 && index == 0 {
                return Some(render_parent_tile(cx).into_any_element());
            }
            let entry = app.displayed_entry_at(index - parent_offset)?;
            let id = entry.id.clone();
            let mouse_id = id.clone();
            let double_id = id.clone();
            let context_id = id.clone();
            let drag_app = app_entity.clone();
            let selected = app.browser.selection.is_selected(&id);
            let icon_color = entry_icon_color(cx, &entry);
            let tile_icon = if entry.is_dir {
                AppIcon::Folder
            } else {
                entry_icon(&entry)
            };
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
                            .text_center()
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
                        this.on_drag(item, move |item: &ArchiveDragItem, _, window, cx| {
                            drag_app.update(cx, |app, cx| {
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
                    .into_any_element(),
            )
        }))
}
