//! 独立窗口类型和窗口级状态。

use super::super::*;
use super::components::{AppIcon, dialog_banner};
use gpui_kit::component::TitleBar;

/// 独立任务进度窗口的视图。
///
/// 进度窗口只读取任务控制句柄和最近一次进度快照，不借用主应用实体，
/// 因此可以在主应用更新期间安全地打开和渲染。
pub(crate) struct ProgressWindowView {
    kind: TaskKind,
    label: String,
    progress: TaskProgress,
    started: std::time::Instant,
    control: Option<std::sync::Arc<lumark_tasks::TaskControl>>,
    confirm_cancel: bool,
    state: TaskState,
    message: Option<String>,
}

impl ProgressWindowView {
    /// 为指定任务创建进度视图。
    pub(crate) fn new(
        kind: TaskKind,
        label: String,
        control: Option<std::sync::Arc<lumark_tasks::TaskControl>>,
        state: TaskState,
        message: Option<String>,
    ) -> Self {
        Self {
            kind,
            label,
            progress: TaskProgress::default(),
            started: std::time::Instant::now(),
            control,
            confirm_cancel: false,
            state,
            message,
        }
    }

    /// 合并主应用收到的最新进度事件。
    pub(crate) fn set_progress(&mut self, progress: TaskProgress) {
        self.progress = progress;
    }
}

