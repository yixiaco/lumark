//! 主程序任务进度与自解压解压进度共用的进度面板。
//!
//! 面板只负责展示进度快照、状态和指标；窗口标题、底部控制按钮以及
//! 取消/后台等动作仍由各自窗口持有，避免把主程序的任务控制逻辑带进
//! 自解压运行时。

use gpui_kit::base::{StyledExt, h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tag::Tag;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px,
};

use crate::assets::AppIcon;
use crate::format::{format_duration, format_size};
use crate::i18n::tr;
use crate::theme::StatusTone;

/// 进度面板对应的高层任务类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    /// 压缩任务。
    Compress,
    /// 解压任务。
    Extract,
    /// 测试压缩文件。
    Test,
    /// 修复压缩文件。
    Repair,
    /// 伪装文件。
    Disguise,
    /// 还原伪装文件。
    Restore,
    /// 导出游戏资产。
    Export,
    /// 其他后台任务。
    Other,
}

impl ProgressKind {
    /// 返回任务类型对应的动作名称。
    pub fn operation_label(self) -> &'static str {
        match self {
            Self::Compress => tr("压缩文件", "Compressing"),
            Self::Extract => tr("解压文件", "Extracting"),
            Self::Test => tr("测试压缩文件", "Testing archive"),
            Self::Repair => tr("修复压缩文件", "Repairing archive"),
            Self::Disguise => tr("伪装文件", "Disguising"),
            Self::Restore => tr("还原伪装文件", "Restoring disguise"),
            Self::Export => tr("导出游戏资产", "Exporting game assets"),
            Self::Other => tr("后台任务", "Background task"),
        }
    }

    /// 这类任务运行期间是否需要阻止系统睡眠。
    pub fn prevents_sleep(self) -> bool {
        matches!(
            self,
            Self::Compress
                | Self::Extract
                | Self::Test
                | Self::Repair
                | Self::Disguise
                | Self::Restore
                | Self::Export
        )
    }
}

/// 进度面板对应的高层任务状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    /// 排队中。
    Queued,
    /// 进行中。
    Running,
    /// 已暂停。
    Paused,
    /// 正在取消。
    Cancelling,
    /// 已完成。
    Completed,
    /// 失败。
    Failed,
    /// 已取消。
    Cancelled,
}

impl ProgressState {
    /// 返回任务是否已经结束。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// 返回状态名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => tr("排队中", "Queued"),
            Self::Running => tr("进行中", "Running"),
            Self::Paused => tr("已暂停", "Paused"),
            Self::Cancelling => tr("取消中", "Cancelling"),
            Self::Completed => tr("已完成", "Completed"),
            Self::Failed => tr("失败", "Failed"),
            Self::Cancelled => tr("已取消", "Cancelled"),
        }
    }
}

/// 进度面板展示所需的最小进度快照。
///
/// 字段与主程序 `TaskProgress` 保持一致，但本 crate 不依赖任务层；
/// 主程序在窗口侧完成转换，自解压运行时从自己的进度事件构造。
#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    /// 当前文件或阶段。
    pub current: String,
    /// 已完成字节数。
    pub bytes_done: u64,
    /// 总字节数。
    pub bytes_total: u64,
    /// 当前文件已处理的字节数。
    pub current_file_done: u64,
    /// 当前文件总字节数。
    pub current_file_total: u64,
    /// 界面平滑显示用的估算字节数。
    pub display_bytes: u64,
    /// 已写入输出文件的字节数。
    pub output_bytes: u64,
    /// 已完成条目数。
    pub entries_done: usize,
    /// 总条目数。
    pub entries_total: usize,
    /// 当前平均速度。
    pub speed_bps: f64,
    /// 已运行秒数。
    pub elapsed_seconds: u64,
    /// 观察到的编码块字节数；0 表示不做块内平滑。
    pub chunk_bytes: u64,
}

