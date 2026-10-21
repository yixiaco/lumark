//! 密码库记录列表：搜索、新建入口、锁定面板和记录行。
use super::super::super::*;
use super::super::components::{AppIcon, card};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::scroll::ScrollableElement;

impl LumarkApp {
    /// 渲染密码记录列表卡片；记录较多时只滚动列表本身。
    pub(super) fn render_password_list(
        &self,
        entries: Vec<PasswordEntry>,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        card(cx)
            .flex_1()
            .min_w(px(420.))
            .min_h_0()
            .child(
                h_flex()
                    .p_4()
                    .gap_2()
                    .items_center()
                    .bg(cx.theme().muted.opacity(0.45))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().text_sm().font_semibold().child("记录列表"))
                    .child(
                        div()
                            .px_2()
                            .py(px(1.))
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 条", entries.len())),
                    )
                    .child(
                        h_flex()
                            .ml_auto()
                            .w(px(250.))
                            .h(px(34.))
                            .px_2()
                            .gap_2()
                            .rounded_md()
                            .bg(cx.theme().secondary)
                            .border_1()
                            .border_color(cx.theme().input)
                            .child(AppIcon::Search.element(14.))
                            .child(
                                Input::new(&self.password_search)
                                    .id("password-search")
                                    .flex_1()
                                    .min_w_0(),
                            ),
                    )
                    .when(!locked, |this| {
                        this.child(
                            button("password-add-new")
                                .primary()
                                .cursor_pointer()
                                .icon(AppIcon::Add.element(16.))
                                .label("新建")
                                .tooltip("新建密码记录")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_password_editor_window(None, window, cx);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(locked, |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_8()
                        .child(
                            div()
                                .size(px(58.))
                                .rounded_full()
                                .bg(cx.theme().warning)
                                .text_color(cx.theme().warning_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(AppIcon::Lock.element(26.)),
                        )
                        .child(div().font_semibold().child("密码库已锁定"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("输入主密码后才能查看、编辑或自动尝试记录密码。"),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Input::new(&self.master_unlock)
                                        .id("unlock-master")
                                        .w(px(260.)),
                                )
                                .child(
                                    button("unlock")
                                        .primary()
                                        .icon(AppIcon::Unlock.element(15.))
                                        .label("解锁")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.unlock_vault(window, cx);
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
            .when(!locked, |this| {
                this.child(
                    v_flex()
                        .id("password-list")
                        .test_support()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.password_list_scroll)
                        .vertical_scrollbar(&self.password_list_scroll)
                        .on_scroll_wheel(cx.listener(
                            |this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                                let delta = event.delta.pixel_delta(px(20.));
                                let offset = this.password_list_scroll.offset();
                                let max = this.password_list_scroll.max_offset().y;
                                let next_y = (offset.y + delta.y).clamp(-max, px(0.));
                                if next_y == offset.y {
                                    return;
                                }
                                this.password_list_scroll
                                    .set_offset(gpui_kit::point(offset.x, next_y));
                                cx.stop_propagation();
                                cx.notify();
                            },
                        ))
                        .when(entries.is_empty(), |this| {
                            this.child(
                                v_flex()
                                    .p_8()
                                    .items_center()
                                    .gap_2()
                                    .child(AppIcon::Key.element(28.))
                                    .child(
                                        div().text_sm().font_semibold().child("还没有保存的密码"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                "点击右上角加号新建记录，或解压成功后选择保存。",
                                            ),
                                    ),
                            )
                        })
                        .children(entries.into_iter().map(|entry| {
                            let id = entry.id;
                            let reveal_id = entry.id;
                            let pattern_label = entry
                                .archive_pattern
                                .clone()
                                .unwrap_or_else(|| "通用密码".to_string());
                            let updated_label = format_record_time(entry.updated_at);
                            let selected = self.selected_password == Some(id);
                            let revealed = self.revealed_password == Some(id);
                            let password_text = if revealed {
                                entry.password.expose().to_string()
                            } else {
                                "••••••••".to_string()
                            };
                            h_flex()
                                .id(format!("password-row-{id}"))
                                .w_full()
                                .min_h(px(58.))
                                .gap_3()
                                .px_4()
                                .py_2()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .when(selected, |this| this.bg(cx.theme().list_active))
                                .when(!selected, |this| {
                                    this.hover(|style| style.bg(cx.theme().list_hover))
                                })
                                .child(
                                    div()
                                        .size(px(32.))
                                        .rounded_md()
                                        .bg(cx.theme().accent)
                                        .text_color(cx.theme().accent_foreground)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(AppIcon::Key.element(16.)),
                                )
                                .child(
                                    v_flex()
                                        .id(format!("password-select-{id}"))
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if this.selected_password == Some(id) {
                                                this.selected_password = None;
                                            } else {
                                                this.selected_password = Some(id);
                                            }
                                            this.revealed_password = None;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .w_full()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_sm()
                                                .font_semibold()
                                                .child(entry.display_name().to_string()),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(pattern_label)
                                                .child(div().child("·"))
                                                .child(AppIcon::Time.element(11.))
                                                .child(updated_label),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(104.))
                                        .text_sm()
                                        .font_semibold()
                                        .child(password_text),
                                )
                                .child(
                                    button(format!("password-reveal-{id}"))
                                        .ghost()
                                        .small()
                                        .cursor_pointer()
                                        .icon(
                                            if revealed {
                                                AppIcon::Lock
                                            } else {
                                                AppIcon::Unlock
                                            }
                                            .element(14.),
                                        )
                                        .tooltip(if revealed {
                                            "隐藏密码"
                                        } else {
                                            "显示 10 秒"
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.toggle_password_reveal(reveal_id, cx);
                                            cx.notify();
                                        })),
                                )
                        })),
                )
            })
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("新建、编辑和删除都会立即写入本地加密密码库。"),
                    ),
            )
            .into_any_element()
    }
}
