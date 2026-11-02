//! 批量导出窗口（E-15）：格式/材质/数量选择、后台批量、取消、报告与目录打开。

use super::super::*;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::AnyElement;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable, TitleBar};
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div, px};
use lumark_tasks::{TaskControl, TaskKind, TaskManager, TaskProgress, TaskReporter, TaskRequest};
use lumark_unreal::{
    BatchExportConfig, BatchExportFormat, BatchExportReport, BatchProgress, GameAssetOptions,
    GameAssetSource, Mappings, export_archive_cancellable,
};

use crate::app::{BrowserLocation, LumarkApp};
use crate::i18n::tr;

/// 批量导出目标。
#[derive(Debug, Clone)]
pub(crate) struct BatchExportTarget {
    /// 容器路径。
    pub archive: PathBuf,
    /// global.utoc。
    pub global: PathBuf,
    /// 映射文件。
    pub mappings: Option<PathBuf>,
    /// Oodle 库。
    pub oodle: Option<PathBuf>,
    /// AES 密钥。
    pub key: Option<String>,
}

/// 批量导出窗口状态。
pub(crate) struct BatchExportWindowView {
    target: BatchExportTarget,
    config: BatchExportConfig,
    manager: Arc<TaskManager>,
    task_id: Option<u64>,
    result: Arc<Mutex<Option<Result<BatchExportReport, String>>>>,
    running: bool,
    status: String,
    report: Option<BatchExportReport>,
    error: Option<String>,
}

impl BatchExportWindowView {
    /// 创建窗口。
    pub(crate) fn new(
        target: BatchExportTarget,
        manager: Arc<TaskManager>,
        config: BatchExportConfig,
    ) -> Self {
        let config = config.normalized();
        Self {
            target,
            config,
            manager,
            task_id: None,
            result: Arc::new(Mutex::new(None)),
            running: false,
            status: tr("选择格式后开始批量导出", "Choose formats and start").to_string(),
            report: None,
            error: None,
        }
    }

    /// 切换格式。
    fn toggle_format(&mut self, format: BatchExportFormat) {
        if let Some(position) = self
            .config
            .formats
            .iter()
            .position(|candidate| *candidate == format)
        {
            self.config.formats.remove(position);
        } else {
            self.config.formats.push(format);
        }
    }

    /// 开始批量导出。
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        if self.config.formats.is_empty() {
            self.error = Some(tr("至少选择一种格式", "Choose at least one format").to_string());
            cx.notify();
            return;
        }
        self.running = true;
        self.error = None;
        self.report = None;
        self.status = tr("已提交后台导出任务…", "Export task submitted…").to_string();
        let target = self.target.clone();
        let config = self.config.clone().normalized();
        let result = Arc::new(Mutex::new(None));
        let result_for_task = result.clone();
        let label = format!(
            "{} {}",
            tr("批量导出", "Batch export"),
            target
                .archive
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("archive")
        );
        let task_id = self.manager.submit(TaskRequest::Custom {
            kind: TaskKind::Export,
            label,
            run: Box::new(move |control, reporter| {
                run_batch_task(target, config, control, reporter, result_for_task)
            }),
        });
        self.task_id = Some(task_id);
        self.result = result;
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .spawn(async move { std::thread::sleep(std::time::Duration::from_millis(120)) })
                    .await;
                let finished = this.update(cx, |view, cx| {
                    let outcome = view.result.lock().ok().and_then(|mut slot| slot.take());
                    if let Some(outcome) = outcome {
                        view.running = false;
                        match outcome {
                            Ok(report) => {
                                view.status = stage_summary(&report);
                                view.report = Some(report);
                            }
                            Err(error) => {
                                view.status = tr("批量导出失败", "Batch export failed").to_string();
                                view.error = Some(error);
                            }
                        }
                        cx.notify();
                        true
                    } else {
                        false
                    }
                });
                if matches!(finished, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// 格式按钮。
    fn format_button(&self, format: BatchExportFormat, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.config.formats.contains(&format);
        let mut button = Button::new(format!("batch-format-{}", format.extension()))
            .xsmall()
            .label(format.label())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_format(format);
                cx.notify();
            }));
        if selected {
            button = button.primary();
        } else {
            button = button.ghost();
        }
        button.into_any_element()
    }

    /// 数量限制按钮。
    fn limit_button(
        &self,
        limit: Option<usize>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.config.limit == limit;
        let id = limit.map_or("batch-limit-all".to_string(), |limit| {
            format!("batch-limit-{limit}")
        });
        let mut button = Button::new(id)
            .xsmall()
            .label(label.to_string())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.config.limit = limit;
                cx.notify();
            }));
        if selected {
            button = button.primary();
        } else {
            button = button.ghost();
        }
        button.into_any_element()
    }
}

