//! 单资产导出窗口（E-14）：格式/贴图/旁挂选择、后台导出、结果与目录打开。

use super::super::*;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::AnyElement;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, TitleBar};
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div, px};
use lumark_export::TextureExportFormat;
use lumark_unreal::{
    AssetExportConfig, AssetExportProgress, AssetExportReport, BatchExportFormat, GameAssetOptions,
    GameAssetSource, Mappings, export_asset,
};

use crate::app::{BrowserLocation, LumarkApp};
use crate::i18n::tr;

/// 导出目标（从游戏会话与当前选中资产捕获）。
#[derive(Debug, Clone)]
pub(crate) struct ExportTarget {
    /// 容器路径。
    pub archive: PathBuf,
    /// global.utoc（可能不存在）。
    pub global: PathBuf,
    /// 映射文件。
    pub mappings: Option<PathBuf>,
    /// Oodle 库。
    pub oodle: Option<PathBuf>,
    /// AES 密钥。
    pub key: Option<String>,
    /// 容器内资产路径。
    pub asset_path: String,
    /// 展示名。
    pub display_name: String,
    /// 默认输出目录。
    pub output_dir: PathBuf,
}

/// 导出窗口状态。
pub(crate) struct ExportWindowView {
    target: ExportTarget,
    config: AssetExportConfig,
    running: bool,
    status: String,
    report: Option<AssetExportReport>,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
}

