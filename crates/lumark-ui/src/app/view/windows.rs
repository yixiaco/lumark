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
}

impl ProgressWindowView {
    /// 为指定任务创建进度视图。
    pub(crate) fn new(
        kind: TaskKind,
        label: String,
        control: Option<std::sync::Arc<lumark_tasks::TaskControl>>,
    ) -> Self {
        Self {
            kind,
            label,
            progress: TaskProgress::default(),
            started: std::time::Instant::now(),
            control,
            confirm_cancel: false,
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
        let state = self.control.as_ref().map(|control| control.state());
        let paused = state == Some(lumark_tasks::TaskState::Paused);
        let cancelling = state == Some(lumark_tasks::TaskState::Cancelled);
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
            _ => "后台任务",
        };
        let elapsed = if paused || cancelling {
            progress.elapsed_seconds
        } else {
            progress
                .elapsed_seconds
                .max(self.started.elapsed().as_secs())
        };
        let title = format!("{operation} · {:.1}% - Lumark", ratio * 100.0);
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
                                        .rounded_full()
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
                                        &progress
                                            .eta_seconds()
                                            .map(format_duration)
                                            .unwrap_or_else(|| "计算中".to_string()),
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
                            .child("任务在后台继续运行；关闭此窗口不会中断任务。"),
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
                                    Button::new("progress-cancel-no")
                                        .secondary()
                                        .xsmall()
                                        .label("继续任务")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_cancel = false;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("progress-cancel-yes")
                                        .danger()
                                        .xsmall()
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
                            .child(
                                Button::new("progress-background")
                                    .icon(AppIcon::ArrowLeft.element(14.))
                                    .label("后台")
                                    .on_click(cx.listener(|_, _, window, _| {
                                        window.remove_window();
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("progress-pause")
                                    .icon(AppIcon::Time.element(14.))
                                    .label(if paused { "继续" } else { "暂停" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(control) = &this.control {
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
                                Button::new("progress-cancel")
                                    .danger()
                                    .icon(AppIcon::Close.element(14.))
                                    .label("取消")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_cancel = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }
}

impl LumarkApp {
    /// 打开或激活独立进度窗口。
    ///
    /// 每次打开都会用当前任务状态创建新的进度视图；用户选择“后台”关闭窗口后，
    /// 再从状态栏的“进度”入口打开即可恢复同一任务的实时进度。
    pub(crate) fn open_progress_window(&mut self, parent: &Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        self.progress_window = None;
        let Some(task_id) = self.active_task else {
            return;
        };
        let Some(kind) = self.active_task_kind else {
            return;
        };
        let Some(label) = self.active_task_label.clone() else {
            return;
        };
        let control = self.tasks.control(task_id);
        let progress = self.task_progress.clone().unwrap_or_default();
        let view = cx.new(|_| {
            let mut view = ProgressWindowView::new(kind, label, control);
            view.set_progress(progress);
            view
        });
        self.progress_view = Some(view.clone());
        let size = gpui_kit::size(px(560.), px(470.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - size.height.as_f32()) / 2.0),
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
            Ok((handle, _view)) => {
                self.progress_window = Some(handle);
            }
            Err(error) => {
                self.status = format!("无法打开进度窗口：{error}");
            }
        }
    }

    /// 关闭进度窗口并释放进度视图；后台任务不会被取消。
    pub(crate) fn dismiss_progress_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.progress_view = None;
    }

    /// 刷新进度视图中的最新进度。
    pub(crate) fn update_progress_window(&self, progress: TaskProgress, cx: &mut Context<Self>) {
        if let Some(view) = &self.progress_view {
            view.update(cx, |view, cx| {
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
