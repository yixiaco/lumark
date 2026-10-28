//! 独立密码相关窗口入口。

use super::super::*;
use super::components::{AppIcon, banner_gradient, form_label, hint_text};
use crate::i18n::tr;
use gpui_kit::Entity;
use gpui_kit::component::TitleBar;

/// 独立密码编辑窗口的视图。
///
/// 新建和编辑都在这一个窗口中完成，窗口持有自己的输入实体，不再复用密码库
/// 页面上的表单控件。保存时直接写入加密密码库，不需要额外的确认步骤。
pub(crate) struct PasswordEditorWindowView {
    app: Entity<LumarkApp>,
    editing: Option<Uuid>,
    label: Entity<InputState>,
    password: Entity<InputState>,
    pattern: Entity<InputState>,
    note: Entity<InputState>,
    password_visible: bool,
    error: Option<String>,
}

impl PasswordEditorWindowView {
    /// 创建密码编辑窗口，并在编辑模式下回填已有记录。
    pub(crate) fn new(
        app: Entity<LumarkApp>,
        entry: Option<PasswordEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let label = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("名称，例如：工作压缩包", "Name, e.g. Work archive").to_string())
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("密码", "Password").to_string())
                .masked(true)
        });
        let pattern = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                tr("适用路径或通配符（可选）", "Path or wildcard (optional)").to_string(),
            )
        });
        let note = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("备注（可选）", "Note (optional)").to_string())
        });
        let editing = entry.as_ref().map(|entry| entry.id);
        let view = Self {
            app,
            editing,
            label,
            password,
            pattern,
            note,
            password_visible: false,
            error: None,
        };
        if let Some(entry) = entry {
            set_input(&view.label, &entry.label, window, cx);
            set_input(&view.password, entry.password.expose(), window, cx);
            if let Some(pattern) = entry.archive_pattern.as_deref() {
                set_input(&view.pattern, pattern, window, cx);
            }
            set_input(&view.note, &entry.note, window, cx);
        }
        view
    }

    /// 渲染窗口标题栏和渐变页头。
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .flex_shrink_0()
            .child(self.render_title_bar(cx))
            .child(self.render_banner(cx))
    }

    /// 渲染独立窗口的系统标题栏内容。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let title = if self.editing.is_some() {
            tr("编辑密码记录", "Edit password record")
        } else {
            tr("新建密码记录", "New password record")
        };
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
                            .child(AppIcon::Key.element(14.)),
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
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.editing.is_some() {
                                tr("编辑记录", "Edit record")
                            } else {
                                tr("新建记录", "New record")
                            }),
                    ),
            )
    }

    /// 渲染标题栏下方的渐变说明带。
    fn render_banner(&self, cx: &Context<Self>) -> impl IntoElement {
        let title = if self.editing.is_some() {
            tr("编辑密码记录", "Edit password record")
        } else {
            tr("新建密码记录", "New password record")
        };
        let caption = if self.editing.is_some() {
            tr(
                "修改后立即写回本地加密密码库",
                "Changes are written to the local encrypted vault immediately",
            )
        } else {
            tr(
                "保存后立即写入本地加密密码库",
                "Saved directly to the local encrypted vault",
            )
        };
        banner_gradient(cx)
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_start()
            .gap_3()
            .px_5()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size(px(34.))
                    .rounded(px(9.))
                    .bg(cx.theme().primary.opacity(0.14))
                    .text_color(cx.theme().primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(AppIcon::Key.element(17.)),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(1.))
                    .child(div().text_sm().font_semibold().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(caption),
                    ),
            )
    }

    /// 渲染名称、密码、匹配规则和备注表单。
    fn render_body(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .px_5()
            .py_4()
            .gap_3()
            .when_some(self.error.clone(), |this, error| {
                this.child(password_error_banner(cx, error))
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(form_label(cx, tr("名称", "Name")))
                    .child(Input::new(&self.label).id("password-editor-label").w_full()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(form_label(cx, tr("密码", "Password")))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                Input::new(&self.password)
                                    .id("password-editor-value")
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                button("password-editor-reveal")
                                    .ghost()
                                    .small()
                                    .rounded(px(6.))
                                    .icon(
                                        if self.password_visible {
                                            AppIcon::Lock
                                        } else {
                                            AppIcon::Unlock
                                        }
                                        .element(14.),
                                    )
                                    .tooltip(if self.password_visible {
                                        tr("隐藏密码", "Hide password")
                                    } else {
                                        tr("显示密码", "Show password")
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.password_visible = !this.password_visible;
                                        let masked = !this.password_visible;
                                        this.password.update(cx, |state, cx| {
                                            state.set_masked(masked, window, cx);
                                        });
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(hint_text(
                        cx,
                        tr("只保存一次密码，不再要求重复确认；保存后可在列表中显示或复制。", "Enter the password once; no confirmation is required. It can be shown or copied from the list."),
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(form_label(cx, tr("适用压缩包", "Archive pattern")))
                    .child(
                        Input::new(&self.pattern)
                            .id("password-editor-pattern")
                            .w_full(),
                    )
                    .child(hint_text(
                        cx,
                        tr("支持完整路径或 * ? 通配符；留空表示通用密码。", "Supports a full path or * ? wildcards; leave empty for a general password."),
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(form_label(cx, tr("备注", "Note")))
                    .child(Input::new(&self.note).id("password-editor-note").w_full()),
            )
    }

    /// 渲染取消和保存操作。
    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .justify_end()
            .p_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                button("password-editor-cancel")
                    .ghost()
                    .rounded(px(6.))
                    .label(tr("取消", "Cancel"))
                    .on_click(cx.listener(|_, _, window, _| {
                        window.remove_window();
                    })),
            )
            .child(
                button("password-editor-save")
                    .primary()
                    .rounded(px(6.))
                    .icon(AppIcon::Save.element(14.))
                    .label(if self.editing.is_some() {
                        tr("保存修改", "Save changes")
                    } else {
                        tr("保存到密码库", "Save to vault")
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        let label = input_value(&this.label, cx);
                        let password = input_value(&this.password, cx);
                        let pattern = input_value(&this.pattern, cx);
                        let note = input_value(&this.note, cx);
                        let result = this.app.update(cx, |app, _| {
                            app.save_password_entry(this.editing, &label, password, &pattern, &note)
                        });
                        match result {
                            Ok(_) => {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| {
                                    app.password_editor_window = None;
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

impl Render for PasswordEditorWindowView {
    /// 渲染独立密码编辑窗口。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .child(self.render_header(cx))
            .child(self.render_body(cx))
            .child(self.render_footer(cx))
    }
}

/// 独立主密码设置窗口的视图。
///
/// 当前主密码、新主密码和确认输入都由窗口独占，避免与密码库页面的
/// 解锁输入共享同一个 InputState 而同时获得焦点。
pub(crate) struct MasterPasswordWindowView {
    app: Entity<LumarkApp>,
    has_master: bool,
    current: Entity<InputState>,
    new_password: Entity<InputState>,
    confirm: Entity<InputState>,
    error: Option<String>,
}

impl MasterPasswordWindowView {
    /// 创建主密码窗口，并按当前是否已设置主密码决定字段。
    pub(crate) fn new(
        app: Entity<LumarkApp>,
        has_master: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("当前主密码", "Current master password").to_string())
                .masked(true)
        });
        let new_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(
                    tr(
                        "新主密码（至少 8 位）",
                        "New master password (at least 8 characters)",
                    )
                    .to_string(),
                )
                .masked(true)
        });
        let confirm = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("再次输入新主密码", "Repeat new master password").to_string())
                .masked(true)
        });
        Self {
            app,
            has_master,
            current,
            new_password,
            confirm,
            error: None,
        }
    }

    /// 渲染窗口标题栏和渐变页头。
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .flex_shrink_0()
            .child(self.render_title_bar(cx))
            .child(self.render_banner(cx))
    }

    /// 渲染独立窗口的系统标题栏内容。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let title = if self.has_master {
            tr("修改主密码", "Change master password")
        } else {
            tr("设置主密码", "Set master password")
        };
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
                            .child(AppIcon::Shield.element(14.)),
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
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("Lumark 密码库", "Lumark vault")),
                    ),
            )
    }

    /// 渲染标题栏下方的主密码说明带。
    fn render_banner(&self, cx: &Context<Self>) -> impl IntoElement {
        let title = if self.has_master {
            tr("修改主密码", "Change master password")
        } else {
            tr("设置主密码", "Set master password")
        };
        banner_gradient(cx)
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_start()
            .gap_3()
            .px_5()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size(px(34.))
                    .rounded(px(9.))
                    .bg(cx.theme().primary.opacity(0.14))
                    .text_color(cx.theme().primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(AppIcon::Shield.element(17.)),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(1.))
                    .child(div().text_sm().font_semibold().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("设置后移除 DPAPI 包裹；忘记主密码将无法恢复。", "Setting one removes DPAPI wrapping; a forgotten master password cannot be recovered.")),
                    ),
            )
    }

    /// 渲染当前主密码、新主密码和确认输入。
    fn render_body(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .px_5()
            .py_4()
            .gap_3()
            .when_some(self.error.clone(), |this, error| {
                this.child(password_error_banner(cx, error))
            })
            .when(self.has_master, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(form_label(cx, tr("当前主密码", "Current master password")))
                        .child(
                            Input::new(&self.current)
                                .id("master-window-current")
                                .w_full(),
                        ),
                )
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(form_label(cx, tr("新主密码", "New master password")))
                    .child(
                        Input::new(&self.new_password)
                            .id("master-window-new")
                            .w_full(),
                    )
                    .child(hint_text(
                        cx,
                        tr(
                            "至少 8 个字符；请使用与其他账户不同的密码。",
                            "At least 8 characters; use a password different from other accounts.",
                        ),
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(form_label(
                        cx,
                        tr("确认新主密码", "Confirm new master password"),
                    ))
                    .child(
                        Input::new(&self.confirm)
                            .id("master-window-confirm")
                            .w_full(),
                    ),
            )
    }

    /// 渲染取消和设置操作。
    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .justify_end()
            .p_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                button("master-window-cancel")
                    .ghost()
                    .rounded(px(6.))
                    .label(tr("取消", "Cancel"))
                    .on_click(cx.listener(|_, _, window, _| {
                        window.remove_window();
                    })),
            )
            .child(
                button("master-window-save")
                    .primary()
                    .rounded(px(6.))
                    .icon(AppIcon::Shield.element(14.))
                    .label(if self.has_master {
                        tr("保存新主密码", "Save new master password")
                    } else {
                        tr("设置主密码", "Set master password")
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        let current = if this.has_master {
                            input_value(&this.current, cx)
                        } else {
                            String::new()
                        };
                        let new = input_value(&this.new_password, cx);
                        let confirm = input_value(&this.confirm, cx);
                        if new.chars().count() < 8 {
                            this.error = Some(
                                tr(
                                    "主密码至少需要 8 个字符",
                                    "Master password must be at least 8 characters",
                                )
                                .to_string(),
                            );
                            cx.notify();
                            return;
                        }
                        if new != confirm {
                            this.error = Some(
                                tr(
                                    "两次输入的新主密码不一致",
                                    "The new master passwords do not match",
                                )
                                .to_string(),
                            );
                            cx.notify();
                            return;
                        }
                        let result = this
                            .app
                            .update(cx, |app, _| app.set_master_password(&current, &new));
                        match result {
                            Ok(()) => {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| {
                                    app.master_password_window = None;
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

impl Render for MasterPasswordWindowView {
    /// 渲染独立主密码窗口。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .child(self.render_header(cx))
            .child(self.render_body(cx))
            .child(self.render_footer(cx))
    }
}

/// 在表单顶部渲染始终可见的错误提示。
///
/// 错误放在字段之前而不是窗口底部，避免内容增长时被底部分隔线裁切；
/// 浅红底色也让校验失败与普通说明文字区分开。
fn password_error_banner(cx: &gpui_kit::App, error: String) -> gpui_kit::Div {
    div()
        .w_full()
        .rounded(px(6.))
        .border_1()
        .border_color(cx.theme().danger.opacity(0.35))
        .bg(cx.theme().danger.opacity(0.14))
        .px_3()
        .py_2()
        .text_xs()
        .text_color(cx.theme().danger_foreground)
        .child(error)
}

impl LumarkApp {
    /// 打开独立密码编辑窗口；新窗口会替换上一个编辑窗口。
    pub(crate) fn open_password_editor_window(
        &mut self,
        entry: Option<PasswordEntry>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.vault.state() != VaultState::Unlocked {
            self.status = tr("请先解锁密码库", "Unlock the vault first").to_string();
            return;
        }
        self.close_password_editor_window(cx);
        let editing = entry.is_some();
        let size = gpui_kit::size(px(540.), px(460.));
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
                title: Some(
                    if editing {
                        tr("编辑密码 - Lumark", "Edit password - Lumark")
                    } else {
                        tr("新建密码 - Lumark", "New password - Lumark")
                    }
                    .into(),
                ),
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
        let entry_for_view = entry;
        match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                PasswordEditorWindowView::new(app.clone(), entry_for_view.clone(), window, cx)
            })
        }) {
            Ok((handle, _)) => self.password_editor_window = Some(handle),
            Err(error) => {
                self.status = format!(
                    "{}{error}",
                    tr("无法打开密码编辑窗口：", "Cannot open password editor: ")
                )
            }
        }
    }

    /// 打开独立主密码设置窗口；已有窗口会被替换。
    pub(crate) fn open_master_password_window(&mut self, parent: &Window, cx: &mut Context<Self>) {
        if self.vault.state() != VaultState::Unlocked {
            self.status = tr("请先解锁密码库", "Unlock the vault first").to_string();
            return;
        }
        self.close_master_password_window(cx);
        let has_master = self.vault.has_master_password();
        let size = gpui_kit::size(px(480.), px(if has_master { 420. } else { 350. }));
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
                title: Some(tr("主密码 - Lumark", "Master password - Lumark").into()),
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
        match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| MasterPasswordWindowView::new(app.clone(), has_master, window, cx))
        }) {
            Ok((handle, _)) => self.master_password_window = Some(handle),
            Err(error) => {
                self.status = format!(
                    "{}{error}",
                    tr(
                        "无法打开主密码窗口：",
                        "Cannot open master password window: "
                    )
                )
            }
        }
    }

    /// 关闭仍然存在的密码编辑窗口。
    pub(crate) fn close_password_editor_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.password_editor_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// 关闭仍然存在的主密码窗口。
    pub(crate) fn close_master_password_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.master_password_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// 密码库锁定或自动锁定时关闭所有密码输入窗口。
    pub(crate) fn dismiss_password_windows(&mut self, cx: &mut Context<Self>) {
        self.close_password_editor_window(cx);
        self.close_master_password_window(cx);
    }
}
