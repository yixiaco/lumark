use super::super::*;
use super::components::{
    AppIcon, classic_field, dialog_banner, hint_text, icon_button, option_check,
};

impl LumarkApp {
    /// 渲染解压目标、可选手动密码和仅解压所选条目的控件。
    pub(crate) fn render_extract_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let archive = self
            .selected_archive()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.browser.current_display());
        dialog_frame(cx, 620.)
            .child(dialog_caption(
                cx,
                "extract-close",
                AppIcon::Zip,
                "解压文件",
                |this, _, _| {
                    this.show_extract = false;
                    this.status = "已取消解压".to_string();
                },
            ))
            .child(dialog_banner(cx, 56.))
            .child(
                v_flex()
                    .p_5()
                    .gap_4()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .rounded(px(5.))
                            .bg(cx.theme().muted)
                            .px_3()
                            .py_2()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(cx.theme().accent_foreground)
                                    .child(AppIcon::Zip.element(16.)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .child(archive),
                            ),
                    )
                    .child(classic_field(
                        "解压到:",
                        66.,
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                Input::new(&self.extract_output)
                                    .id("extract-output")
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                Button::new("extract-choose-dir")
                                    .small()
                                    .label("浏览…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_extract_output_dir(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ))
                    .child(classic_field(
                        "密码:",
                        66.,
                        Input::new(&self.extract_password)
                            .id("extract-password")
                            .w_full(),
                    ))
                    .child(h_flex().w_full().gap_2().items_center().child(option_check(
                        cx,
                        "extract-selection-only",
                        "仅解压选中的归档条目".to_string(),
                        self.extract_selection_only,
                        |this, _, _| this.extract_selection_only = !this.extract_selection_only,
                    )))
                    .child(hint_text(
                        cx,
                        "记录密码会自动尝试，失败后可手动输入；密码不会显示在界面中。",
                    ))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .pt_1()
                            .child(Button::new("extract-cancel").label("取消").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.show_extract = false;
                                    this.status = "已取消解压".to_string();
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("extract-start")
                                    .primary()
                                    .icon(AppIcon::Zip.element(15.))
                                    .label("开始解压")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_extract(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

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
                Input::new(&self.password_confirm)
                    .id("save-confirm-value")
                    .w_full(),
            )
            .child(
                Input::new(&self.password_pattern)
                    .id("save-pattern")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-dismiss")
                            .secondary()
                            .label("不保存")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_save_prompt = false;
                                this.pending_save = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("save-confirm")
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
                Input::new(&self.master_current)
                    .id("unlock-for-extract")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("unlock-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_unlock_for_extract = false;
                                this.pending_unlock_archive = None;
                                this.unlock_for_browse = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("unlock-confirm")
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
                        Button::new("extra-passwords-decline")
                            .label("手动输入")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.decline_extra_password_candidates(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("extra-passwords-try")
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
                        Button::new("open-executable-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending_open_target = None;
                                this.show_open_executable_confirm = false;
                                this.status = "已取消打开可执行文件".to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("open-executable-confirm")
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
        centered_modal(
            cx,
            if archive {
                "从归档删除"
            } else {
                "移入回收站"
            },
        )
        .child(if archive {
            format!(
                "将从当前归档中删除 {} 个项目，并重建归档。失败时原归档保持不变。",
                self.pending_delete_paths.len()
            )
        } else {
            format!(
                "将删除 {} 个项目。默认进入 Windows 回收站，可在资源管理器中恢复。",
                self.pending_delete_paths.len()
            )
        })
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("delete-cancel")
                        .label("取消")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_delete_confirm = false;
                            this.pending_delete_paths.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("delete-confirm")
                        .danger()
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
                        Button::new("new-folder-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_new_folder = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("new-folder-create")
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
                    .child(
                        Button::new("rename-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_rename = false;
                                this.rename_target = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("rename-confirm")
                            .primary()
                            .label("重命名")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_rename(window, cx);
                                cx.notify();
                            })),
                    ),
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
                Button::new("toggle-import-replace")
                    .small()
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
                        Button::new("vault-export")
                            .primary()
                            .label("导出")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.export_vault(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("vault-import")
                            .label("导入")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.import_vault(cx);
                                cx.notify();
                            })),
                    )
                    .child(Button::new("vault-transfer-cancel").label("关闭").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.show_vault_transfer = false;
                            cx.notify();
                        }),
                    )),
            )
    }

    /// 删除主密码前验证当前主密码，并让密码库恢复 DPAPI 包裹。
    pub(crate) fn render_confirm_remove_master(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "删除主密码")
            .child("删除后密码库将改由当前 Windows 用户的 DPAPI 保护。请输入当前主密码确认。")
            .child(
                Input::new(&self.master_current)
                    .id("confirm-remove-master")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("cancel-remove-master")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_confirm_remove_master = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("confirm-remove-master")
                            .danger()
                            .label("确认删除")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_remove_master(cx);
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
                        Button::new("cancel-clear-vault")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_confirm_clear_vault = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("confirm-clear-vault")
                            .danger()
                            .label("确认清除")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_vault(cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 离开密码工作区时防止未提交的密码修改意外丢失。
    pub(crate) fn render_discard_password_draft(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "放弃未保存的密码修改？")
            .child("密码管理中的添加、编辑和删除尚未写入密码库。离开前可以继续编辑或丢弃草稿。")
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("keep-password-draft")
                            .label("继续编辑")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_discard_password_draft = false;
                                this.pending_screen = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("discard-draft-and-switch")
                            .danger()
                            .label("丢弃并离开")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.discard_draft_and_switch();
                                cx.notify();
                            })),
                    ),
            )
    }
}

/// 构建压缩和解压对话框共用的分类外框。
fn dialog_frame(cx: &Context<LumarkApp>, width: f32) -> gpui_kit::Div {
    v_flex()
        .w(px(width))
        .max_h(px(700.))
        .overflow_hidden()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(8.))
        .shadow_xl()
}

/// 构建对话框顶部的窗体标题行和关闭按钮。
fn dialog_caption(
    cx: &Context<LumarkApp>,
    close_id: &'static str,
    icon: AppIcon,
    title: &str,
    close: impl Fn(&mut LumarkApp, &mut Window, &mut Context<LumarkApp>) + 'static,
) -> gpui_kit::AnyElement {
    h_flex()
        .h(px(32.))
        .w_full()
        .flex_shrink_0()
        .px_4()
        .gap_2()
        .items_center()
        .bg(cx.theme().title_bar)
        .border_b_1()
        .border_color(cx.theme().border)
        .text_color(cx.theme().foreground)
        .child(icon.element(13.))
        .child(div().text_sm().font_semibold().child(title.to_string()))
        .child(
            icon_button(close_id, AppIcon::Close, "关闭")
                .ml_auto()
                .on_click(cx.listener(move |this, _, window, cx| {
                    close(this, window, cx);
                    cx.notify();
                })),
        )
        .into_any_element()
}
