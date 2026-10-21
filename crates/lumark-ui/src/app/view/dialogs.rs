use super::super::*;
use super::components::AppIcon;

impl LumarkApp {
    /// 提示保存成功打开归档的密码。
    pub(crate) fn render_save_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "保存解压密码")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("密码将写入本地加密密码库，下次可直接用于同类压缩包。"),
            )
            .child(Input::new(&self.password_label).id("save-label").w_full())
            .child(Input::new(&self.password_value).id("save-value").w_full())
            .child(
                Input::new(&self.password_pattern)
                    .id("save-pattern")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(button("save-dismiss").secondary().label("不保存").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.show_save_prompt = false;
                            cx.notify();
                        }),
                    ))
                    .child(
                        button("save-confirm")
                            .primary()
                            .icon(AppIcon::Save.element(16.))
                            .label("保存密码")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_password(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 在自动密码尝试开始前请求密码库主密码。
    pub(crate) fn render_unlock_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "解锁密码库")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("解锁后只会尝试密码库中的记录密码，不会显示密码内容。"),
            )
            .child(
                Input::new(&self.master_extract_unlock)
                    .id("unlock-for-extract")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(button("unlock-cancel").label("取消").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.show_unlock_for_extract = false;
                            this.pending_unlock_archive = None;
                            this.unlock_for_browse = false;
                            cx.notify();
                        },
                    )))
                    .child(
                        button("unlock-confirm")
                            .primary()
                            .label("解锁并自动尝试")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.unlock_for_extract(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 自动密码搜索超过前 32 个候选密码时请求确认。
    pub(crate) fn render_extra_password_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self
            .pending_extra_password
            .as_ref()
            .map_or(0, |job| job.candidates.len());
        centered_modal(cx, "尝试其余记录密码？")
            .child(format!(
                "前 32 条记录密码均不正确，密码库中还有 {count} 条候选。\
                 继续尝试可能需要较长时间；也可以停止自动尝试并手动输入。"
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("extra-passwords-decline")
                            .label("手动输入")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.decline_extra_password_candidates(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        button("extra-passwords-try")
                            .primary()
                            .label(format!("尝试其余 {count} 条"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.try_extra_password_candidates(window, cx);
                                cx.notify();
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

    /// 收集当前位置中新文件夹的名称。
    pub(crate) fn render_new_folder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "新建文件夹")
            .child(
                Input::new(&self.new_folder_name)
                    .id("new-folder-name")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("new-folder-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_new_folder = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("new-folder-create")
                            .primary()
                            .label("创建")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_folder(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 收集所选文件或归档条目的新名称。
    pub(crate) fn render_rename(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "重命名")
            .child(Input::new(&self.rename_name).id("rename-name").w_full())
            .child(
                h_flex()
                    .gap_2()
                    .child(button("rename-cancel").label("取消").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.show_rename = false;
                            this.rename_target = None;
                            cx.notify();
                        },
                    )))
                    .child(button("rename-confirm").primary().label("重命名").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.confirm_rename(window, cx);
                            cx.notify();
                        }),
                    )),
            )
    }

    /// 使用独立口令导入或导出加密密码库。
    pub(crate) fn render_vault_transfer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "加密密码库导入 / 导出")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "导出文件使用独立口令和 Argon2id + XChaCha20-Poly1305，不提供明文 CSV。",
                    ),
            )
            .child(
                Input::new(&self.vault_transfer_path)
                    .id("vault-transfer-path")
                    .w_full(),
            )
            .child(
                Input::new(&self.vault_transfer_passphrase)
                    .id("vault-transfer-passphrase")
                    .w_full(),
            )
            .child(
                button("toggle-import-replace")
                    .small()
                    .cursor_pointer()
                    .label(if self.vault_transfer_replace {
                        "导入策略：替换现有记录"
                    } else {
                        "导入策略：合并去重"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vault_transfer_replace = !this.vault_transfer_replace;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("vault-export")
                            .primary()
                            .label("导出")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.export_vault(cx);
                                cx.notify();
                            })),
                    )
                    .child(button("vault-import").label("导入").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.import_vault(cx);
                            cx.notify();
                        },
                    )))
                    .child(
                        button("vault-transfer-cancel")
                            .label("关闭")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_vault_transfer = false;
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 删除主密码前验证当前主密码，并让密码库恢复 DPAPI 包裹。
    pub(crate) fn render_confirm_remove_master(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "删除主密码")
            .child(
                "删除后密码库将改由与当前计算机绑定的 Windows DPAPI 保护。请输入当前主密码确认。",
            )
            .child(
                Input::new(&self.master_remove)
                    .id("confirm-remove-master")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("cancel-remove-master")
                            .label("取消")
                            .on_click(cx.listener(|this, _, window, cx| {
                                set_input(&this.master_remove, "", window, cx);
                                this.show_confirm_remove_master = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("confirm-remove-master")
                            .danger()
                            .label("确认删除")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_remove_master(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 警告清除密码库不可恢复。
    pub(crate) fn render_confirm_clear_vault(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "清除密码库")
            .child("此操作会删除密码库中的所有记录，无法撤销。建议先加密导出备份。")
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("cancel-clear-vault")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_confirm_clear_vault = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("confirm-clear-vault")
                            .danger()
                            .label("确认清除")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_vault(cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 确认从密码库永久删除当前记录。
    pub(crate) fn render_confirm_delete_password(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self
            .selected_password
            .and_then(|id| self.find_password(id))
            .map(|entry| entry.display_name().to_string())
            .unwrap_or_else(|| "所选密码".to_string());
        destructive_confirm_card(
            cx,
            "删除密码记录",
            "记录将从本地加密密码库移除",
            "条记录",
            1,
            vec![label],
            "删除后无法恢复，但不会影响已经解压的文件。",
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
                    button("cancel-delete-password")
                        .secondary()
                        .label("取消")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_confirm_delete_password = false;
                            cx.notify();
                        })),
                )
                .child(
                    button("confirm-delete-password")
                        .danger()
                        .icon(AppIcon::Delete.element(14.))
                        .label("删除记录")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete_password();
                            cx.notify();
                        })),
                ),
        )
    }
}

