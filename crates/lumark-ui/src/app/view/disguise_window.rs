//! 独立伪装窗口：把一批普通文件包装成媒体外观。
//!
//! 窗口固定收集源文件、输出目录和面具；所有文件写入仍由主应用的任务池
//! 执行，关闭窗口不会影响已提交的任务。

use super::super::*;
use super::components::{AppIcon, classic_field, dialog_banner, hint_text};
use crate::i18n::tr;
use gpui_kit::Entity;
use gpui_kit::Subscription;
use gpui_kit::component::IndexPath;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use std::path::PathBuf;

/// 独立伪装窗口的状态。
pub(crate) struct DisguiseWindowView {
    app: Entity<LumarkApp>,
    sources: Vec<PathBuf>,
    output: Entity<InputState>,
    mask: DisguiseMaskPreference,
    custom_mask: Option<PathBuf>,
    delete_source: bool,
    error: Option<String>,
    mask_select: Entity<SelectState<SearchableVec<&'static str>>>,
    _subscriptions: Vec<Subscription>,
}

impl DisguiseWindowView {
    /// 创建伪装窗口状态，并初始化面具下拉框。
    pub(crate) fn new(
        app: Entity<LumarkApp>,
        sources: Vec<PathBuf>,
        output: Entity<InputState>,
        mask: DisguiseMaskPreference,
        custom_mask: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let index = disguise_mask_index(mask);
        let mask_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(disguise_mask_labels().to_vec()),
                Some(IndexPath::new(index)),
                window,
                cx,
            )
        });
        let subscriptions = vec![cx.subscribe_in(
            &mask_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.set_mask(disguise_mask_from_label(value), window, cx);
                }
            },
        )];
        Self {
            app,
            sources,
            output,
            mask,
            custom_mask,
            delete_source: false,
            error: None,
            mask_select,
            _subscriptions: subscriptions,
        }
    }

    /// 复用已有窗口时切换到新的源文件列表。
    pub(crate) fn set_sources(&mut self, sources: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.sources = sources;
        self.error = None;
        cx.notify();
    }

    /// 切换面具类型并同步下拉框选中项。
    fn set_mask(
        &mut self,
        mask: DisguiseMaskPreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mask = mask;
        self.mask_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(disguise_mask_index(mask))), window, cx);
        });
        cx.notify();
    }

    /// 打开目录选择器，选择伪装文件的输出目录。
    fn choose_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("选择伪装输出目录", "Choose disguise output folder").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(directory) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |view, window, cx| {
                    set_input(
                        &view.output,
                        directory.to_string_lossy().as_ref(),
                        window,
                        cx,
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 选择自定义面具文件。
    fn choose_mask(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("选择伪装面具文件", "Choose disguise mask file").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |view, window, cx| {
                    view.custom_mask = Some(path);
                    view.set_mask(DisguiseMaskPreference::Custom, window, cx);
                });
            }
        })
        .detach();
    }

    /// 渲染标题栏。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
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
                            .child(AppIcon::Video.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_semibold()
                            .child(tr("伪装文件 - Lumark", "Disguise file - Lumark")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} {}", self.sources.len(), tr("个文件", "files"))),
                    ),
            )
    }

    /// 渲染源文件、输出目录、面具和删除选项。
    fn render_body(&self, cx: &Context<Self>) -> impl IntoElement {
        let summary = match self.sources.first() {
            Some(first) if self.sources.len() == 1 => first.display().to_string(),
            Some(first) => format!(
                "{}{} {} {}",
                first
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(tr("文件", "file")),
                tr("等", "and"),
                self.sources.len(),
                tr("个文件", "files")
            ),
            None => tr("没有可伪装的文件", "No files to disguise").to_string(),
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .px_4()
            .py_3()
            .gap_2()
            .child(
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
                            .child(AppIcon::Video.element(15.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .child(summary),
                    ),
            )
            .child(classic_field(
                tr("输出到:", "Output to:"),
                60.,
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Input::new(&self.output)
                            .id("disguise-window-output")
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        button("disguise-window-browse")
                            .secondary()
                            .small()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .label(tr("浏览…", "Browse..."))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_directory(cx);
                                cx.notify();
                            })),
                    ),
            ))
            .child(classic_field(
                tr("面具:", "Mask:"),
                60.,
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div().w(px(180.)).flex_shrink_0().child(
                            gpui_kit::component::select::Select::new(&self.mask_select)
                                .id("disguise-window-mask-select")
                                .w_full()
                                .menu_width(px(180.))
                                .accessibility_label(tr("伪装面具", "Disguise mask")),
                        ),
                    )
                    .child(
                        button("disguise-window-choose-mask")
                            .secondary()
                            .small()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .label(tr("选择面具文件…", "Choose mask file..."))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_mask(cx);
                                cx.notify();
                            })),
                    ),
            ))
            .child(
                Checkbox::new("disguise-window-delete-source")
                    .label(tr("伪装后删除源文件", "Delete source files after disguising"))
                    .checked(self.delete_source)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.delete_source = *checked;
                        cx.notify();
                    })),
            )
            .child(match &self.error {
                Some(error) => div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone())
                    .into_any_element(),
                None => hint_text(
                    cx,
                    tr("输出文件名为「原名.面具扩展名」；载荷经混淆，只能用 Lumark 打开或还原。", "Output is named \"original name.mask extension\"; the payload is obfuscated and can only be opened or restored with Lumark."),
                )
                .into_any_element(),
            })
    }

    /// 渲染底部取消和开始伪装操作。
    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .justify_end()
            .p_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                button("disguise-window-cancel")
                    .small()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .label(tr("取消", "Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let app = this.app.clone();
                        app.update(cx, |app, cx| {
                            app.status = tr("已取消伪装", "Disguise cancelled").to_string();
                            cx.notify();
                        });
                        window.remove_window();
                    })),
            )
            .child(
                button("disguise-window-start")
                    .primary()
                    .small()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .icon(AppIcon::Video.element(14.))
                    .label(tr("开始伪装", "Start disguising"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let output = PathBuf::from(input_value(&this.output, cx));
                        if output.as_os_str().is_empty() {
                            this.error =
                                Some(tr("请选择输出目录", "Choose an output folder").to_string());
                            cx.notify();
                            return;
                        }
                        let mask =
                            match resolve_disguise_mask(this.mask, this.custom_mask.as_deref()) {
                                Ok(mask) => mask,
                                Err(message) => {
                                    this.error = Some(message);
                                    cx.notify();
                                    return;
                                }
                            };
                        let app = this.app.clone();
                        let sources = this.sources.clone();
                        let delete_source = this.delete_source;
                        let result = app.update(cx, |app, cx| {
                            app.start_disguise_from_options(
                                sources,
                                output,
                                mask,
                                delete_source,
                                window,
                                cx,
                            )
                        });
                        match result {
                            Ok(()) => {
                                app.update(cx, |app, cx| {
                                    app.disguise_window = None;
                                    app.disguise_view = None;
                                    cx.notify();
                                });
                                window.remove_window();
                            }
                            Err(message) => {
                                this.error = Some(message);
                                cx.notify();
                            }
                        }
                    })),
            )
    }
}

