//! 记录密码校验进度对话框；使用官方 Dialog，并在任务更新时刷新窗口。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 打开官方 Dialog 展示记录密码校验进度和停止入口。
    pub(crate) fn open_password_attempt_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(attempt) = self
            .password_attempt
            .as_ref()
            .filter(|attempt| attempt.is_visible())
        else {
            return;
        };
        let archive = attempt.archive.display().to_string();
        let app = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(attempt) = app
                .read(cx)
                .password_attempt
                .as_ref()
                .filter(|attempt| attempt.is_visible())
            else {
                return dialog;
            };
            let ratio = if attempt.total == 0 {
                0.0
            } else {
                (attempt.index as f32 / attempt.total as f32).clamp(0.0, 1.0)
            };
            let app_stop = app.clone();
            dialog
                .title(tr("正在尝试记录密码", "Trying saved passwords"))
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("验证期间不会显示密码内容；可以停止后手动输入。", "Password contents are hidden while verifying; stop and enter one manually if needed.")),
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
                        .child(archive.clone()),
                )
                .child(
                    Progress::new("password-attempt-progress")
                        .value(ratio * 100.0)
                        .w_full(),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .child(
                            button("password-attempt-stop")
                                .secondary()
                                .label(tr("停止自动尝试", "Stop auto-try"))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    app_stop.update(cx, |app, cx| {
                                        app.cancel_password_attempt(cx);
                                    });
                                }),
                        ),
                )
        });
    }
}
