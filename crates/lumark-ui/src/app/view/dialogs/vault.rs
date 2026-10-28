use super::*;

impl LumarkApp {
    /// 使用独立口令导入或导出加密密码库，并可导出明文 CSV。
    ///
    /// 主密码保护的密码库锁定时不保留记录，因此弹窗先提供解锁入口；
    /// 解锁后同一层立即切换为导入/导出表单。
    pub(crate) fn render_vault_transfer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.vault.state() == VaultState::Locked;
        centered_modal(cx, "加密密码库导入 / 导出")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "加密备份使用独立口令和 Argon2id + XChaCha20-Poly1305；CSV 为明文，任何程序都能读取。",
                    ),
            )
            .when(locked, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(cx.theme().warning.opacity(0.45))
                        .bg(cx.theme().warning.opacity(0.14))
                        .p_3()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(AppIcon::Lock.element(15.))
                                .child(div().text_sm().font_semibold().child("密码库已锁定")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("锁定时不保留记录，导入、导出和 CSV 都需要先解锁。"),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(
                                    Input::new(&self.vault_transfer_unlock)
                                        .id("vault-transfer-unlock")
                                        .flex_1()
                                        .min_w_0(),
                                )
                                .child(
                                    button("vault-transfer-unlock-confirm")
                                        .primary()
                                        .label("解锁")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.unlock_vault_transfer(window, cx);
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
            .when(!locked, |this| {
                this.child(labeled_input(
                    cx,
                    "导出位置",
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            Input::new(&self.vault_export_dir)
                                .id("vault-export-dir")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            button("vault-export-browse")
                                .ghost()
                                .icon(AppIcon::FolderOpen.element(15.))
                                .label("浏览…")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.browse_vault_export_dir(cx);
                                })),
                        ),
                ))
                .child(hint_text(
                    cx,
                    "导出文件写入所选文件夹，文件名自动附带本地时间戳，不覆盖旧备份。",
                ))
                .child(labeled_input(
                    cx,
                    "独立口令",
                    Input::new(&self.vault_transfer_passphrase)
                        .id("vault-transfer-passphrase")
                        .w_full(),
                ))
                .child(labeled_input(
                    cx,
                    "导入文件",
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            Input::new(&self.vault_import_path)
                                .id("vault-import-path")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            button("vault-import-browse")
                                .ghost()
                                .icon(AppIcon::FolderOpen.element(15.))
                                .label("浏览…")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.browse_vault_import(cx);
                                })),
                        ),
                ))
                .child(
                    button("toggle-import-replace")
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
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .flex_wrap()
                    .when(!locked, |this| {
                        this.child(
                            button("vault-export")
                                .primary()
                                .icon(AppIcon::Save.element(15.))
                                .label("导出加密备份")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_vault(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            button("vault-export-csv")
                                .warning()
                                .icon(AppIcon::Doc.element(15.))
                                .label("导出明文 CSV")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_vault_csv(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            button("vault-import")
                                .label("导入")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.import_vault(window, cx);
                                    cx.notify();
                                })),
                        )
                    })
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
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_vault(window, cx);
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
