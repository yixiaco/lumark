//! 底部状态栏。

use super::*;
use crate::i18n::tr;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::status_bar::StatusBar;

impl LumarkApp {
    /// 渲染底部状态栏和紧凑任务控件。
    ///
    /// 使用官方 `StatusBar` 的左/中/右区域，任务控件和全局版本信息都沿用
    /// 组件的内边距、边框和主题 token，不再自绘一条 h_flex。
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
        let mut bar = StatusBar::new()
            .h(px(28.))
            .flex_shrink_0()
            .w_full()
            .px_3()
            .gap_2()
            .left(div().size(px(6.)).rounded_full().bg(if running {
                cx.theme().warning
            } else {
                cx.theme().success
            }))
            .left(
                div()
                    .max_w(px(620.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(progress_text),
            );
        if running {
            let ratio = self
                .task_progress
                .as_ref()
                .map_or(0.0, TaskProgress::ratio)
                .clamp(0.0, 1.0);
            bar = bar
                .child(
                    Progress::new("status-task-progress")
                        .value(ratio * 100.0)
                        .xsmall()
                        .w(px(110.))
                        .accessibility_label(tr("任务进度", "Task progress")),
                )
                .child(
                    icon_button(
                        "pause-task",
                        AppIcon::Time,
                        tr("暂停或继续", "Pause or resume"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
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
                    })),
                )
                .child(
                    icon_button("cancel-task", AppIcon::Close, tr("取消任务", "Cancel task"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(id) = this.active_task
                                && let Some(control) = this.tasks.control(id)
                            {
                                control.cancel();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    button("show-progress")
                        .ghost()
                        .xsmall()
                        .icon(AppIcon::List.element(13.))
                        .label(tr("进度", "Progress"))
                        .tooltip(tr("显示任务进度窗口", "Show task progress window"))
                        .on_click(cx.listener(|_, _, window, cx| {
                            cx.defer_in(window, |app, window, cx| {
                                app.open_progress_window(window, cx);
                            });
                            cx.notify();
                        })),
                );
        }
        if self.password_attempt.is_some() {
            // 弹窗被“后台继续”隐藏后，状态栏仍保留停止入口。
            bar = bar.child(
                button("stop-password-attempt")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Close.element(13.))
                    .label(tr("停止自动尝试", "Stop auto-try"))
                    .tooltip(tr(
                        "停止验证记录密码并改为手动输入",
                        "Stop verifying saved passwords and enter one manually",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_password_attempt(cx);
                    })),
            );
        }
        if self.screen == Screen::Browser {
            bar = bar.right(div().whitespace_nowrap().child(format!(
                "{}{} {} · {}",
                tr("总计 ", "Total "),
                self.displayed_count(),
                tr(" 个项目", " items"),
                format_size(total_size)
            )));
        }
        bar.right(
            div()
                .pl_3()
                .border_l_1()
                .border_color(cx.theme().border)
                .child("v0.1.0"),
        )
    }
}
