//! 归档添加、可执行文件确认和删除确认。

use super::*;
use crate::i18n::tr;
use gpui_kit::component::radio::Radio;

impl LumarkApp {
    /// 使用官方 Dialog 让用户选择归档添加/同步的更新模式。
    pub(crate) fn request_archive_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let state = app.read(cx);
            let file_count = state.pending_archive_additions.len();
            let folder_count = state.pending_archive_roots.len();
            let mode = state.archive_add_mode;
            let target = match &state.browser.location {
                BrowserLocation::Archive { archive, .. } => archive
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(tr("当前归档", "Current archive"))
                    .to_string(),
                BrowserLocation::FileSystem(_) => tr("当前归档", "Current archive").to_string(),
            };
            let summary = match (file_count, folder_count) {
                (0, folders) => format!(
                    "{folders} {}{target}{}",
                    tr(" 个文件夹将写入 ", " folders will be written to "),
                    tr("，请选择更新方式：", "; choose an update mode:")
                ),
                (files, 0) => format!(
                    "{files} {}{target}{}",
                    tr(" 个文件将写入 ", " files will be written to "),
                    tr(
                        "，请选择同名条目的处理方式：",
                        "; choose how to handle same-named entries:"
                    )
                ),
                (files, folders) => format!(
                    "{files} {}{folders} {}{target}{}",
                    tr(" 个文件、", " files, "),
                    tr(" 个文件夹将写入 ", " folders will be written to "),
                    tr("，请选择更新方式：", "; choose an update mode:")
                ),
            };
            let hint = match mode {
                UpdateMode::Replace => tr(
                    "同名条目直接覆盖，新文件全部加入。",
                    "Overwrite same-named entries and add all new files.",
                ),
                UpdateMode::Newer => tr(
                    "仅当磁盘文件比归档条目更新时替换，较旧条目保留原内容。",
                    "Replace only when the disk file is newer; keep older entries unchanged.",
                ),
                UpdateMode::Freshen => tr(
                    "只更新归档中已存在的条目，不会加入新文件。",
                    "Update only existing entries; do not add new files.",
                ),
                UpdateMode::Sync => tr(
                    "同步会新增缺少的文件、更新较新文件，并删除归档中磁盘已不存在的条目。",
                    "Sync adds missing files, updates newer files and removes entries no longer on disk.",
                ),
            };
            let hint_color = if mode == UpdateMode::Sync {
                cx.theme().warning
            } else {
                cx.theme().muted_foreground
            };
            let app_cancel = app.clone();
            let app_confirm = app.clone();
            let mut dialog = dialog
                .title(tr("添加到归档", "Add to archive"))
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(summary),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(mode_radio(
                            app.clone(),
                            "archive-add-replace",
                            tr("添加并替换同名文件", "Add and replace same-named files"),
                            mode == UpdateMode::Replace,
                            UpdateMode::Replace,
                        ))
                        .child(mode_radio(
                            app.clone(),
                            "archive-add-newer",
                            tr("只更新较新的文件", "Update newer files only"),
                            mode == UpdateMode::Newer,
                            UpdateMode::Newer,
                        ))
                        .child(mode_radio(
                            app.clone(),
                            "archive-add-freshen",
                            tr("仅刷新已有条目", "Refresh existing entries only"),
                            mode == UpdateMode::Freshen,
                            UpdateMode::Freshen,
                        ))
                        .when(folder_count > 0, |this| {
                            this.child(mode_radio(
                                app.clone(),
                                "archive-add-sync",
                                tr(
                                    "同步文件夹内容（会删除多余条目）",
                                    "Sync folder contents (removes extra entries)",
                                ),
                                mode == UpdateMode::Sync,
                                UpdateMode::Sync,
                            ))
                        }),
                )
                .child(div().w_full().text_xs().text_color(hint_color).child(hint));
            dialog = dialog.footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("archive-add-cancel")
                            .secondary()
                            .label(tr("取消", "Cancel"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                app_cancel.update(cx, |app, cx| app.cancel_archive_add(cx));
                            }),
                    )
                    .child(
                        button("archive-add-confirm")
                            .primary()
                            .label(tr("开始添加", "Start adding"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                app_confirm.update(cx, |app, cx| app.confirm_archive_add(cx));
                            }),
                    ),
            );
            dialog
        });
    }
}

/// 生成归档更新模式单选项，点击后刷新官方对话框内容。
fn mode_radio(
    app: gpui_kit::Entity<LumarkApp>,
    id: &'static str,
    label: &'static str,
    checked: bool,
    mode: UpdateMode,
) -> impl IntoElement {
    Radio::new(id)
        .label(label.to_string())
        .checked(checked)
        .on_click(move |_, window, cx| {
            app.update(cx, |app, cx| {
                app.archive_add_mode = mode;
                cx.notify();
            });
            window.refresh();
        })
}
