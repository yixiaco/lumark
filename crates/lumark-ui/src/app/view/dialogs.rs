//! 主窗口弹窗入口和通用弹窗辅助。

use super::super::*;
use super::components::{AppIcon, card, form_label, hint_text, option_radio};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;

mod archive_edit;
mod archive_password;
mod fs_edit;
mod temp_extract;
mod vault;

/// 构建“标签 + 输入框”的纵向字段，避免表单只露出无说明的输入框。
fn labeled_input(cx: &gpui_kit::App, label: &str, input: impl IntoElement) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(form_label(cx, label))
        .child(input)
}

/// 返回密码弹窗需要展示的归档或条目名称。
fn password_target_label(action: &PendingPasswordAction) -> String {
    match action {
        PendingPasswordAction::OpenArchive { archive, .. }
        | PendingPasswordAction::ExtractDialog { archive }
        | PendingPasswordAction::Test { archive } => archive.display().to_string(),
        PendingPasswordAction::OpenEntry {
            archive, relative, ..
        } => {
            format!("{} · {relative}", archive.display())
        }
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
