//! 密码库导入导出、主密码和删除确认弹窗。

use super::*;
use crate::app::view::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::component::button::ButtonVariant;

impl LumarkApp {
    /// 使用官方 AlertDialog 确认清除密码库。
    pub(crate) fn request_clear_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            alert
                .title(tr("清除密码库", "Clear vault"))
                .description(tr(
                    "此操作会删除密码库中的所有记录，无法撤销。建议先加密导出备份。",
                    "This deletes all vault records and cannot be undone. Export an encrypted backup first.",
                ))
                .confirm()
                .ok_text(tr("确认清除", "Confirm clear"))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(tr("取消", "Cancel"))
                .on_ok(move |_, _, cx| {
                    let app = app.clone();
                    app.update(cx, |app, cx| {
                        app.clear_vault_entries(cx);
                    });
                    true
                })
        });
    }

    /// 使用官方 AlertDialog 确认删除当前选中的密码记录。
    pub(crate) fn request_delete_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_password else {
            return;
        };
        let label = self
            .find_password(id)
            .map(|entry| entry.display_name().to_string())
            .unwrap_or_else(|| tr("所选密码", "Selected password").to_string());
        let app = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            alert
                .title(tr("删除密码记录", "Delete password record"))
                .description(format!(
                    "{}{}",
                    tr(
                        "将从本地加密密码库移除：",
                        "Will be removed from the local vault: "
                    ),
                    label
                ))
                .confirm()
                .ok_text(tr("删除", "Delete"))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(tr("取消", "Cancel"))
                .on_ok(move |_, _, cx| {
                    let app = app.clone();
                    app.update(cx, |app, cx| {
                        app.selected_password = Some(id);
                        app.confirm_delete_password();
                        cx.notify();
                    });
                    true
                })
        });
    }

    /// 使用官方 Dialog 要求输入当前主密码后删除主密码。
    pub(crate) fn request_remove_master(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        set_input(&self.master_remove, "", window, cx);
        let app = cx.entity();
        let input = self.master_remove.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let app_confirm = app.clone();
            let input = input.clone();
            dialog
                .title(tr("删除主密码", "Remove master password"))
                .child(tr(
                    "删除后密码库将改由与当前计算机绑定的 Windows DPAPI 保护。请输入当前主密码确认。",
                    "After removal the vault returns to Windows DPAPI protection bound to this computer. Enter the current master password to confirm.",
                ))
                .child(Input::new(&input).id("confirm-remove-master").w_full())
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("cancel-remove-master")
                                .label(tr("取消", "Cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            button("confirm-remove-master")
                                .danger()
                                .label(tr("确认删除", "Confirm removal"))
                                .on_click(move |_, window, cx| {
                                    let ok = app_confirm
                                        .update(cx, |app, cx| app.confirm_remove_master(cx));
                                    if ok {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
    }

    /// 使用官方 Dialog 承载密码库加密导入 / 导出表单。
    ///
    /// 锁定时只显示解锁输入；解锁成功后窗口重绘并切换为完整表单。
    pub(crate) fn request_vault_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        set_input(&self.vault_transfer_unlock, "", window, cx);
        let app = cx.entity();
        let unlock = self.vault_transfer_unlock.clone();
        let export_dir = self.vault_export_dir.clone();
        let import_path = self.vault_import_path.clone();
        let passphrase = self.vault_transfer_passphrase.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let locked = app.read(cx).vault.state() == VaultState::Locked;
            let replace = app.read(cx).vault_transfer_replace;
            let app_unlock = app.clone();
            let app_export = app.clone();
            let app_csv = app.clone();
            let app_import = app.clone();
            let app_browse_export = app.clone();
            let app_browse_import = app.clone();
            let app_toggle = app.clone();
            let mut dialog = dialog
                .title(tr(
                    "加密密码库导入 / 导出",
                    "Encrypted vault import / export",
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("加密备份使用独立口令和 Argon2id + XChaCha20-Poly1305；CSV 为明文，任何程序都能读取。", "Encrypted backups use a separate passphrase with Argon2id + XChaCha20-Poly1305; CSV is plain text readable by any program.")),
                );
            if locked {
                dialog = dialog.child(
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
                                .child(div().text_sm().font_semibold().child(tr("密码库已锁定", "Vault is locked"))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("锁定时不保留记录，导入、导出和 CSV 都需要先解锁。", "Records are not kept while locked; unlock before import, export or CSV.")),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(
                                    Input::new(&unlock)
                                        .id("vault-transfer-unlock")
                                        .flex_1()
                                        .min_w_0(),
                                )
                                .child(
                                    button("vault-transfer-unlock-confirm")
                                        .primary()
                                        .label(tr("解锁", "Unlock"))
                                        .on_click(move |_, window, cx| {
                                            let app = app_unlock.clone();
                                            app.update(cx, |app, cx| {
                                                if app.unlock_vault_transfer(cx) {
                                                    window.refresh();
                                                }
                                            });
                                        }),
                                ),
                        ),
                );
            } else {
                dialog = dialog
                    .child(labeled_input(
                        cx,
                        tr("导出位置", "Export location"),
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                Input::new(&export_dir)
                                    .id("vault-export-dir")
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                button("vault-export-browse")
                                    .ghost()
                                    .icon(AppIcon::FolderOpen.element(15.))
                                    .label(tr("浏览…", "Browse..."))
                                    .on_click(move |_, _, cx| {
                                        let app = app_browse_export.clone();
                                        app.update(cx, |app, cx| app.browse_vault_export_dir(cx));
                                    }),
                            ),
                    ))
                    .child(hint_text(
                        cx,
                        tr("导出文件写入所选文件夹，文件名自动附带本地时间戳，不覆盖旧备份。", "Export files go to the selected folder with a local timestamp; old backups are not overwritten."),
                    ))
                    .child(labeled_input(
                        cx,
                        tr("独立口令", "Separate passphrase"),
                        Input::new(&passphrase)
                            .id("vault-transfer-passphrase")
                            .w_full(),
                    ))
                    .child(labeled_input(
                        cx,
                        tr("导入文件", "Import file"),
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                Input::new(&import_path)
                                    .id("vault-import-path")
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                button("vault-import-browse")
                                    .ghost()
                                    .icon(AppIcon::FolderOpen.element(15.))
                                    .label(tr("浏览…", "Browse..."))
                                    .on_click(move |_, _, cx| {
                                        let app = app_browse_import.clone();
                                        app.update(cx, |app, cx| app.browse_vault_import(cx));
                                    }),
                            ),
                    ))
                    .child(
                        button("toggle-import-replace")
                            .label(if replace {
                                tr("导入策略：替换现有记录", "Import policy: replace existing records")
                            } else {
                                tr("导入策略：合并去重", "Import policy: merge and deduplicate")
                            })
                            .on_click(move |_, _, cx| {
                                let app = app_toggle.clone();
                                app.update(cx, |app, cx| {
                                    app.vault_transfer_replace = !app.vault_transfer_replace;
                                    cx.notify();
                                });
                            }),
                    );
            }
            dialog.footer(
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
                                .label(tr("导出加密备份", "Export encrypted backup"))
                                .on_click(move |_, window, cx| {
                                    let app = app_export.clone();
                                    window.defer(cx, move |window, cx| {
                                        app.update(cx, |app, cx| app.export_vault(window, cx));
                                    });
                                }),
                        )
                        .child(
                            button("vault-export-csv")
                                .warning()
                                .icon(AppIcon::Doc.element(15.))
                                .label(tr("导出明文 CSV", "Export plain CSV"))
                                .on_click(move |_, window, cx| {
                                    let app = app_csv.clone();
                                    window.defer(cx, move |window, cx| {
                                        app.update(cx, |app, cx| app.export_vault_csv(window, cx));
                                    });
                                }),
                        )
                        .child(
                            button("vault-import")
                                .label(tr("导入", "Import"))
                                .on_click(move |_, window, cx| {
                                    let app = app_import.clone();
                                    window.defer(cx, move |window, cx| {
                                        app.update(cx, |app, cx| app.import_vault(window, cx));
                                    });
                                }),
                        )
                    })
                    .child(
                        button("vault-transfer-cancel")
                            .label(tr("关闭", "Close"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
        });
    }
}