impl Render for BatchExportWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let format_buttons: Vec<_> = BatchExportFormat::all()
            .into_iter()
            .map(|format| self.format_button(format, cx))
            .collect();
        let limit_buttons = vec![
            self.limit_button(None, tr("全部", "All"), cx),
            self.limit_button(Some(10), "10", cx),
            self.limit_button(Some(50), "50", cx),
            self.limit_button(Some(100), "100", cx),
            self.limit_button(Some(500), "500", cx),
        ];
        let mut content = v_flex()
            .p_4()
            .gap_3()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(tr("批量导出容器资产", "Batch export container assets")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.target.archive.display().to_string()),
                    ),
            )
            .child(section(
                tr("模型格式", "Model formats"),
                h_flex().gap_1().flex_wrap().children(format_buttons),
            ))
            .child(section(
                tr("材质", "Materials"),
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("batch-materials")
                            .xsmall()
                            .ghost()
                            .selected(self.config.include_materials)
                            .label(tr("解析材质", "Resolve materials"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.config.include_materials = !this.config.include_materials;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("batch-overwrite")
                            .xsmall()
                            .ghost()
                            .selected(self.config.overwrite)
                            .label(tr("覆盖已有文件", "Overwrite existing files"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.config.overwrite = !this.config.overwrite;
                                cx.notify();
                            })),
                    ),
            ))
            .child(section(
                tr("最多导出数量", "Asset limit"),
                h_flex().gap_1().flex_wrap().children(limit_buttons),
            ))
            .child(section(
                tr("输出目录", "Output directory"),
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.config.output_dir.display().to_string()),
            ))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(div().text_xs().text_color(cx.theme().danger).child(error))
                    }),
            );
        if let Some(report) = &self.report {
            content = content.child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "{} {} · {} {} · {} {} · {} {}",
                        report.exported.len(),
                        tr("资产", "assets"),
                        report.file_count(),
                        tr("文件", "files"),
                        report.skipped,
                        tr("跳过", "skipped"),
                        report.failures.len(),
                        tr("失败", "failed")
                    )))
                    .child(
                        Button::new("batch-open-dir")
                            .xsmall()
                            .ghost()
                            .label(tr("打开输出目录", "Open output folder"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let path = this.config.output_dir.clone();
                                match lumark_platform_windows::reveal(&path) {
                                    Ok(()) => {
                                        this.status =
                                            tr("已打开输出目录", "Opened output folder").to_string()
                                    }
                                    Err(error) => this.error = Some(error.to_string()),
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
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
                        h_flex().w_full().items_center().gap_2().px_3().child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(tr("批量导出", "Batch export")),
                        ),
                    ),
            )
            .child(content)
            .child(
                h_flex()
                    .w_full()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("batch-start")
                            .primary()
                            .label(tr("开始批量导出", "Start batch export"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start(cx);
                            })),
                    )
                    .when(self.running, |this| {
                        this.child(
                            Button::new("batch-cancel")
                                .ghost()
                                .label(tr("取消", "Cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(id) = this.task_id
                                        && let Some(control) = this.manager.control(id)
                                    {
                                        control.cancel();
                                    }
                                    this.status = tr("正在取消…", "Cancelling…").to_string();
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .ml_auto()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.target.archive.display().to_string()),
                    ),
            )
    }
}

/// 分节。
fn section(title: &str, content: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_xs().font_semibold().child(title.to_string()))
        .child(content)
}

/// 完成摘要。
fn stage_summary(report: &BatchExportReport) -> String {
    if report.cancelled {
        format!(
            "{} · {} {} · {:.2}s",
            tr("已取消", "Cancelled"),
            report.exported.len(),
            tr("个资产", "assets"),
            report.elapsed_seconds
        )
    } else {
        format!(
            "{} · {} {} · {:.2}s",
            tr("已完成", "Completed"),
            report.exported.len(),
            tr("个资产", "assets"),
            report.elapsed_seconds
        )
    }
}

/// 后台批量导出：写入任务结果并回报完成消息。
fn run_batch_task(
    target: BatchExportTarget,
    config: BatchExportConfig,
    control: Arc<TaskControl>,
    reporter: TaskReporter,
    result: Arc<Mutex<Option<Result<BatchExportReport, String>>>>,
) -> Result<String, String> {
    let outcome = run_batch_export(target, config, control, reporter);
    if let Ok(mut slot) = result.lock() {
        *slot = Some(outcome.clone());
    }
    outcome.map(|report| stage_summary(&report))
}