impl Render for ProgressWindowView {
    /// 渲染窗口标题、目标路径、双进度指标和任务控制按钮。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let live_state = self.control.as_ref().map(|control| control.state());
        let terminal = is_terminal_task_state(self.state);
        let paused = !terminal && live_state == Some(lumark_tasks::TaskState::Paused);
        let cancelling = !terminal && live_state == Some(lumark_tasks::TaskState::Cancelled);
        let progress = &self.progress;
        let ratio = progress.ratio().clamp(0.0, 1.0);
        let file_ratio = if progress.current_file_total > 0 {
            progress.current_file_ratio()
        } else {
            ratio
        };
        let compression_ratio = progress.compression_ratio();
        let target = self
            .label
            .split_once(' ')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_else(|| self.label.clone());
        let operation = match self.kind {
            TaskKind::Compress => "压缩文件",
            TaskKind::Extract => "解压文件",
            TaskKind::Test => "测试压缩文件",
            TaskKind::Disguise => "伪装文件",
            TaskKind::Restore => "还原伪装文件",
            _ => "后台任务",
        };
        let elapsed = if paused || cancelling {
            progress.elapsed_seconds
        } else {
            progress
                .elapsed_seconds
                .max(self.started.elapsed().as_secs())
        };
        let title = if terminal {
            format!("{operation} - Lumark")
        } else {
            format!("{operation} · {:.1}% - Lumark", ratio * 100.0)
        };
        let final_state = terminal.then_some(self.state);
        let bar_width = 520.0;
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
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
                                    .child(AppIcon::Compression.element(14.)),
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
                            .when(paused || cancelling, |this| {
                                this.child(
                                    div()
                                        .px_2()
                                        .py(px(1.))
                                        .rounded(cx.theme().radius / 2.)
                                        .bg(if cancelling {
                                            cx.theme().danger
                                        } else {
                                            cx.theme().warning
                                        })
                                        .text_color(if cancelling {
                                            cx.theme().danger_foreground
                                        } else {
                                            cx.theme().warning_foreground
                                        })
                                        .text_xs()
                                        .child(if cancelling {
                                            "正在取消"
                                        } else {
                                            "已暂停"
                                        }),
                                )
                            })
                            .when_some(final_state, |this, state| {
                                let (background, foreground) = match state {
                                    TaskState::Completed => {
                                        (cx.theme().success, cx.theme().success_foreground)
                                    }
                                    TaskState::Failed => {
                                        (cx.theme().danger, cx.theme().danger_foreground)
                                    }
                                    _ => (cx.theme().secondary, cx.theme().secondary_foreground),
                                };
                                this.child(
                                    div()
                                        .px_2()
                                        .py(px(1.))
                                        .rounded(px(5.))
                                        .bg(background)
                                        .text_color(foreground)
                                        .text_xs()
                                        .child(task_state_label(state)),
                                )
                            }),
                    ),
            )
            .child(dialog_banner(cx, 54.))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px_5()
                    .py_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(cx.theme().accent_foreground)
                                    .child(
                                        if self.kind == TaskKind::Compress {
                                            AppIcon::Compression
                                        } else {
                                            AppIcon::Zip
                                        }
                                        .element(18.),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap(px(0.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{operation}:")),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .overflow_hidden()
                                            .whitespace_normal()
                                            .child(target),
                                    ),
                            ),
                    )
                    .when_some(self.message.clone(), |this, message| {
                        let color = match self.state {
                            TaskState::Failed => cx.theme().danger,
                            TaskState::Completed => cx.theme().success_foreground,
                            _ => cx.theme().muted_foreground,
                        };
                        this.child(
                            div()
                                .w_full()
                                .rounded(px(6.))
                                .bg(cx.theme().muted)
                                .px_3()
                                .py_2()
                                .text_sm()
                                .text_color(color)
                                .child(message),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .justify_between()
                                    .gap_2()
                                    .text_xs()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if progress.current.is_empty() {
                                                "正在准备文件…".to_string()
                                            } else {
                                                format!("正在处理：{}", progress.current)
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .whitespace_nowrap()
                                            .font_semibold()
                                            .child(format!("总进度 {:.1}%", ratio * 100.0)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .justify_between()
                                    .gap_2()
                                    .text_xs()
                                    .child(div().text_color(cx.theme().muted_foreground).child(
                                        if self.kind == TaskKind::Compress {
                                            "当前文件"
                                        } else {
                                            "当前进度"
                                        },
                                    ))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .whitespace_nowrap()
                                            .font_semibold()
                                            .child(format!("{:.1}%", file_ratio * 100.0)),
                                    ),
                            )
                            .child(progress_bar(cx, bar_width, file_ratio))
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .justify_between()
                                    .gap_2()
                                    .text_xs()
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("总进度"),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .whitespace_nowrap()
                                            .font_semibold()
                                            .child(format!("{:.1}%", ratio * 100.0)),
                                    ),
                            )
                            .child(progress_bar(cx, bar_width, ratio)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_6()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_2()
                                    .child(metric_row(cx, "已用时间", &format_duration(elapsed)))
                                    .child(metric_row(
                                        cx,
                                        "总大小",
                                        &format_size(progress.bytes_total),
                                    ))
                                    .when(self.kind == TaskKind::Compress, |this| {
                                        this.child(metric_row(
                                            cx,
                                            "已压缩",
                                            &format_size(progress.bytes_done),
                                        ))
                                        .child(
                                            metric_row(
                                                cx,
                                                "文件个数",
                                                &format!(
                                                    "{} / {}",
                                                    progress.entries_done, progress.entries_total
                                                ),
                                            ),
                                        )
                                    })
                                    .when(self.kind != TaskKind::Compress, |this| {
                                        this.child(metric_row(
                                            cx,
                                            "已处理",
                                            &format_size(progress.bytes_done),
                                        ))
                                    }),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_2()
                                    .child(metric_row(
                                        cx,
                                        "剩余时间",
                                        &if terminal {
                                            "—".to_string()
                                        } else {
                                            progress
                                                .eta_seconds()
                                                .map(format_duration)
                                                .unwrap_or_else(|| "计算中".to_string())
                                        },
                                    ))
                                    .when(self.kind == TaskKind::Compress, |this| {
                                        this.child(metric_row(
                                            cx,
                                            "压缩后",
                                            &if progress.output_bytes > 0 {
                                                format_size(progress.output_bytes)
                                            } else {
                                                "—".to_string()
                                            },
                                        ))
                                        .child(
                                            metric_row(
                                                cx,
                                                "压缩率",
                                                &compression_ratio
                                                    .map(|value| format!("{:.1}%", value * 100.0))
                                                    .unwrap_or_else(|| "—".to_string()),
                                            ),
                                        )
                                    })
                                    .when(self.kind != TaskKind::Compress, |this| {
                                        this.child(metric_row(
                                            cx,
                                            "文件个数",
                                            &format!(
                                                "{} / {}",
                                                progress.entries_done, progress.entries_total
                                            ),
                                        ))
                                    })
                                    .child(metric_row(
                                        cx,
                                        "处理速度",
                                        &format!(
                                            "{}/秒",
                                            format_size(progress.speed_bps.max(0.0) as u64)
                                        ),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if terminal {
                                "任务已结束，可以关闭此窗口。"
                            } else {
                                "任务在后台继续运行；关闭此窗口不会中断任务。"
                            }),
                    )
                    .when(self.confirm_cancel, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().danger)
                                .text_color(cx.theme().danger_foreground)
                                .child(AppIcon::Close.element(15.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .child("取消后当前任务会停止，已写入的临时文件会被清理。"),
                                )
                                .child(
                                    button("progress-cancel-no")
                                        .secondary()
                                        .xsmall()
                                        .cursor_pointer()
                                        .label("继续任务")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_cancel = false;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    button("progress-cancel-yes")
                                        .danger()
                                        .xsmall()
                                        .cursor_pointer()
                                        .label("确认取消")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(control) = &this.control {
                                                control.cancel();
                                            }
                                            this.confirm_cancel = false;
                                            cx.notify();
                                        })),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .when(!terminal, |this| {
                                this.child(
                                    button("progress-background")
                                        .icon(AppIcon::ArrowLeft.element(14.))
                                        .label("后台")
                                        .cursor_pointer()
                                        .on_click(cx.listener(|_, _, window, _| {
                                            window.remove_window();
                                        })),
                                )
                            })
                            .child(div().flex_1())
                            .when(terminal, |this| {
                                this.child(
                                    button("progress-close")
                                        .primary()
                                        .label("关闭")
                                        .cursor_pointer()
                                        .on_click(cx.listener(|_, _, window, _| {
                                            window.remove_window();
                                        })),
                                )
                            })
                            .when(!terminal, |this| {
                                this.child(
                                    button("progress-pause")
                                        .icon(AppIcon::Time.element(14.))
                                        .label(if paused { "继续" } else { "暂停" })
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(control) = &this.control {
                                                if control.state()
                                                    == lumark_tasks::TaskState::Paused
                                                {
                                                    control.resume();
                                                } else {
                                                    control.pause();
                                                }
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    button("progress-cancel")
                                        .danger()
                                        .icon(AppIcon::Close.element(14.))
                                        .label("取消")
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_cancel = true;
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
    }
}

/// 一个任务对应的进度窗口句柄和视图。
pub(crate) struct ProgressWindowEntry {
    /// 窗口句柄。
    pub(crate) handle: gpui_kit::AnyWindowHandle,
    /// 进度窗口实体。
    pub(crate) view: gpui_kit::Entity<ProgressWindowView>,
}

impl LumarkApp {
    /// 打开当前活动任务的独立进度窗口。
    pub(crate) fn open_progress_window(&mut self, parent: &Window, cx: &mut Context<Self>) {
        if let Some(task_id) = self.active_task {
            self.open_progress_window_for(task_id, parent, cx);
        }
    }

    /// 为仍在进行的任务自动打开进度窗口。
    ///
    /// 事件桥按批次处理任务事件，短任务可能在延迟回调执行前就已经结束；
    /// 此时终态进度窗口已经被收回，这里不能再补开一个停在 100% 的结果窗口。
    /// 任务中心仍通过 [`Self::open_progress_window_for`] 主动查看已结束任务。
    pub(crate) fn open_auto_progress_window_for(
        &mut self,
        task_id: u64,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.task_record(task_id) else {
            return;
        };
        // 同时检查任务控制句柄：完成事件可能还没被轮询取走，仅看任务记录
        // 会让窗口先弹出来再立刻关闭。
        let terminal = is_terminal_task_state(record.state)
            || self
                .tasks
                .control(task_id)
                .is_some_and(|control| is_terminal_task_state(control.state()));
        if terminal {
            return;
        }
        self.open_progress_window_for(task_id, parent, cx);
    }

    /// 打开或激活指定任务的进度窗口，包括已结束任务的结果窗口。
    ///
    /// 每个任务维护独立窗口，多个压缩/解压任务并行时不会互相覆盖；
    /// 后台任务关闭窗口后仍保留任务记录，从任务中心点击即可重新打开。
    pub(crate) fn open_progress_window_for(
        &mut self,
        task_id: u64,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.task_record(task_id).cloned() else {
            return;
        };
        if let Some(entry) = self.progress_windows.get(&task_id)
            && entry
                .handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        if let Some(entry) = self.progress_windows.remove(&task_id) {
            let _ = entry
                .handle
                .update(cx, |_, window, _| window.remove_window());
        }
        if !is_terminal_task_state(record.state) {
            self.active_task = Some(task_id);
            self.task_progress = Some(record.progress.clone());
            self.active_task_kind = Some(record.kind);
            self.active_task_label = Some(record.label.clone());
        }
        let control = self.tasks.control(task_id);
        let view = cx.new(|_| {
            let mut view = ProgressWindowView::new(
                record.kind,
                record.label.clone(),
                control,
                record.state,
                record.message.clone(),
            );
            view.set_progress(record.progress.clone());
            view
        });
        let size = gpui_kit::size(px(560.), px(470.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let cascade = (self.progress_windows.len() as f32 * 24.0).min(120.0);
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - size.width.as_f32()) / 2.0
                + cascade),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - size.height.as_f32()) / 2.0
                + cascade),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("任务进度 - Lumark".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: false,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(size),
            ..TitleBar::window_options()
        };
        match gpui_kit::open_window(options, cx, |_, _| view) {
            Ok((handle, view)) => {
                self.progress_windows
                    .insert(task_id, ProgressWindowEntry { handle, view });
            }
            Err(error) => {
                self.status = format!("无法打开进度窗口：{error}");
            }
        }
    }

    /// 关闭指定任务的进度窗口；后台任务本身不会被取消。
    pub(crate) fn dismiss_progress_window_for(&mut self, task_id: u64, cx: &mut Context<Self>) {
        if let Some(entry) = self.progress_windows.remove(&task_id) {
            let _ = entry
                .handle
                .update(cx, |_, window, _| window.remove_window());
        }
    }

    /// 每个轮询周期把平滑后的进度同步到仍在运行的进度窗口。
    ///
    /// 压缩线程按 LZMA2 块成批推进，真实事件几秒才来一次；只靠事件驱动时
    /// 两块之间窗口会静止。这里把任务记录里已经推进过的显示值写回窗口并
    /// 重绘，百分比、当前文件和剩余时间都保持连续。
    pub(crate) fn refresh_running_progress_windows(&self, cx: &mut Context<Self>) {
        for (&task_id, entry) in &self.progress_windows {
            let Some(record) = self.task_record(task_id) else {
                continue;
            };
            if is_terminal_task_state(record.state) {
                continue;
            }
            let progress = record.progress.clone();
            entry.view.update(cx, |view, cx| {
                view.set_progress(progress);
                cx.notify();
            });
        }
    }

    /// 刷新指定任务进度窗口中的最新进度。
    pub(crate) fn update_progress_window_for(
        &self,
        task_id: u64,
        progress: TaskProgress,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.progress_windows.get(&task_id) {
            entry.view.update(cx, |view, cx| {
                view.set_progress(progress);
                cx.notify();
            });
        }
    }
}

/// 渲染进度条，按固定宽度计算填充比例。
fn progress_bar(cx: &gpui_kit::App, width: f32, ratio: f32) -> gpui_kit::Div {
    div()
        .w(px(width))
        .h(px(12.))
        .rounded_full()
        .overflow_hidden()
        .bg(cx.theme().secondary_active)
        .child(
            div()
                .w(px(width * ratio.clamp(0.0, 1.0)))
                .h_full()
                .rounded_full()
                .bg(cx.theme().primary),
        )
}

/// 渲染进度窗口中的一行指标。
fn metric_row(cx: &gpui_kit::App, label: &str, value: &str) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .justify_between()
        .text_xs()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(div().font_semibold().child(value.to_string()))
}