impl ExportWindowView {
    /// 创建导出窗口。
    pub(crate) fn new(target: ExportTarget) -> Self {
        let config = AssetExportConfig {
            output_dir: target.output_dir.clone(),
            formats: vec![BatchExportFormat::Glb, BatchExportFormat::Usda],
            ..AssetExportConfig::default()
        };
        Self {
            target,
            config,
            running: false,
            status: tr("选择格式后开始导出", "Choose formats and start").to_string(),
            report: None,
            error: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 切换一个格式。
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

    /// 开始后台导出。
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
        self.status = tr("正在解析资产并导出…", "Exporting…").to_string();
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let target = self.target.clone();
        let config = self.config.clone().normalized();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { run_export(target, config, cancel) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.running = false;
                match result {
                    Ok(report) => {
                        view.status = if report.cancelled {
                            tr("导出已取消", "Export cancelled").to_string()
                        } else {
                            format!(
                                "{} {} · {} {} · {:.2}s",
                                tr("已导出", "Exported"),
                                report.files.len(),
                                tr("文件", "files"),
                                report.sidecar_files.len(),
                                report.elapsed_seconds
                            )
                        };
                        view.report = Some(report);
                    }
                    Err(error) => {
                        view.status = tr("导出失败", "Export failed").to_string();
                        view.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 格式选择按钮。
    fn format_button(&self, format: BatchExportFormat, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.config.formats.contains(&format);
        let mut button = Button::new(format!("export-format-{}", format.extension()))
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

    /// 贴图格式按钮。
    fn texture_button(&self, format: TextureExportFormat, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.config.texture_format == format;
        let mut button = Button::new(format!("export-texture-{}", format.extension()))
            .label(format.label())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.config.texture_format = format;
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

impl Render for ExportWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let output = self.config.output_dir.display().to_string();
        let format_buttons: Vec<_> = BatchExportFormat::all()
            .into_iter()
            .map(|format| self.format_button(format, cx))
            .collect();
        let texture_buttons: Vec<_> = TextureExportFormat::all()
            .into_iter()
            .map(|format| self.texture_button(format, cx))
            .collect();
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
                            .child(self.target.display_name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.target.asset_path.clone()),
                    ),
            )
            .child(section(
                tr("模型格式", "Model formats"),
                h_flex().gap_1().flex_wrap().children(format_buttons),
            ))
            .child(section(
                tr("材质与贴图", "Materials and textures"),
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new("export-sidecars")
                                    .label(tr("写出材质旁挂", "Write material sidecars"))
                                    .selected(self.config.write_sidecars)
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.config.write_sidecars = !this.config.write_sidecars;
                                        cx.notify();
                                    })),
                            )
                            .child(format!(
                                "{} · {}",
                                tr("贴图格式", "Texture format"),
                                self.config.texture_format.label()
                            )),
                    )
                    .child(h_flex().gap_1().flex_wrap().children(texture_buttons)),
            ))
            .child(section(
                tr("输出目录", "Output directory"),
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(output),
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
                        "{} {} · {} {} · {} {}",
                        report.files.len(),
                        tr("个文件", "files"),
                        report.sidecar_files.len(),
                        tr("个旁挂", "sidecars"),
                        report.vertices,
                        tr("顶点", "vertices")
                    )))
                    .child(
                        Button::new("export-open-dir")
                            .ghost()
                            .label(tr("打开输出目录", "Open output folder"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let path = this.config.output_dir.clone();
                                match lumark_platform_windows::reveal(&path) {
                                    Ok(()) => {
                                        this.status =
                                            tr("已打开输出目录", "Opened output folder").to_string()
                                    }
                                    Err(error) => {
                                        this.error = Some(error.to_string());
                                    }
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
                                .child(tr("导出资产", "Export asset")),
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
                        Button::new("export-start")
                            .primary()
                            .label(tr("开始导出", "Start export"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start(cx);
                            })),
                    )
                    .when(self.running, |this| {
                        this.child(
                            Button::new("export-cancel")
                                .ghost()
                                .label(tr("取消", "Cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel.store(true, Ordering::Relaxed);
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

/// 分节标题 + 内容。
fn section(title: &str, content: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_xs().font_semibold().child(title.to_string()))
        .child(content)
}

/// 后台执行导出。
fn run_export(
    target: ExportTarget,
    config: AssetExportConfig,
    cancel: Arc<AtomicBool>,
) -> Result<AssetExportReport, String> {
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
    export_asset(
        &mut source,
        &mappings,
        &target.asset_path,
        &config,
        Some(cancel.as_ref()),
        |_progress: AssetExportProgress| {},
    )
}

impl LumarkApp {
    /// 为当前选中的游戏资产打开导出窗口。
    pub(crate) fn open_asset_export(&mut self, parent: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.notify(
                parent,
                cx,
                gpui_kit::component::notification::NotificationType::Warning,
                tr("请先选中一个游戏资产", "Select a game asset first"),
            );
            return;
        };
        if entry.is_dir
            || !entry
                .path
                .extension()
                .is_some_and(|extension| extension == "uasset")
        {
            self.notify(
                parent,
                cx,
                gpui_kit::component::notification::NotificationType::Warning,
                tr(
                    "仅支持导出 .uasset 资产",
                    "Only .uasset assets can be exported",
                ),
            );
            return;
        }
        let BrowserLocation::Archive { archive, .. } = self.browser.location.clone() else {
            self.notify(
                parent,
                cx,
                gpui_kit::component::notification::NotificationType::Warning,
                tr("请在游戏容器中导出资产", "Open a game container first"),
            );
            return;
        };
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
        let output_dir = dirs::desktop_dir()
            .unwrap_or_else(|| self.temp_root.clone())
            .join("LumarkExports");
        let target = ExportTarget {
            archive,
            global,
            mappings: session.mappings.clone(),
            oodle: session.oodle.clone(),
            key: session.key.as_ref().map(|key| key.to_string()),
            asset_path: entry.path.to_string_lossy().replace('\\', "/"),
            display_name: entry.name.clone(),
            output_dir,
        };
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
                title: Some(tr("导出资产 - Lumark", "Export asset - Lumark").into()),
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
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            window.on_next_frame(|window, _| crate::window_focus::activate_and_raise(window));
            cx.new(|_| ExportWindowView::new(target.clone()))
        });
        match result {
            Ok(_) => self.status = tr("已打开导出窗口", "Export window opened").to_string(),
            Err(error) => self.status = format!("无法打开导出窗口：{error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::test::TestWindowExt;

    fn target() -> ExportTarget {
        ExportTarget {
            archive: PathBuf::from("Game/pakchunk0-Windows.utoc"),
            global: PathBuf::from("Game/global.utoc"),
            mappings: Some(PathBuf::from("Game/mapping.usmap")),
            oodle: None,
            key: None,
            asset_path: "Game/SM_Box.uasset".to_string(),
            display_name: "SM_Box".to_string(),
            output_dir: PathBuf::from("out"),
        }
    }

    /// 格式按钮切换、贴图格式与默认输出。
    #[test]
    fn export_window_toggles_formats() {
        let mut view = ExportWindowView::new(target());
        assert_eq!(
            view.config.formats,
            vec![BatchExportFormat::Glb, BatchExportFormat::Usda]
        );
        view.toggle_format(BatchExportFormat::Glb);
        assert_eq!(view.config.formats, vec![BatchExportFormat::Usda]);
        view.toggle_format(BatchExportFormat::Glb);
        assert_eq!(view.config.formats.len(), 2);
        view.toggle_format(BatchExportFormat::Psk);
        assert!(view.config.formats.contains(&BatchExportFormat::Psk));
        view.config.texture_format = TextureExportFormat::Tga;
        assert_eq!(view.config.texture_format.extension(), "tga");
        assert_eq!(view.config.output_dir, PathBuf::from("out"));
    }

    /// 导出窗口渲染格式/贴图/开始按钮。
    #[gpui_kit::test]
    fn export_window_renders_controls(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let (_view, cx) = cx.add_window_view(|_, _| ExportWindowView::new(target()));
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(window.find("export-start").visible());
            assert!(window.find("export-format-glb").visible());
            assert!(window.find("export-format-usda").visible());
            assert!(window.find("export-texture-png").visible());
            assert!(window.find("export-sidecars").visible());
        });
    }
}