impl Render for DisguiseWindowView {
    /// 渲染独立伪装窗口。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(dialog_banner(cx, 46.))
            .child(self.render_body(cx))
            .child(self.render_footer(cx))
    }
}

impl LumarkApp {
    /// 打开独立伪装窗口；已有窗口时切换到新的源文件列表并激活。
    pub(crate) fn open_disguise_window(
        &mut self,
        sources: Vec<PathBuf>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(handle), Some(view)) = (self.disguise_window, self.disguise_view.clone()) {
            view.update(cx, |view, cx| view.set_sources(sources.clone(), cx));
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        self.disguise_window = None;
        self.disguise_view = None;
        let size = gpui_kit::size(px(620.), px(360.));
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
                title: Some(tr("伪装文件 - Lumark", "Disguise file - Lumark").into()),
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
        let app = cx.entity();
        let output = self.disguise_output.clone();
        let mask = self.compress_disguise_mask;
        let custom = self.compress_disguise_custom.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                DisguiseWindowView::new(
                    app.clone(),
                    sources.clone(),
                    output.clone(),
                    mask,
                    custom.clone(),
                    window,
                    cx,
                )
            })
        });
        match result {
            Ok((handle, view)) => {
                self.disguise_window = Some(handle);
                self.disguise_view = Some(view);
            }
            Err(error) => {
                self.status = format!(
                    "{}{error}",
                    tr("无法打开伪装窗口：", "Cannot open disguise window: ")
                )
            }
        }
    }
}