/// 构建文件或密码删除共用的危险确认卡片。
///
/// 顶部按危险级别着色的图标、待删除项目清单和底部操作区保持一致，
/// 避免“移入回收站”和“删除密码记录”出现两种不同的确认样式。
fn destructive_confirm_card(
    cx: &Context<LumarkApp>,
    title: &str,
    subtitle: &str,
    unit: &str,
    count: usize,
    names: Vec<String>,
    description: &str,
) -> gpui_kit::Div {
    let names: Vec<String> = names.into_iter().take(3).collect();
    let hidden = count.saturating_sub(names.len());
    v_flex()
        .w(px(460.))
        .overflow_hidden()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(14.))
        .shadow_xl()
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .p_5()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .size(px(40.))
                        .flex_shrink_0()
                        .rounded(px(12.))
                        .bg(cx.theme().danger.opacity(0.14))
                        .text_color(cx.theme().danger_foreground)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(AppIcon::Delete.element(20.)),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .gap(px(2.))
                        .child(div().text_lg().font_semibold().child(title.to_string()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .whitespace_normal()
                                .child(subtitle.to_string()),
                        ),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p_5()
                .gap_3()
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .p_3()
                        .rounded(px(8.))
                        .bg(cx.theme().muted)
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_color(cx.theme().danger_foreground)
                                        .child(AppIcon::Delete.element(14.)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .child(format!("{count} {unit}")),
                                ),
                        )
                        .children(names.into_iter().map(|name| {
                            div()
                                .w_full()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("· {name}"))
                        }))
                        .when(hidden > 0, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("还有 {hidden} {unit}…")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(description.to_string()),
                ),
        )
}
