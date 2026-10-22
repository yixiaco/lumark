//! 密码库右侧栏：记录详情和主密码卡片。
use super::super::super::*;
use super::super::components::{AppIcon, card, form_label, icon_button};

impl LumarkApp {
    /// 渲染右侧栏，组合记录详情和主密码入口。
    pub(super) fn render_password_sidebar(
        &self,
        selected: Option<PasswordEntry>,
        has_master: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .w(px(360.))
            .flex_shrink_0()
            .gap_3()
            .child(self.render_password_details(selected, cx))
            .child(self.render_master_password_card(has_master, cx))
            .into_any_element()
    }

    /// 渲染当前选中记录的详情卡片。
    fn render_password_details(
        &self,
        selected: Option<PasswordEntry>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        card(cx)
            .child(
                h_flex()
                    .p_4()
                    .gap_2()
                    .items_center()
                    .bg(cx.theme().muted.opacity(0.45))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(AppIcon::Doc.element(16.))
                    .child(div().text_sm().font_semibold().child("记录详情"))
                    .when(selected.is_some(), |this| {
                        this.child(
                            icon_button("clear-password-selection", AppIcon::Close, "取消选择")
                                .ml_auto()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.selected_password = None;
                                    this.revealed_password = None;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(match selected.as_ref() {
                Some(entry) => {
                    let id = entry.id;
                    let revealed = self.revealed_password == Some(id);
                    let password_text = if revealed {
                        entry.password.expose().to_string()
                    } else {
                        "••••••••".to_string()
                    };
                    let pattern_label = entry
                        .archive_pattern
                        .clone()
                        .unwrap_or_else(|| "通用密码".to_string());
                    let note = if entry.note.trim().is_empty() {
                        "未填写备注".to_string()
                    } else {
                        entry.note.clone()
                    };
                    let entry_for_edit = entry.clone();
                    v_flex()
                        .p_4()
                        .gap_3()
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .child(
                                    div()
                                        .size(px(40.))
                                        .flex_shrink_0()
                                        .rounded(px(10.))
                                        .bg(cx.theme().primary.opacity(0.12))
                                        .text_color(cx.theme().primary)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(AppIcon::Key.element(18.)),
                                )
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .whitespace_normal()
                                                .child(entry.display_name().to_string()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(pattern_label),
                                        ),
                                ),
                        )
                        .child(div().h(px(1.)).bg(cx.theme().border))
                        .child(
                            v_flex().gap_1().child(form_label(cx, "密码")).child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .font_semibold()
                                            .child(password_text),
                                    )
                                    .child(
                                        icon_button(
                                            "detail-password-reveal",
                                            if revealed {
                                                AppIcon::Lock
                                            } else {
                                                AppIcon::Unlock
                                            },
                                            if revealed {
                                                "隐藏密码"
                                            } else {
                                                "显示 10 秒"
                                            },
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.toggle_password_reveal(id, cx);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        icon_button(
                                            "detail-password-copy",
                                            AppIcon::Copy,
                                            "复制密码",
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.copy_password(cx);
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            ),
                        )
                        .child(
                            v_flex().gap_1().child(form_label(cx, "适用压缩包")).child(
                                div().text_sm().whitespace_normal().child(
                                    entry
                                        .archive_pattern
                                        .clone()
                                        .unwrap_or_else(|| "通用密码".to_string()),
                                ),
                            ),
                        )
                        .child(
                            v_flex().gap_1().child(form_label(cx, "备注")).child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_normal()
                                    .child(note),
                            ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .pt_1()
                                .child(
                                    button("edit-password")
                                        .primary()
                                        .icon(AppIcon::Edit.element(16.))
                                        .label("编辑")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_password_editor_window(
                                                Some(entry_for_edit.clone()),
                                                window,
                                                cx,
                                            );
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    button("delete-password")
                                        .danger()
                                        .icon(AppIcon::Delete.element(16.))
                                        .label("删除")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_confirm_delete_password = true;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .into_any_element()
                }
                None => v_flex()
                    .p_6()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(46.))
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Key.element(22.)),
                    )
                    .child(div().text_sm().font_semibold().child("选择一条记录"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .text_center()
                            .child("从左侧选择记录查看详情，或新建一条密码记录。"),
                    )
                    .child(
                        button("detail-new-password")
                            .primary()
                            .icon(AppIcon::Add.element(16.))
                            .label("新建记录")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_password_editor_window(None, window, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            })
            .into_any_element()
    }

    /// 渲染主密码状态和设置入口。
    fn render_master_password_card(
        &self,
        has_master: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        card(cx)
            .child(
                h_flex()
                    .p_4()
                    .gap_2()
                    .items_center()
                    .bg(cx.theme().muted.opacity(0.45))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(AppIcon::Shield.element(16.))
                    .child(div().text_sm().font_semibold().child("主密码")),
            )
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py(px(1.))
                                    .rounded(cx.theme().radius / 2.)
                                    .bg(if has_master {
                                        cx.theme().success
                                    } else {
                                        cx.theme().warning
                                    })
                                    .text_color(if has_master {
                                        cx.theme().success_foreground
                                    } else {
                                        cx.theme().warning_foreground
                                    })
                                    .text_xs()
                                    .child(if has_master { "已设置" } else { "DPAPI" }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if has_master {
                                        "由 Argon2id 派生密钥保护"
                                    } else {
                                        "与当前计算机绑定"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .whitespace_normal()
                            .child("设置主密码后会移除 DPAPI 包裹；忘记主密码将无法恢复。"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                button("open-master-window")
                                    .primary()
                                    .icon(AppIcon::Shield.element(16.))
                                    .label(if has_master {
                                        "修改主密码"
                                    } else {
                                        "设置主密码"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_master_password_window(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .when(has_master, |this| {
                                this.child(
                                    button("remove-master")
                                        .icon(AppIcon::Delete.element(16.))
                                        .label("删除主密码")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_confirm_remove_master = true;
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}
