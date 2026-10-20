use super::super::*;
use super::components::{AppIcon, classic_field, dialog_banner, hint_text};
use gpui_kit::Entity;
use gpui_kit::component::checkbox::Checkbox;
use std::path::PathBuf;

/// 独立解压窗口的视图状态。
///
/// 归档路径在打开窗口时固定，避免用户在文件表中改变选择后把任务提交到
/// 另一个压缩包；输出目录和密码输入仍与主应用共享，便于密码库回填。
pub(crate) struct ExtractWindowView {
    app: Entity<LumarkApp>,
    archive: PathBuf,
    output: Entity<InputState>,
    password: Entity<InputState>,
    selection_only: bool,
    error: Option<String>,
}

impl ExtractWindowView {
    /// 创建解压窗口状态。
    pub(crate) fn new(
        app: Entity<LumarkApp>,
        archive: PathBuf,
        output: Entity<InputState>,
        password: Entity<InputState>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            app,
            archive,
            output,
            password,
            selection_only: false,
            error: None,
        }
    }

    /// 切换到新的压缩包，并清除上一轮的校验错误和选择状态。
    fn set_archive(&mut self, archive: PathBuf, cx: &mut Context<Self>) {
        self.archive = archive;
        self.selection_only = false;
        self.error = None;
        cx.notify();
    }

    /// 渲染窗口标题栏和当前压缩包名称。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let name = self
            .archive
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("解压文件")
            .to_string();
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
                            .child(AppIcon::Zip.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_semibold()
                            .child("解压文件 - Lumark"),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(180.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(name),
                    ),
            )
    }

    /// 打开系统目录选择器，并把选中的目录写回共享的输出输入框。
    fn choose_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择解压目标目录".into()),
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

    /// 渲染归档路径、目标目录、密码和仅解压选中条目选项。
    fn render_body(&self, cx: &Context<Self>) -> impl IntoElement {
        let archive = self.archive.display().to_string();
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
                            .child(AppIcon::Zip.element(15.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .child(archive),
                    ),
            )
            .child(classic_field(
                "解压到:",
                60.,
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Input::new(&self.output)
                            .id("extract-window-output")
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        Button::new("extract-window-browse")
                            .secondary()
                            .small()
                            .rounded(px(6.))
                            .label("浏览…")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_directory(cx);
                                cx.notify();
                            })),
                    ),
            ))
            .child(classic_field(
                "密码:",
                60.,
                Input::new(&self.password)
                    .id("extract-window-password")
                    .w_full(),
            ))
            .child(
                Checkbox::new("extract-window-selection-only")
                    .label("仅解压选中的归档条目")
                    .checked(self.selection_only)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.selection_only = *checked;
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
                    "记录密码会自动尝试，失败后可手动输入；密码不会显示在界面中。",
                )
                .into_any_element(),
            })
    }

    /// 渲染底部取消和开始解压操作。
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
                Button::new("extract-window-cancel")
                    .small()
                    .rounded(px(6.))
                    .label("取消")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let app = this.app.clone();
                        app.update(cx, |app, cx| {
                            app.status = "已取消解压".to_string();
                            cx.notify();
                        });
                        window.remove_window();
                    })),
            )
            .child(
                Button::new("extract-window-start")
                    .primary()
                    .small()
                    .rounded(px(6.))
                    .icon(AppIcon::Zip.element(14.))
                    .label("开始解压")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let app = this.app.clone();
                        let archive = this.archive.clone();
                        let selection_only = this.selection_only;
                        let result = app.update(cx, |app, cx| {
                            app.start_extract_for(archive, selection_only, window, cx)
                        });
                        match result {
                            Ok(()) => {
                                app.update(cx, |app, cx| {
                                    app.extract_window = None;
                                    app.extract_view = None;
                                    cx.notify();
                                });
                                window.remove_window();
                            }
                            Err(error) => {
                                this.error = Some(error);
                                cx.notify();
                            }
                        }
                    })),
            )
    }
}

impl Render for ExtractWindowView {
    /// 渲染独立解压窗口。
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
    /// 关闭独立解压窗口并释放视图，供密码库解锁流程接管界面。
    pub(crate) fn dismiss_extract_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.extract_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.extract_view = None;
    }

    /// 打开独立解压窗口；已有窗口时切换到新的压缩包并激活。
    pub(crate) fn open_extract_window(
        &mut self,
        archive: PathBuf,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(handle), Some(view)) = (self.extract_window, self.extract_view.clone()) {
            view.update(cx, |view, cx| view.set_archive(archive.clone(), cx));
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        self.extract_window = None;
        self.extract_view = None;
        let size = gpui_kit::size(px(620.), px(340.));
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
                title: Some("解压文件 - Lumark".into()),
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
        let output = self.extract_output.clone();
        let password = self.extract_password.clone();
        let archive_for_view = archive.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                ExtractWindowView::new(
                    app.clone(),
                    archive_for_view.clone(),
                    output.clone(),
                    password.clone(),
                    window,
                    cx,
                )
            })
        });
        match result {
            Ok((handle, view)) => {
                self.extract_window = Some(handle);
                self.extract_view = Some(view);
            }
            Err(error) => self.status = format!("无法打开解压窗口：{error}"),
        }
    }
}
