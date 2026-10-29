//! 归档密码、保存密码、解锁和密码尝试弹窗。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 使用官方 AlertDialog 确认是否继续尝试剩余记录密码。
    pub(crate) fn request_extra_password_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self
            .pending_extra_password
            .as_ref()
            .map_or(0, |job| job.candidates.len());
        let app = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app_try = app.clone();
            let app_decline = app.clone();
            alert
                .title(tr(
                    "尝试其余记录密码？",
                    "Try remaining saved passwords?",
                ))
                .description(format!(
                    "{}{count}{}",
                    tr(
                        "前 32 条记录密码均不正确，密码库中还有 ",
                        "The first 32 saved passwords did not match; "
                    ),
                    tr(
                        " 条候选。继续尝试可能需要较长时间；也可以停止自动尝试并手动输入。",
                        " candidates remain. Trying all may take a while; you can stop and enter one manually."
                    )
                ))
                .confirm()
                .ok_text(format!(
                    "{}{count}{}",
                    tr("尝试其余 ", "Try remaining "),
                    tr(" 条", " entries")
                ))
                .cancel_text(tr("手动输入", "Enter manually"))
                .on_ok(move |_, window, cx| {
                    let app = app_try.clone();
                    window.defer(cx, move |window, cx| {
                        app.update(cx, |app, cx| {
                            app.try_extra_password_candidates(window, cx);
                        });
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    let app = app_decline.clone();
                    window.defer(cx, move |window, cx| {
                        app.update(cx, |app, cx| {
                            app.decline_extra_password_candidates(window, cx);
                        });
                    });
                    true
                })
        });
    }

    /// 打开官方 Dialog 保存刚验证成功的解压密码。
    pub(crate) fn request_save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity();
        let label = self.password_label.clone();
        let value = self.password_value.clone();
        let pattern = self.password_pattern.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let app_confirm = app.clone();
            let label = label.clone();
            let value = value.clone();
            let pattern = pattern.clone();
            dialog
                .title(tr("保存解压密码", "Save extraction password"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr(
                            "密码将写入本地加密密码库，下次可直接用于同类压缩包。",
                            "The password is saved to the local encrypted vault for similar archives.",
                        )),
                )
                .child(labeled_input(
                    cx,
                    tr("名称", "Name"),
                    Input::new(&label).id("save-label").w_full(),
                ))
                .child(labeled_input(
                    cx,
                    tr("密码", "Password"),
                    Input::new(&value).id("save-value").w_full(),
                ))
                .child(labeled_input(
                    cx,
                    tr(
                        "适用压缩包（路径或通配符）",
                        "Archive pattern (path or wildcard)",
                    ),
                    Input::new(&pattern).id("save-pattern").w_full(),
                ))
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("save-dismiss")
                                .secondary()
                                .label(tr("不保存", "Do not save"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            button("save-confirm")
                                .primary()
                                .icon(AppIcon::Save.element(16.))
                                .label(tr("保存密码", "Save password"))
                                .on_click(move |_, window, cx| {
                                    let ok = app_confirm
                                        .update(cx, |app, cx| app.save_password(cx));
                                    if ok {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
    }

    /// 使用官方 Dialog 请求密码库主密码，解锁后继续自动尝试记录密码。
    pub(crate) fn request_unlock_for_extract(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        set_input(&self.master_extract_unlock, "", window, cx);
        let app = cx.entity();
        let input = self.master_extract_unlock.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let app_confirm = app.clone();
            let app_cancel = app.clone();
            let input = input.clone();
            dialog
                .title(tr("解锁密码库", "Unlock vault"))
                .child(tr(
                    "解锁后只会尝试密码库中的记录密码，不会显示密码内容。",
                    "After unlocking, only saved passwords are tried; their contents are never shown.",
                ))
                .child(
                    Input::new(&input)
                        .id("unlock-for-extract")
                        .w_full(),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("unlock-cancel")
                                .label(tr("取消", "Cancel"))
                                .on_click(move |_, window, cx| {
                                    app_cancel.update(cx, |app, cx| {
                                        app.pending_unlock_archive = None;
                                        app.unlock_for_browse = false;
                                        cx.notify();
                                    });
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            button("unlock-confirm")
                                .primary()
                                .label(tr("解锁并自动尝试", "Unlock and auto-try"))
                                .on_click(move |_, window, cx| {
                                    let app = app_confirm.clone();
                                    window.defer(cx, move |window, cx| {
                                        window.close_dialog(cx);
                                        app.update(cx, |app, cx| {
                                            app.unlock_for_extract(window, cx);
                                        });
                                    });
                                }),
                        ),
                )
        });
    }

    /// 使用官方 Dialog 请求归档密码，支持手动输入、密码库自动匹配和保存引导。
    pub(crate) fn request_archive_password_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity();
        let input = self.archive_password_input.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let state = app.read(cx);
            let target = state
                .archive_password_action
                .as_ref()
                .map(password_target_label)
                .unwrap_or_else(|| tr("加密压缩包", "Encrypted archive").to_string());
            let visible = state.archive_password_visible;
            let use_vault = state.archive_password_use_vault;
            let after_failure = state.archive_password_after_failure;
            let save_to_vault = state.archive_password_save_to_vault;
            let vault_unlocked = state.vault.state() == VaultState::Unlocked;
            let auto_hint = if vault_unlocked {
                tr(
                    "自动输入【密码管理】中密码匹配解密",
                    "Auto-fill a matching password from Password manager",
                )
            } else {
                tr(
                    "解锁密码库后自动匹配记录密码",
                    "Unlock the vault to auto-match saved passwords",
                )
            };
            let app_show = app.clone();
            let app_use_vault = app.clone();
            let app_save = app.clone();
            let app_manage = app.clone();
            let app_cancel = app.clone();
            let app_confirm = app.clone();
            let input_show = input.clone();
            let mut dialog = dialog
                .title(tr("输入密码", "Enter password"))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .size(px(30.))
                                .flex_shrink_0()
                                .rounded(px(8.))
                                .bg(cx.theme().primary.opacity(0.12))
                                .text_color(cx.theme().primary)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(AppIcon::Lock.element(16.)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("为加密的文件输入密码：", "Enter the password for the encrypted file:")),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(34.))
                        .px_3()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().input)
                        .text_sm()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(target),
                )
                .child(labeled_input(
                    cx,
                    tr("密码", "Password"),
                    Input::new(&input).id("archive-password").w_full(),
                ))
                .child(
                    Checkbox::new("archive-password-show")
                        .label(tr("显示密码", "Show password"))
                        .checked(visible)
                        .on_click(move |checked: &bool, window, cx| {
                            let checked = *checked;
                            app_show.update(cx, |app, cx| {
                                app.archive_password_visible = checked;
                                input_show.update(cx, |state, cx| {
                                    state.set_masked(!checked, window, cx);
                                });
                                cx.notify();
                            });
                            window.refresh();
                        }),
                )
                .child(
                    Checkbox::new("archive-password-auto-vault")
                        .label(auto_hint)
                        .checked(use_vault)
                        .on_click(move |checked: &bool, window, cx| {
                            let checked = *checked;
                            app_use_vault.update(cx, |app, cx| {
                                app.archive_password_use_vault = checked;
                                cx.notify();
                            });
                            window.refresh();
                        }),
                );
            if after_failure {
                dialog = dialog.child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(StatusTone::Warning.text_color(cx))
                        .child(tr("记录密码均不正确。输入正确密码后可直接保存，下次自动匹配。", "Saved passwords did not match. Enter the correct one and save it for next time.")),
                );
            }
            if vault_unlocked {
                dialog = dialog.child(
                    Checkbox::new("archive-password-save")
                        .label(tr("保存到密码库（验证成功后写入）", "Save to vault (after successful verification)"))
                        .checked(save_to_vault)
                        .on_click(move |checked: &bool, window, cx| {
                            let checked = *checked;
                            app_save.update(cx, |app, cx| {
                                app.archive_password_save_to_vault = checked;
                                cx.notify();
                            });
                            window.refresh();
                        }),
                );
            }
            dialog.footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        button("archive-password-manage")
                            .icon(AppIcon::Key.element(16.))
                            .label(tr("密码管理", "Password manager"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                app_manage.update(cx, |app, cx| {
                                    app.archive_password_action = None;
                                    app.switch_screen(Screen::Passwords);
                                    app.status = tr("请在密码管理中新增匹配密码", "Add a matching password in Password manager").to_string();
                                    cx.notify();
                                });
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        button("archive-password-cancel")
                            .secondary()
                            .label(tr("取消", "Cancel"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                app_cancel.update(cx, |app, cx| {
                                    app.archive_password_action = None;
                                    app.status = tr("已取消输入压缩包密码", "Archive password input cancelled").to_string();
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        button("archive-password-confirm")
                            .primary()
                            .label(tr("确定", "OK"))
                            .on_click(move |_, window, cx| {
                                let close = app_confirm.update(cx, |app, cx| {
                                    app.confirm_archive_password(window, cx)
                                });
                                if close {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
        });
    }
}
