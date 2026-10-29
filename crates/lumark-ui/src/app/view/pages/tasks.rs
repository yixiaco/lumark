//! 任务中心页面：任务统计、任务队列和任务行。
use super::super::super::*;
use super::super::components::{AppIcon, card};
use crate::i18n::tr;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement;

impl LumarkApp {
    /// 渲染任务中心页面。
    ///
    /// 任务列表使用完整页面而不是浮层，避免浮层下方文件表继续接收鼠标事件；
    /// 每条记录保留实时进度和最终结果，点击行可重新打开独立进度窗口。
    pub(crate) fn render_task_center(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let aggregate = aggregate_task_progress(&self.task_records);
        let total = self.task_records.len();
        v_flex()
            .size_full()
            .p_5()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(task_stat_card(
                        cx,
                        AppIcon::Time,
                        tr("运行中", "Running"),
                        aggregate.running,
                        cx.theme().primary,
                    ))
                    .child(task_stat_card(
                        cx,
                        AppIcon::List,
                        tr("排队中", "Queued"),
                        aggregate.queued,
                        cx.theme().muted_foreground,
                    ))
                    .child(task_stat_card(
                        cx,
                        AppIcon::Check,
                        tr("已完成", "Completed"),
                        aggregate.finished,
                        StatusTone::Success.color(cx),
                    ))
                    .child(task_stat_card(
                        cx,
                        AppIcon::List,
                        tr("任务总数", "Total tasks"),
                        total,
                        cx.theme().muted_foreground,
                    )),
            )
            .when(aggregate.running > 0, |this| {
                // 多个任务并行时只显示各自的进度会看不到整体进展，这里按
                // 字节总量给出一条合并进度。
                this.child(
                    card(cx).w_full().p_4().child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .items_center()
                                    .child(div().text_sm().font_semibold().child(tr("跨任务总进度", "Overall progress")))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{} 个任务 · {:.0}%",
                                                aggregate.running,
                                                aggregate.ratio() * 100.0
                                            )),
                                    ),
                            )
                            .child(
                                Progress::new("task-aggregate-progress")
                                    .value(aggregate.ratio() * 100.0)
                                    .w_full(),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} / {}",
                                        format_size(aggregate.bytes_done),
                                        format_size(aggregate.bytes_total)
                                    )),
                            ),
                    ),
                )
            })
            .child(
                card(cx)
                    .flex_1()
                    .min_h_0()
                    .child(
                        h_flex()
                            .w_full()
                            .flex_shrink_0()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .bg(cx.theme().muted.opacity(0.45))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(AppIcon::List.element(16.))
                                    .child(div().font_semibold().child(tr("任务队列", "Task queue"))),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{}{total} {}", tr("共 ", "Total "), tr(" 个任务", " tasks"))),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("task-center-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .when(self.task_records.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .items_center()
                                        .justify_center()
                                        .gap_2()
                                        .p_8()
                                        .child(AppIcon::List.element(28.))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(tr("还没有任务", "No tasks yet")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(tr("压缩、解压和测试任务会显示在这里，点击任务可查看进度。", "Compression, extraction and test tasks appear here; click a task to view progress.")),
                                        ),
                                )
                            })
                            .children(
                                self.task_records
                                    .iter()
                                    .rev()
                                    .map(|record| self.render_task_row(record, cx)),
                            ),
                    ),
            )
    }

    /// 渲染任务中心中的一条任务记录。
    fn render_task_row(&self, record: &TaskRecord, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let live_state = self
            .tasks
            .control(record.id)
            .map(|control| control.state())
            .unwrap_or(record.state);
        let state = match record.state {
            TaskState::Queued => TaskState::Queued,
            state if is_terminal_task_state(state) => state,
            _ => match live_state {
                lumark_tasks::TaskState::Paused => TaskState::Paused,
                lumark_tasks::TaskState::Cancelled => TaskState::Cancelling,
                _ => TaskState::Running,
            },
        };
        let (badge_bg, badge_fg) = task_state_colors(cx, state);
        let ratio = if state == TaskState::Completed {
            1.0
        } else {
            record.progress.ratio().clamp(0.0, 1.0)
        };
        let task_id = record.id;
        let icon = match record.kind {
            TaskKind::Compress => AppIcon::Compression,
            TaskKind::Extract => AppIcon::Zip,
            TaskKind::Test => AppIcon::Shield,
            TaskKind::Edit => AppIcon::Edit,
            TaskKind::LoadArchive => AppIcon::FileZip,
            TaskKind::PasswordResolve => AppIcon::Key,
            TaskKind::Disguise => AppIcon::Video,
            TaskKind::Restore => AppIcon::Unlock,
        };
        let detail = record
            .message
            .clone()
            .or_else(|| {
                (!record.progress.current.is_empty()).then(|| {
                    format!(
                        "{}{}",
                        tr("正在处理：", "Processing: "),
                        record.progress.current
                    )
                })
            })
            .unwrap_or_else(|| {
                if state == TaskState::Queued {
                    tr("等待开始…", "Waiting to start...").to_string()
                } else {
                    tr("正在准备文件…", "Preparing files...").to_string()
                }
            });
        let metrics = format!(
            "{} / {} · {} / {} {} · {}{} · {}",
            format_size(record.progress.bytes_done),
            format_size(record.progress.bytes_total),
            record.progress.entries_done,
            record.progress.entries_total,
            tr("项", "items"),
            format_size(record.progress.speed_bps.max(0.0) as u64),
            tr("/秒", "/s"),
            format_duration(record.progress.elapsed_seconds),
        );
        h_flex()
            .id(format!("task-row-{task_id}"))
            .w_full()
            .gap_3()
            .items_start()
            .p_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .hover(|style| style.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_progress_window_for(task_id, window, cx);
                cx.notify();
            }))
            .child(
                div()
                    .size(px(34.))
                    .mt(px(2.))
                    .flex_shrink_0()
                    .rounded(px(8.))
                    .bg(badge_bg)
                    .text_color(badge_fg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.element(17.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .font_semibold()
                                    .child(record.label.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_2()
                                    .py(px(1.))
                                    .rounded(px(5.))
                                    .bg(badge_bg)
                                    .text_color(badge_fg)
                                    .text_xs()
                                    .child(task_state_label(state)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail),
                    )
                    .child(
                        Progress::new(("task-row-progress", record.id))
                            .value(ratio * 100.0)
                            .when(state == TaskState::Failed, |this| {
                                this.color(cx.theme().danger)
                            })
                            .xsmall()
                            .w_full(),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(metrics),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        button(format!("task-open-{task_id}"))
                            .ghost()
                            .xsmall()
                            .label(tr("查看", "View"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_progress_window_for(task_id, window, cx);
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .when(
                        matches!(state, TaskState::Running | TaskState::Paused),
                        |this| {
                            this.child(
                                button(format!("task-pause-{task_id}"))
                                    .ghost()
                                    .xsmall()
                                    .label(if state == TaskState::Paused {
                                        tr("继续", "Resume")
                                    } else {
                                        tr("暂停", "Pause")
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(control) = this.tasks.control(task_id) {
                                            if control.state() == lumark_tasks::TaskState::Paused {
                                                control.resume();
                                            } else {
                                                control.pause();
                                            }
                                        }
                                        cx.stop_propagation();
                                        cx.notify();
                                    })),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}

/// 渲染任务中心的统计卡片。
fn task_stat_card(
    cx: &Context<LumarkApp>,
    icon: AppIcon,
    label: &str,
    value: usize,
    color: gpui_kit::Hsla,
) -> impl IntoElement {
    card(cx).flex_1().p_4().child(
        h_flex()
            .gap_3()
            .items_center()
            .child(
                div()
                    .size(px(34.))
                    .rounded(px(8.))
                    .bg(color.opacity(0.12))
                    .text_color(color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.element(17.)),
            )
            .child(
                v_flex()
                    .gap(px(0.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    )
                    .child(div().text_lg().font_semibold().child(value.to_string())),
            ),
    )
}

/// 返回任务状态徽标的背景色和前景色。
fn task_state_colors(
    cx: &Context<LumarkApp>,
    state: TaskState,
) -> (gpui_kit::Hsla, gpui_kit::Hsla) {
    match state {
        TaskState::Queued | TaskState::Cancelled => {
            (cx.theme().secondary, cx.theme().secondary_foreground)
        }
        TaskState::Running => (cx.theme().accent, cx.theme().accent_foreground),
        TaskState::Paused => (cx.theme().warning, cx.theme().warning_foreground),
        TaskState::Cancelling | TaskState::Failed => {
            (cx.theme().danger, cx.theme().danger_foreground)
        }
        TaskState::Completed => (cx.theme().success, cx.theme().success_foreground),
    }
}
