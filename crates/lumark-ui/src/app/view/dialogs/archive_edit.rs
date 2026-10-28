use super::*;

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
                .unwrap_or("当前归档")
                .to_string(),
            BrowserLocation::FileSystem(_) => "当前归档".to_string(),
        };
        let summary = match (file_count, folder_count) {
            (0, folders) => format!("{folders} 个文件夹将写入 {target}，请选择更新方式："),
            (files, 0) => format!("{files} 个文件将写入 {target}，请选择同名条目的处理方式："),
            (files, folders) => {
                format!("{files} 个文件、{folders} 个文件夹将写入 {target}，请选择更新方式：")
            }
        };
        let hint = match self.archive_add_mode {
            UpdateMode::Replace => "同名条目直接覆盖，新文件全部加入。",
            UpdateMode::Newer => "仅当磁盘文件比归档条目更新时替换，较旧条目保留原内容。",
            UpdateMode::Freshen => "只更新归档中已存在的条目，不会加入新文件。",
            UpdateMode::Sync => {
                "同步会新增缺少的文件、更新较新文件，并删除归档中磁盘已不存在的条目。"
            }
        };
        let hint_color = if self.archive_add_mode == UpdateMode::Sync {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };
        centered_modal(cx, "添加到归档")
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
                        "添加并替换同名文件".to_string(),
                        self.archive_add_mode == UpdateMode::Replace,
                        |app, _, _| app.archive_add_mode = UpdateMode::Replace,
                    ))
                    .child(option_radio(
                        cx,
                        "archive-add-newer",
                        "只更新较新的文件".to_string(),
                        self.archive_add_mode == UpdateMode::Newer,
                        |app, _, _| app.archive_add_mode = UpdateMode::Newer,
                    ))
                    .child(option_radio(
                        cx,
                        "archive-add-freshen",
                        "仅刷新已有条目".to_string(),
                        self.archive_add_mode == UpdateMode::Freshen,
                        |app, _, _| app.archive_add_mode = UpdateMode::Freshen,
                    ))
                    .when(folder_count > 0, |this| {
                        this.child(option_radio(
                            cx,
                            "archive-add-sync",
                            "同步文件夹内容（会删除多余条目）".to_string(),
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
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_archive_add(cx);
                            })),
                    )
                    .child(
                        button("archive-add-confirm")
                            .primary()
                            .label("开始添加")
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
        centered_modal(cx, "打开归档中的可执行文件？")
            .child(format!(
                "该条目已解压到临时目录，但可执行文件或脚本可能修改系统。\n{path}"
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("open-executable-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending_open_target = None;
                                this.show_open_executable_confirm = false;
                                this.status = "已取消打开可执行文件".to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        button("open-executable-confirm")
                            .danger()
                            .label("仍然打开")
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
            "从归档删除"
        } else {
            "移入回收站"
        };
        let subtitle = if archive {
            "归档会被重建，操作失败时原归档保持不变"
        } else {
            "项目将进入 Windows 回收站"
        };
        let description = if archive {
            "这些项目将从当前归档中永久移除。删除过程会重建归档，请不要同时进行其他写入操作。"
        } else {
            "删除后可以在回收站中还原；清空回收站后将无法恢复。"
        };
        destructive_confirm_card(cx, title, subtitle, "个项目", count, names, description).child(
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
                        .label("取消")
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
                            "从归档删除"
                        } else {
                            "移入回收站"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete();
                            cx.notify();
                        })),
                ),
        )
    }
}
