//! 归档密码、保存密码、解锁和密码尝试弹窗。

use super::*;

impl LumarkApp {
    /// 渲染归档密码输入卡片，供打开、解压、测试和条目打开流程共用。
    pub(crate) fn render_archive_password_prompt(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let target = self
            .archive_password_action
            .as_ref()
            .map(password_target_label)
            .unwrap_or_else(|| "加密压缩包".to_string());
        let auto_hint = if self.vault.state() == VaultState::Unlocked {
            "自动输入【密码管理】中密码匹配解密"
        } else {
            "解锁密码库后自动匹配记录密码"
        };
        centered_modal(cx, "输入密码")
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
                            .child("为加密的文件输入密码："),
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
                "密码",
                Input::new(&self.archive_password_input)
                    .id("archive-password")
                    .w_full(),
            ))
            .child(
                Checkbox::new("archive-password-show")
                    .label("显示密码")
                    .checked(self.archive_password_visible)
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        this.archive_password_visible = *checked;
                        this.archive_password_input.update(cx, |state, cx| {
                            state.set_masked(!*checked, window, cx);
                        });
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("archive-password-auto-vault")
                    .label(auto_hint)
                    .checked(self.archive_password_use_vault)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.archive_password_use_vault = *checked;
                        cx.notify();
                    })),
            )
            .when(self.archive_password_after_failure, |this| {
                this.child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child("记录密码均不正确。输入正确密码后可直接保存，下次自动匹配。"),
                )
            })
            .when(self.vault.state() == VaultState::Unlocked, |this| {
                this.child(
                    Checkbox::new("archive-password-save")
                        .label("保存到密码库（验证成功后写入）")
                        .checked(self.archive_password_save_to_vault)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.archive_password_save_to_vault = *checked;
                            cx.notify();
                        })),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        button("archive-password-manage")
                            .icon(AppIcon::Key.element(16.))
                            .label("密码管理")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_archive_password_prompt = false;
                                this.archive_password_action = None;
                                this.switch_screen(Screen::Passwords);
                                this.status = "请在密码管理中新增匹配密码".to_string();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        button("archive-password-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_archive_password_prompt = false;
                                this.archive_password_action = None;
                                this.status = "已取消输入压缩包密码".to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        button("archive-password-confirm")
                            .primary()
                            .label("确定")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_archive_password(window, cx);
                                cx.notify();
                            })),
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
            .child(labeled_input(
                cx,
                "名称",
                Input::new(&self.password_label).id("save-label").w_full(),
            ))
            .child(labeled_input(
                cx,
                "密码",
                Input::new(&self.password_value).id("save-value").w_full(),
            ))
            .child(labeled_input(
                cx,
                "适用压缩包（路径或通配符）",
                Input::new(&self.password_pattern)
                    .id("save-pattern")
                    .w_full(),
            ))
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

    /// 渲染记录密码校验进度，并提供停止当前候选的入口。
    ///
    /// 只有校验持续到可见时长才返回元素；快速命中时这个弹窗根本不会出现，
    /// 用户只会看到状态栏一闪而过的验证提示。
    pub(crate) fn render_password_attempt_prompt(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(attempt) = self
            .password_attempt
            .as_ref()
            .filter(|attempt| attempt.is_visible())
        else {
            return div().into_any_element();
        };
        let ratio = if attempt.total == 0 {
            0.0
        } else {
            (attempt.index as f32 / attempt.total as f32).clamp(0.0, 1.0)
        };
        centered_modal(cx, "正在尝试记录密码")
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("验证期间不会显示密码内容；可以停止后手动输入。"),
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
                    .child(attempt.archive.display().to_string()),
            )
            .child(
                Progress::new("password-attempt-progress")
                    .value(ratio)
                    .w_full(),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "第 {} / {} 条候选密码",
                                attempt.index, attempt.total
                            )),
                    )
                    .child(
                        button("password-attempt-hide")
                            .ghost()
                            .label("后台继续")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(attempt) = this.password_attempt.as_mut() {
                                    attempt.hidden = true;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        button("password-attempt-stop")
                            .secondary()
                            .label("停止自动尝试")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_password_attempt(cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}