impl ProgressSnapshot {
    /// 返回总进度：优先按字节统计，缺少字节总量时退回条目数。
    pub fn ratio(&self) -> f32 {
        if self.bytes_total > 0 {
            (self.display_bytes.max(self.bytes_done) as f32 / self.bytes_total as f32)
                .clamp(0.0, 1.0)
        } else if self.entries_total > 0 {
            (self.entries_done as f32 / self.entries_total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// 返回当前处理文件的完成比例。
    ///
    /// 把总进度的平滑增量借给当前文件，避免当前文件条在块内长时间不动；
    /// 增量最多补到文件末尾。
    pub fn current_file_ratio(&self) -> f32 {
        if self.current_file_total == 0 {
            return 0.0;
        }
        // 只有当前文件本身跨过至少一个编码块时才借用总进度的平滑增量；
        // 小文件直接显示真实比例，避免被估算值瞬间补到 100%。
        let extra = if self.chunk_bytes > 0 && self.current_file_total >= self.chunk_bytes {
            self.display_bytes.saturating_sub(self.bytes_done)
        } else {
            0
        };
        let done = self
            .current_file_done
            .saturating_add(extra)
            .min(self.current_file_total);
        (done as f32 / self.current_file_total as f32).clamp(0.0, 1.0)
    }

    /// 返回输出大小与已处理字节的比值。
    pub fn compression_ratio(&self) -> Option<f32> {
        (self.bytes_done > 0).then(|| self.output_bytes as f32 / self.bytes_done as f32)
    }

    /// 按当前速度估算剩余秒数。
    pub fn eta_seconds(&self) -> Option<u64> {
        let done = self.display_bytes.max(self.bytes_done);
        if self.speed_bps <= 0.0 || self.bytes_total <= done {
            return None;
        }
        Some(((self.bytes_total - done) as f64 / self.speed_bps) as u64)
    }
}

/// 主程序与自解压运行时共用的进度面板。
#[derive(IntoElement)]
pub struct ProgressPanel {
    kind: ProgressKind,
    target: SharedString,
    progress: ProgressSnapshot,
    state: ProgressState,
    message: Option<SharedString>,
    elapsed_seconds: u64,
    sleep_blocked: bool,
    ongoing_hint: Option<SharedString>,
}

impl ProgressPanel {
    /// 为指定任务类型创建进度面板。
    pub fn new(kind: ProgressKind) -> Self {
        Self {
            kind,
            target: SharedString::default(),
            progress: ProgressSnapshot::default(),
            state: ProgressState::Running,
            message: None,
            elapsed_seconds: 0,
            sleep_blocked: false,
            ongoing_hint: None,
        }
    }

    /// 设置当前任务对象路径。
    pub fn target(mut self, target: impl Into<SharedString>) -> Self {
        self.target = target.into();
        self
    }

    /// 设置进度快照。
    pub fn progress(mut self, progress: ProgressSnapshot) -> Self {
        self.progress = progress;
        self
    }

    /// 设置任务状态。
    pub fn state(mut self, state: ProgressState) -> Self {
        self.state = state;
        self
    }

    /// 设置要展示的结果或错误消息。
    pub fn message(mut self, message: Option<String>) -> Self {
        self.message = message.map(SharedString::from);
        self
    }

    /// 设置已运行秒数。
    pub fn elapsed_seconds(mut self, seconds: u64) -> Self {
        self.elapsed_seconds = seconds;
        self
    }

    /// 设置是否展示“阻止系统休眠”提示。
    pub fn sleep_blocked(mut self, blocked: bool) -> Self {
        self.sleep_blocked = blocked;
        self
    }

    /// 覆盖运行中的默认提示文案。
    pub fn ongoing_hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.ongoing_hint = Some(hint.into());
        self
    }
}

impl RenderOnce for ProgressPanel {
    /// 渲染目标信息、进度条、指标和结果提示。
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let terminal = self.state.is_terminal();
        let progress = self.progress;
        let ratio = progress.ratio().clamp(0.0, 1.0);
        let file_ratio = if progress.current_file_total > 0 {
            progress.current_file_ratio()
        } else {
            ratio
        };
        let compression_ratio = progress.compression_ratio();
        let operation = self.kind.operation_label();
        let state = self.state;
        let kind = self.kind;
        let target = self.target;
        let message = self.message;
        let elapsed = self.elapsed_seconds;
        let sleep_blocked = self.sleep_blocked;
        let ongoing_hint = self.ongoing_hint;
        let hint: SharedString = if terminal {
            tr(
                "任务已结束，可以关闭此窗口。",
                "The task has finished; you can close this window.",
            )
            .into()
        } else {
            ongoing_hint.unwrap_or_else(|| {
                tr(
                    "任务在后台继续运行；关闭此窗口不会中断任务。",
                    "The task continues in the background; closing this window does not stop it.",
                )
                .into()
            })
        };

        v_flex()
            .w_full()
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
                                if kind == ProgressKind::Compress {
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
            .when(
                sleep_blocked
                    && matches!(
                        state,
                        ProgressState::Running | ProgressState::Paused | ProgressState::Cancelling
                    ),
                |this| {
                    this.child(Tag::secondary().xsmall().child(tr(
                        "任务运行期间阻止系统休眠",
                        "System sleep is blocked while the task runs",
                    )))
                },
            )
            .when_some(message, |this, message| {
                let color = match state {
                    ProgressState::Failed => StatusTone::Danger.text_color(cx),
                    ProgressState::Completed => StatusTone::Success.text_color(cx),
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
                                        tr("正在准备文件…", "Preparing files...").to_string()
                                    } else {
                                        format!(
                                            "{}{}",
                                            tr("正在处理：", "Processing: "),
                                            progress.current
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .font_semibold()
                                    .child(format!(
                                        "{} {:.1}%",
                                        tr("总进度", "Overall progress"),
                                        ratio * 100.0
                                    )),
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
                                if kind == ProgressKind::Compress {
                                    tr("当前文件", "Current file")
                                } else {
                                    tr("当前进度", "Current progress")
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
                    .child(progress_bar("file-progress", file_ratio))
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
                                    .child(tr("总进度", "Overall progress")),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .font_semibold()
                                    .child(format!("{:.1}%", ratio * 100.0)),
                            ),
                    )
                    .child(progress_bar("task-progress", ratio)),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_6()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .child(metric_row(
                                cx,
                                tr("已用时间", "Elapsed"),
                                &format_duration(elapsed),
                            ))
                            .child(metric_row(
                                cx,
                                tr("总大小", "Total size"),
                                &format_size(progress.bytes_total),
                            ))
                            .when(kind == ProgressKind::Compress, |this| {
                                this.child(metric_row(
                                    cx,
                                    tr("已压缩", "Compressed"),
                                    &format_size(progress.bytes_done),
                                ))
                                .child(metric_row(
                                    cx,
                                    tr("文件个数", "Files"),
                                    &format!(
                                        "{} / {}",
                                        progress.entries_done, progress.entries_total
                                    ),
                                ))
                            })
                            .when(kind != ProgressKind::Compress, |this| {
                                this.child(metric_row(
                                    cx,
                                    tr("已处理", "Processed"),
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
                                tr("剩余时间", "Remaining"),
                                &if terminal {
                                    "—".to_string()
                                } else {
                                    progress
                                        .eta_seconds()
                                        .map(format_duration)
                                        .unwrap_or_else(|| tr("计算中", "Calculating").to_string())
                                },
                            ))
                            .when(kind == ProgressKind::Compress, |this| {
                                this.child(metric_row(
                                    cx,
                                    tr("压缩后", "Output size"),
                                    &if progress.output_bytes > 0 {
                                        format_size(progress.output_bytes)
                                    } else {
                                        "—".to_string()
                                    },
                                ))
                                .child(metric_row(
                                    cx,
                                    tr("压缩率", "Ratio"),
                                    &compression_ratio
                                        .map(|value| format!("{:.1}%", value * 100.0))
                                        .unwrap_or_else(|| "—".to_string()),
                                ))
                            })
                            .when(kind != ProgressKind::Compress, |this| {
                                this.child(metric_row(
                                    cx,
                                    tr("文件个数", "Files"),
                                    &format!(
                                        "{} / {}",
                                        progress.entries_done, progress.entries_total
                                    ),
                                ))
                            })
                            .child(metric_row(
                                cx,
                                tr("处理速度", "Speed"),
                                &format!(
                                    "{}{}",
                                    format_size(progress.speed_bps.max(0.0) as u64),
                                    tr("/秒", "/s")
                                ),
                            )),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
    }
}

/// 渲染官方进度条，按 0–1 比例铺满可用宽度。
fn progress_bar(id: &'static str, ratio: f32) -> Progress {
    Progress::new(id)
        .value(ratio.clamp(0.0, 1.0) * 100.0)
        .w_full()
        .accessibility_label(tr("任务进度", "Task progress"))
}

/// 渲染进度面板中的一行指标。
fn metric_row(cx: &App, label: &str, value: &str) -> gpui_kit::Div {
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

#[cfg(test)]
mod tests {
    use super::ProgressSnapshot;

    /// 总进度优先按字节统计，缺少字节总量时退回条目数。
    #[test]
    fn progress_ratios_follow_snapshot() {
        let snapshot = ProgressSnapshot {
            bytes_done: 10,
            bytes_total: 100,
            current_file_done: 5,
            current_file_total: 10,
            ..Default::default()
        };
        assert!((snapshot.ratio() - 0.1).abs() < f32::EPSILON);
        assert!((snapshot.current_file_ratio() - 0.5).abs() < f32::EPSILON);

        let entries = ProgressSnapshot {
            entries_done: 3,
            entries_total: 4,
            ..Default::default()
        };
        assert!((entries.ratio() - 0.75).abs() < f32::EPSILON);
    }
}
