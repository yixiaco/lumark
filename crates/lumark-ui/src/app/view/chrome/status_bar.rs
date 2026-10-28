use super::*;

impl LumarkApp {
    /// 渲染底部状态栏和紧凑任务控件。
    ///
    /// 参考图片的状态栏横跨整个窗口；这里同时保留运行状态、暂停/取消
    /// 和当前目录的总项目数与字节数。
    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.active_task.is_some();
        let progress_text = self
            .task_progress
            .as_ref()
            .map(|progress| {
                format!(
                    "{} · {:.0}% · {}",
                    progress.current,
                    progress.ratio() * 100.0,
                    format_size(progress.bytes_done)
                )
            })
            .unwrap_or_else(|| self.status.clone());
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        h_flex()
            .h(px(28.))
            .flex_shrink_0()
            .w_full()
            .px_3()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().size(px(6.)).rounded_full().bg(if running {
                cx.theme().warning
            } else {
                cx.theme().success
            }))
            .child(
                div()
                    .max_w(px(620.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(progress_text),
            )
            .when(running, |this| {
                let ratio = self
                    .task_progress
                    .as_ref()
                    .map_or(0.0, TaskProgress::ratio)
                    .clamp(0.0, 1.0);
                this.child(
                    div()
                        .w(px(110.))
                        .h(px(5.))
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .w(px(110. * ratio))
                                .h_full()
                                .rounded_full()
                                .bg(cx.theme().primary),
                        ),
                )
                .child(
                    icon_button("pause-task", AppIcon::Time, "暂停或继续").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(id) = this.active_task
                                && let Some(control) = this.tasks.control(id)
                            {
                                if control.state() == lumark_tasks::TaskState::Paused {
                                    control.resume();
                                } else {
                                    control.pause();
                                }
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    icon_button("cancel-task", AppIcon::Close, "取消任务").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(id) = this.active_task
                                && let Some(control) = this.tasks.control(id)
                            {
                                control.cancel();
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    button("show-progress")
                        .ghost()
                        .xsmall()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .icon(AppIcon::List.element(13.))
                        .label("进度")
                        .tooltip("显示任务进度窗口")
                        .on_click(cx.listener(|_, _, window, cx| {
                            cx.defer_in(window, |app, window, cx| {
                                app.open_progress_window(window, cx);
                            });
                            cx.notify();
                        })),
                )
            })
            .when(self.password_attempt.is_some(), |this| {
                // 弹窗被“后台继续”隐藏后，状态栏仍保留停止入口。
                this.child(
                    button("stop-password-attempt")
                        .ghost()
                        .xsmall()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .icon(AppIcon::Close.element(13.))
                        .label("停止自动尝试")
                        .tooltip("停止验证记录密码并改为手动输入")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_password_attempt(cx);
                        })),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .whitespace_nowrap()
                    .child(if self.screen == Screen::Browser {
                        format!(
                            "总计 {} 个项目 · {}",
                            self.displayed_count(),
                            format_size(total_size)
                        )
                    } else {
                        String::new()
                    }),
            )
            .child(
                div()
                    .pl_3()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .child("v0.1.0"),
            )
    }
}
