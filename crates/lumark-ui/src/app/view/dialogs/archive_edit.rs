//! 归档添加、可执行文件确认和删除确认。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 渲染添加到归档的更新模式选择卡片。
    ///
    /// 更新模式只在确认后生效，取消不会触发归档重建，因此可以先浏览选项
    /// 再决定是否写入。
    pub(crate) fn render_archive_add_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let file_count = self.pending_archive_additions.len();
        let folder_count = self.pending_archive_roots.len();
        let target = match &self.browser.location {
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
        let hint = match self.archive_add_mode {
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
        let hint_color = if self.archive_add_mode == UpdateMode::Sync {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };
        centered_modal(cx, tr("添加到归档", "Add to archive"))
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
                    .child(option_radio(
                        cx,
                        "archive-add-replace",
                        tr("添加并替换同名文件", "Add and replace same-named files").to_string(),
                        self.archive_add_mode == UpdateMode::Replace,
                        |app, _, _| app.archive_add_mode = UpdateMode::Replace,
                    ))
                    .child(option_radio(
                        cx,
                        "archive-add-newer",
                        tr("只更新较新的文件", "Update newer files only").to_string(),
                        self.archive_add_mode == UpdateMode::Newer,
                        |app, _, _| app.archive_add_mode = UpdateMode::Newer,
                    ))
                    .child(option_radio(
                        cx,
                        "archive-add-freshen",
                        tr("仅刷新已有条目", "Refresh existing entries only").to_string(),
                        self.archive_add_mode == UpdateMode::Freshen,
                        |app, _, _| app.archive_add_mode = UpdateMode::Freshen,
                    ))
                    .when(folder_count > 0, |this| {
                        this.child(option_radio(
                            cx,
                            "archive-add-sync",
                            tr(
                                "同步文件夹内容（会删除多余条目）",
                                "Sync folder contents (removes extra entries)",
                            )
                            .to_string(),
                            self.archive_add_mode == UpdateMode::Sync,
                            |app, _, _| app.archive_add_mode = UpdateMode::Sync,
                        ))
                    }),
            )
            .child(div().w_full().text_xs().text_color(hint_color).child(hint))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("archive-add-cancel")
                            .secondary()
                            .label(tr("取消", "Cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_archive_add(cx);
                            })),
                    )
                    .child(
                        button("archive-add-confirm")
                            .primary()
                            .label(tr("开始添加", "Start adding"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_archive_add(cx);
                            })),
                    ),
            )
    }

    /// 确认是否执行从归档解压到受控临时目录的可执行文件或脚本。
    pub(crate) fn render_open_executable_confirm(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = self
            .pending_open_target
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        centered_modal(cx, tr("打开归档中的可执行文件？", "Open executable from archive?"))
            .child(format!(
                "{}\n{}{path}",
                tr("该条目已解压到临时目录，但可执行文件或脚本可能修改系统。", "The entry has been extracted to a temporary folder, but executable files or scripts may modify the system."),
                tr("路径：", "Path: ")
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("open-executable-cancel")
                            .label(tr("取消", "Cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending_open_target = None;
                                this.show_open_executable_confirm = false;
                                this.status = tr("已取消打开可执行文件", "Opening executable cancelled").to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        button("open-executable-confirm")
                            .danger()
                            .label(tr("仍然打开", "Open anyway"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_open_executable(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 确认从归档中永久删除，或把普通文件系统项目移入回收站。
    pub(crate) fn render_delete_confirm(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let archive = matches!(&self.browser.location, BrowserLocation::Archive { .. });
        let count = self.pending_delete_paths.len();
        let names: Vec<String> = self
            .pending_delete_paths
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .take(3)
            .map(ToString::to_string)
            .collect();
        let title = if archive {
            tr("从归档删除", "Delete from archive")
        } else {
            tr("移入回收站", "Move to Recycle Bin")
        };
        let subtitle = if archive {
            tr(
                "归档会被重建，操作失败时原归档保持不变",
                "The archive will be rebuilt; the original stays intact on failure",
            )
        } else {
            tr(
                "项目将进入 Windows 回收站",
                "Items will be moved to the Windows Recycle Bin",
            )
        };
        let description = if archive {
            tr(
                "这些项目将从当前归档中永久移除。删除过程会重建归档，请不要同时进行其他写入操作。",
                "These items will be permanently removed. The archive is rebuilt during deletion; avoid other writes at the same time.",
            )
        } else {
            tr(
                "删除后可以在回收站中还原；清空回收站后将无法恢复。",
                "Items can be restored from the Recycle Bin until it is emptied.",
            )
        };
        destructive_confirm_card(
            cx,
            title,
            subtitle,
            tr("个项目", "items"),
            count,
            names,
            description,
        )
        .child(
            h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .p_4()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    button("delete-cancel")
                        .secondary()
                        .label(tr("取消", "Cancel"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_delete_confirm = false;
                            this.pending_delete_paths.clear();
                            cx.notify();
                        })),
                )
                .child(
                    button("delete-confirm")
                        .danger()
                        .icon(AppIcon::Delete.element(14.))
                        .label(if archive {
                            tr("从归档删除", "Delete from archive")
                        } else {
                            tr("移入回收站", "Move to Recycle Bin")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete();
                            cx.notify();
                        })),
                ),
        )
    }
}
