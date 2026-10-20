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
