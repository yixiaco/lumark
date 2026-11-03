//! 自解压窗口的 GPUI 渲染实现。

use gpui_kit::base::{StyledExt, h_flex, v_flex};
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, TitleBar};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement, Styled, Window, div, px,
};

use lumark_sfx::SfxFormat;
use lumark_ui_shared::assets::AppIcon;
use lumark_ui_shared::chrome::dialog_banner;
use lumark_ui_shared::i18n::tr;
use lumark_ui_shared::progress::{ProgressKind, ProgressPanel, ProgressState};
use lumark_ui_shared::theme::StatusTone;
use lumark_ui_shared::widgets::{button, classic_field, hint_text};

use super::view::{SfxView, Stage};

/// 渲染整个自解压窗口。
pub(super) fn render(
    view: &SfxView,
    _window: &mut Window,
    cx: &mut Context<SfxView>,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .bg(cx.theme().popover)
        .text_color(cx.theme().foreground)
        .child(title_bar(view, cx))
        .child(dialog_banner(cx, 54., tagline()))
        .child(match view.stage {
            Stage::Setup => setup_body(view, cx).into_any_element(),
            Stage::Extracting | Stage::Completed => progress_body(view, cx).into_any_element(),
        })
        .child(footer(view, cx))
}

/// 自解压窗口横幅使用的说明文案。
fn tagline() -> &'static str {
    tr(
        "自解压包 · 本地解压",
        "Self-extracting archive · local extraction",
    )
}

/// 渲染品牌标题栏和当前窗口标题。
fn title_bar(view: &SfxView, cx: &Context<SfxView>) -> impl IntoElement {
    let title = match view.stage {
        Stage::Setup => tr("Lumark 自解压", "Lumark Self-Extractor").to_string(),
        Stage::Extracting => format!(
            "{} · {:.1}% - Lumark",
            tr("解压文件", "Extracting"),
            view.progress.ratio() * 100.0
        ),
        Stage::Completed => {
            format!("{} - Lumark", tr("解压完成", "Extraction complete"))
        }
    };
    TitleBar::new()
        .h(px(38.))
        .bg(cx.theme().title_bar)
        .border_b_1()
        .border_color(cx.theme().title_bar_border)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(24.))
                        .rounded(px(7.))
                        .bg(cx.theme().button_primary)
                        .text_color(cx.theme().button_primary_foreground)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(AppIcon::Zip.element(14.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_sm()
                        .font_semibold()
                        .child(title),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(180.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(view.exe_name()),
                ),
        )
}

/// 渲染归档信息和格式/加密标签。
fn info_row(view: &SfxView, cx: &Context<SfxView>) -> impl IntoElement {
    let format = match view.footer.format {
        SfxFormat::Zip => "ZIP",
        SfxFormat::SevenZip => "7Z",
    };
    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .rounded(px(5.))
        .bg(cx.theme().muted)
        .px_2()
        .py_1()
        .child(
            div()
                .flex_shrink_0()
                .text_color(cx.theme().accent_foreground)
                .child(AppIcon::FileZip.element(15.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_sm()
                .child(view.exe_name()),
        )
        .child(Tag::secondary().xsmall().child(format))
        .when(view.footer.encrypted, |this| {
            this.child(Tag::secondary().xsmall().child(tr("加密", "Encrypted")))
        })
}

/// 渲染设置阶段的目录、密码和校验提示。
fn setup_body(view: &SfxView, cx: &mut Context<SfxView>) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .px_5()
        .py_4()
        .gap_3()
        .child(info_row(view, cx))
        .child(classic_field(
            tr("解压到:", "Extract to:"),
            60.,
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    Input::new(&view.target)
                        .id("sfx-target")
                        .flex_1()
                        .min_w_0(),
                )
                .child(
                    button("sfx-browse")
                        .secondary()
                        .label(tr("浏览…", "Browse..."))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_directory(window, cx);
                        })),
                ),
        ))
        .when(view.footer.encrypted, |this| {
            this.child(classic_field(
                tr("密码:", "Password:"),
                60.,
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Input::new(&view.password)
                            .id("sfx-password")
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        button("sfx-password-toggle")
                            .secondary()
                            .label(if view.password_visible {
                                tr("隐藏密码", "Hide password")
                            } else {
                                tr("显示密码", "Show password")
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_password(window, cx);
                            })),
                    ),
            ))
        })
        .child(match &view.error {
            Some(error) => div()
                .id("sfx-error")
                .text_xs()
                .text_color(StatusTone::Danger.text_color(cx))
                .child(error.clone())
                .into_any_element(),
            None => hint_text(
                cx,
                tr(
                    "解压到默认文件夹，可修改路径或浏览选择；目标非空时会先确认覆盖。",
                    "Extract to the default folder, edit the path, or browse; you will be asked before overwriting.",
                ),
            )
            .into_any_element(),
        })
        .child(div().flex_1())
}

/// 渲染解压中/完成后的进度面板。
fn progress_body(view: &SfxView, cx: &Context<SfxView>) -> impl IntoElement {
    let elapsed = match view.stage {
        Stage::Extracting => view.progress.elapsed_seconds.max(
            view.started
                .map(|value| value.elapsed().as_secs())
                .unwrap_or(0),
        ),
        _ => view.progress.elapsed_seconds,
    };
    v_flex()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .px_5()
        .py_4()
        .gap_3()
        .child(
            ProgressPanel::new(ProgressKind::Extract)
                .target(view.progress_target(cx))
                .progress(view.progress.clone())
                .state(view.progress_state)
                .message(view.message.clone())
                .elapsed_seconds(elapsed)
                .sleep_blocked(false)
                .ongoing_hint(tr(
                    "解压过程中关闭窗口会停止任务。",
                    "Closing the window stops extraction.",
                )),
        )
        .child(div().flex_1())
}

/// 渲染底部操作按钮；页脚始终保留可见高度，避免内容裁掉按钮。
fn footer(view: &SfxView, cx: &mut Context<SfxView>) -> impl IntoElement {
    h_flex()
        .w_full()
        .flex_shrink_0()
        .gap_2()
        .items_center()
        .justify_end()
        .p_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .when(view.stage == Stage::Setup, |this| {
            this.child(
                button("sfx-cancel")
                    .secondary()
                    .label(tr("取消", "Cancel"))
                    .on_click(cx.listener(|_, _, window, _| window.remove_window())),
            )
            .child(
                button("sfx-extract")
                    .primary()
                    .icon(AppIcon::Zip.element(14.))
                    .label(tr("解压", "Extract"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_extraction(window, cx);
                    })),
            )
        })
        .when(view.stage == Stage::Extracting, |this| {
            let cancelling = view.progress_state == ProgressState::Cancelling;
            this.child(
                button("sfx-cancel-run")
                    .danger()
                    .disabled(cancelling)
                    .icon(AppIcon::Close.element(14.))
                    .label(if cancelling {
                        tr("正在取消…", "Cancelling...")
                    } else {
                        tr("取消解压", "Cancel extraction")
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request_cancel(window, cx);
                    })),
            )
        })
        .when(view.stage == Stage::Completed, |this| {
            this.child(
                button("sfx-open-folder")
                    .secondary()
                    .icon(AppIcon::FolderOpen.element(14.))
                    .label(tr("打开文件夹", "Open folder"))
                    .on_click(cx.listener(|this, _, _, _| this.open_folder())),
            )
            .child(
                button("sfx-close")
                    .primary()
                    .label(tr("完成", "Done"))
                    .on_click(cx.listener(|_, _, window, _| window.remove_window())),
            )
        })
}