/// 实际批量导出，并把进度写入任务中心。
fn run_batch_export(
    target: BatchExportTarget,
    config: BatchExportConfig,
    control: Arc<TaskControl>,
    reporter: TaskReporter,
) -> Result<BatchExportReport, String> {
    let mappings_path = target
        .mappings
        .clone()
        .ok_or_else(|| tr("当前游戏缺少映射文件", "Game mapping file is missing").to_string())?;
    let mappings = Mappings::parse_file(&mappings_path).map_err(|error| error.to_string())?;
    let global = target.global.is_file().then_some(target.global.clone());
    let mut source = GameAssetSource::open(
        &target.archive,
        GameAssetOptions {
            key: target.key.as_deref(),
            global,
            oodle: target.oodle.clone(),
            mappings: Some(mappings_path),
        },
    )
    .map_err(|error| error.to_string())?;
    let output_dir = config.output_dir.clone();
    let cancel = control.cancelled_flag();
    let mut total = 0usize;
    let mut exported = 0usize;
    let report = export_archive_cancellable(
        &mut source,
        &mappings,
        &config,
        Some(cancel.as_ref()),
        |progress| {
            control.wait_if_paused();
            let (current, entries_done) = match progress {
                BatchProgress::Analyzing {
                    index,
                    total: count,
                    asset,
                } => {
                    total = count;
                    (asset, index.saturating_sub(1))
                }
                BatchProgress::Exported { asset, .. } => {
                    exported += 1;
                    (asset, exported)
                }
                BatchProgress::Skipped { asset, .. }
                | BatchProgress::Failed { asset, .. }
                | BatchProgress::Cancelled { asset } => (asset, exported),
            };
            reporter.progress(TaskProgress {
                current,
                entries_done,
                entries_total: total,
                ..TaskProgress::default()
            });
        },
    )?;
    let report_path = output_dir.join("batch-report.json");
    std::fs::write(&report_path, report.to_json_pretty()?)
        .map_err(|error| format!("写入导出报告失败：{error}"))?;
    Ok(report)
}

impl LumarkApp {
    /// 为当前游戏容器打开批量导出窗口。
    pub(crate) fn open_batch_export(&mut self, parent: &mut Window, cx: &mut Context<Self>) {
        let BrowserLocation::Archive { archive, .. } = self.browser.location.clone() else {
            self.notify(
                parent,
                cx,
                gpui_kit::component::notification::NotificationType::Warning,
                tr("请先打开一个游戏容器", "Open a game container first"),
            );
            return;
        };
        let extension = archive
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(extension.as_str(), "utoc" | "pak") {
            self.notify(
                parent,
                cx,
                gpui_kit::component::notification::NotificationType::Warning,
                tr(
                    "批量导出仅支持 .utoc/.pak 容器",
                    "Batch export supports .utoc/.pak containers",
                ),
            );
            return;
        }
        let Some(session) = self.game_session.as_ref() else {
            self.notify(
                parent,
                cx,
                gpui_kit::component::notification::NotificationType::Warning,
                tr("当前没有游戏会话", "No active game session"),
            );
            return;
        };
        let global = archive
            .parent()
            .map(|parent| parent.join("global.utoc"))
            .filter(|path| path.is_file())
            .unwrap_or_default();
        let saved = self
            .settings
            .get()
            .games
            .iter()
            .find(|game| game.id == session.entry_id)
            .map(|game| game.export.clone())
            .unwrap_or_default();
        let output_dir = saved
            .output_dir
            .clone()
            .unwrap_or_else(|| session.game.root.join("LumarkExports"));
        let mut formats: Vec<BatchExportFormat> = saved
            .formats
            .iter()
            .filter_map(|format| batch_format_from_setting(format))
            .collect();
        if formats.is_empty() {
            formats = vec![BatchExportFormat::Glb, BatchExportFormat::UeModel];
        }
        let config = BatchExportConfig {
            output_dir: output_dir.clone(),
            formats,
            scale: saved.scale,
            include_materials: saved.include_materials,
            max_texture_size: saved.max_texture_size,
            asset_filter: saved.asset_filter.clone(),
            limit: saved.limit,
            overwrite: saved.overwrite,
            ..BatchExportConfig::default()
        };
        let target = BatchExportTarget {
            archive,
            global,
            mappings: session.mappings.clone(),
            oodle: session.oodle.clone(),
            key: session.key.as_ref().map(|key| key.to_string()),
        };
        let manager = self.tasks.clone();
        let size = gpui_kit::size(px(720.), px(640.));
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
                title: Some(tr("批量导出 - Lumark", "Batch export - Lumark").into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(560.), px(480.))),
            ..TitleBar::window_options()
        };
        let result = gpui_kit::open_window(options, cx, move |_window, cx| {
            cx.new(|_| BatchExportWindowView::new(target.clone(), manager.clone(), config.clone()))
        });
        match result {
            Ok(_) => {
                self.status = tr("已打开批量导出窗口", "Batch export window opened").to_string()
            }
            Err(error) => self.status = format!("无法打开批量导出窗口：{error}"),
        }
    }
}

/// 设置中的格式字符串 → 批量导出格式。
fn batch_format_from_setting(value: &str) -> Option<BatchExportFormat> {
    match value.to_ascii_lowercase().as_str() {
        "glb" => Some(BatchExportFormat::Glb),
        "uemodel" => Some(BatchExportFormat::UeModel),
        "usda" => Some(BatchExportFormat::Usda),
        "psk" => Some(BatchExportFormat::Psk),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
