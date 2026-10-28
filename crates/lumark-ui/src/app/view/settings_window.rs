//! 独立设置窗口：标题栏、选项卡、通用/外观/关于页面和窗口生命周期。
use super::super::*;
use super::super::{apply_theme_accent, hsla_to_hex};
use super::components::{AppIcon, banner_gradient, segmented_tab, segmented_tab_bar};
use super::settings_window_selects::CompressionSelect;
use crate::i18n::tr;
use gpui_kit::component::TitleBar;
use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::try_parse_color;
use gpui_kit::{Entity, Subscription};

/// 独立设置窗口的视图。
///
/// 设置项仍然写入主应用持有的设置存储，窗口只负责展示和派发操作；
/// 选项卡索引和密码库位置编辑状态由窗口自己维护，关闭窗口后不会影响主界面。
pub(crate) struct SettingsWindowView {
    /// 主应用句柄。
    pub(super) app: Entity<LumarkApp>,
    /// 当前设置页签。
    pub(super) tab: usize,
    /// 强调色选择器状态。
    pub(super) accent_picker: Entity<ColorPickerState>,
    _accent_subscription: Subscription,
    /// 密码库位置选择器是否打开。
    pub(super) vault_location_open: bool,
    /// 密码库位置输入框。
    pub(super) vault_location_input: Entity<InputState>,
    /// 密码库位置错误。
    pub(super) vault_location_error: Option<String>,
    /// SMTP 主机输入框。
    pub(super) smtp_host: Entity<InputState>,
    /// SMTP 端口输入框。
    pub(super) smtp_port: Entity<InputState>,
    /// SMTP 用户名输入框。
    pub(super) smtp_username: Entity<InputState>,
    /// SMTP 密码输入框；留空保留已保存密码。
    pub(super) smtp_password: Entity<InputState>,
    /// 发件人输入框。
    pub(super) smtp_from: Entity<InputState>,
    /// 收件人输入框。
    pub(super) smtp_to: Entity<InputState>,
    /// 是否清除已保存的 SMTP 密码。
    pub(super) smtp_clear_password: bool,
    /// 当前设置快照。
    pub(super) settings: Settings,
    /// 设置文件路径。
    pub(super) settings_path: PathBuf,
    /// 密码库路径。
    pub(super) vault_path: PathBuf,
    /// 密码库锁定状态。
    pub(super) vault_state: VaultState,
    /// 密码库是否设置主密码。
    pub(super) vault_has_master: bool,
    /// 密码库错误。
    pub(super) vault_error: Option<String>,
    /// 已注册文件关联数量。
    pub(super) file_association_count: usize,
    /// 选中的关联格式。
    pub(super) selected_formats: HashSet<String>,
    /// 设置页状态文本。
    pub(super) status: String,
    /// 默认格式下拉框。
    pub(super) format_select: CompressionSelect,
    /// 默认压缩预设下拉框。
    pub(super) preset_select: CompressionSelect,
    /// 覆盖策略下拉框。
    pub(super) overwrite_select: CompressionSelect,
    /// 默认面具下拉框。
    pub(super) disguise_select: CompressionSelect,
    _select_subscriptions: Vec<Subscription>,
}

/// 打开设置窗口时从主应用复制的一份只读快照。
///
/// 窗口第一次渲染发生在主应用仍处于更新状态时，此时不能再次读取主应用
/// 实体；因此所有首帧数据都通过快照传入，后续操作再显式刷新。
#[derive(Clone)]
pub(crate) struct SettingsWindowSnapshot {
    /// 设置快照。
    pub(crate) settings: Settings,
    /// 设置文件路径。
    pub(crate) settings_path: PathBuf,
    /// 密码库路径。
    pub(crate) vault_path: PathBuf,
    /// 密码库状态。
    pub(crate) vault_state: VaultState,
    /// 是否有主密码。
    pub(crate) vault_has_master: bool,
    /// 密码库错误。
    pub(crate) vault_error: Option<String>,
    /// 文件关联数量。
    pub(crate) file_association_count: usize,
    /// 已选关联格式。
    pub(crate) selected_formats: HashSet<String>,
    /// 状态文本。
    pub(crate) status: String,
}

impl SettingsWindowView {
    /// 创建设置窗口，并按传入快照初始化首帧数据。
    pub(crate) fn new(
        app: Entity<LumarkApp>,
        snapshot: SettingsWindowSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let accent_picker = cx.new(|cx| {
            let mut state = ColorPickerState::new(window, cx);
            if let Ok(color) = try_parse_color(&snapshot.settings.accent_color) {
                state.set_value(color, window, cx);
            }
            state
        });
        let accent_subscription =
            cx.subscribe(&accent_picker, |this, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                let hex = hsla_to_hex(*color);
                this.app.update(cx, |app, cx| {
                    match app
                        .settings
                        .update(|settings| settings.accent_color = hex.clone())
                    {
                        Ok(()) => {
                            apply_theme_accent(&hex, cx);
                            app.status = format!("主题色：{hex}");
                        }
                        Err(error) => app.status = error.to_string(),
                    }
                    cx.notify();
                });
                this.settings.accent_color = hex.clone();
                this.status = format!("主题色：{hex}");
                cx.notify();
            });
        let vault_location_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("密码库保存文件夹", "Vault folder").to_string())
        });
        let smtp_host = cx.new(|cx| InputState::new(window, cx).placeholder("smtp.example.com"));
        let smtp_port = cx.new(|cx| InputState::new(window, cx).placeholder("465"));
        let smtp_username =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("登录用户名", "Username")));
        let smtp_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("留空保留已保存密码")
                .masked(true)
        });
        let smtp_from =
            cx.new(|cx| InputState::new(window, cx).placeholder("Lumark <name@example.com>"));
        let smtp_to = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("收件人，多个用逗号分隔", "Recipients, comma-separated"))
        });
        set_input(&smtp_host, &snapshot.settings.smtp_host, window, cx);
        set_input(
            &smtp_port,
            &snapshot.settings.smtp_port.to_string(),
            window,
            cx,
        );
        set_input(&smtp_username, &snapshot.settings.smtp_username, window, cx);
        set_input(&smtp_from, &snapshot.settings.smtp_from, window, cx);
        set_input(&smtp_to, &snapshot.settings.smtp_to, window, cx);
        let (format_select, preset_select, overwrite_select, disguise_select, select_subscriptions) =
            Self::new_compression_selects(&snapshot.settings, window, cx);
        Self {
            app,
            tab: 0,
            accent_picker,
            _accent_subscription: accent_subscription,
            vault_location_open: false,
            vault_location_input,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            smtp_from,
            smtp_to,
            smtp_clear_password: false,
            vault_location_error: None,
            settings: snapshot.settings,
            settings_path: snapshot.settings_path,
            vault_path: snapshot.vault_path,
            vault_state: snapshot.vault_state,
            vault_has_master: snapshot.vault_has_master,
            vault_error: snapshot.vault_error,
            file_association_count: snapshot.file_association_count,
            selected_formats: snapshot.selected_formats,
            status: snapshot.status,
            format_select,
            preset_select,
            overwrite_select,
            disguise_select,
            _select_subscriptions: select_subscriptions,
        }
    }

    /// 在窗口操作结束后重新读取主应用状态，保持首帧快照与后台变化一致。
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        let app = self.app.read(cx);
        self.settings = app.settings.get();
        self.settings_path = app.settings.path().to_path_buf();
        self.vault_path = app.vault.path().to_path_buf();
        self.vault_state = app.vault.state();
        self.vault_has_master = app.vault.has_master_password();
        self.vault_error = app.vault_error.clone();
        self.file_association_count = app.file_association_count;
        self.status = app.status.clone();
    }

    /// 渲染窗口标题栏和渐变页头。
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .flex_shrink_0()
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
                                    .child(AppIcon::Setting.element(14.)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .font_semibold()
                                    .child(tr("设置 - Lumark", "Settings - Lumark")),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("立即生效"),
                            ),
                    ),
            )
            .child(
                banner_gradient(cx)
                    .w_full()
                    .h(px(70.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .size(px(36.))
                            .rounded(px(10.))
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .shadow_sm()
                            .child(AppIcon::Setting.element(19.)),
                    )
                    .child(
                        v_flex()
                            .gap(px(1.))
                            .child(div().text_lg().font_semibold().child(tr("设置", "Settings")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("外观、压缩偏好、文件关联、通知和本机安全", "Appearance, compression, file associations, notifications and local security")),
                            ),
                    ),
            )
    }

    /// 渲染顶部的七个分段选项卡。
    fn render_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected = self.tab;
        let view = cx.entity().downgrade();
        let tabs = [
            (AppIcon::Grid, tr("综合", "General")),
            (AppIcon::Tool, tr("外观", "Appearance")),
            (AppIcon::Compression, tr("压缩", "Compression")),
            (AppIcon::FileZip, tr("文件关联", "Associations")),
            (AppIcon::Key, tr("密码库", "Vault")),
            (AppIcon::Shield, tr("通知", "Notifications")),
            (AppIcon::Time, tr("关于", "About")),
        ];
        let items = tabs
            .iter()
            .enumerate()
            .map(|(index, (icon, label))| {
                let view = view.clone();
                segmented_tab(
                    cx,
                    ("settings-window-tab", index),
                    *icon,
                    label,
                    selected == index,
                )
                .set_position(index + 1, tabs.len())
                .on_click(move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.tab = index;
                        this.refresh(cx);
                        cx.notify();
                    })
                    .ok();
                })
                .into_any_element()
            })
            .collect::<Vec<_>>();

        segmented_tab_bar(cx, "settings-window-tabs", items)
    }

    /// 渲染当前选项卡内容，并在内容区域独立滚动。
    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.tab {
            0 => self.render_general_tab(cx).into_any_element(),
            1 => self.render_appearance_tab(cx).into_any_element(),
            2 => self.render_compression_tab(cx).into_any_element(),
            3 => self.render_association_tab(cx).into_any_element(),
            4 => self.render_vault_tab(cx).into_any_element(),
            5 => self.render_notification_tab(cx).into_any_element(),
            _ => self.render_about_tab(cx).into_any_element(),
        };
        v_flex()
            .id(format!("settings-window-content-{}", self.tab))
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .p_4()
            .gap_3()
            .child(content)
    }

    /// 渲染底部状态、恢复默认和关闭操作。
    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_3()
            .items_center()
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status),
            )
            .child(
                button("settings-reset")
                    .ghost()
                    .rounded(px(6.))
                    .icon(AppIcon::Refresh.element(15.))
                    .label(tr("恢复默认设置", "Restore defaults"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.reset_preferences(window, cx);
                    })),
            )
            .child(
                button("settings-close")
                    .primary()
                    .rounded(px(6.))
                    .icon(AppIcon::Check.element(15.))
                    .label("完成")
                    .on_click(cx.listener(|_, _, window, _| {
                        window.remove_window();
                    })),
            )
    }

    /// 把设置窗口中的一次操作转交给主应用并刷新两个界面。
    pub(super) fn update_app(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut LumarkApp, &mut Context<LumarkApp>),
    ) {
        self.app.update(cx, |app, app_cx| {
            update(app, app_cx);
            app_cx.notify();
        });
        self.refresh(cx);
        cx.notify();
    }

    /// 恢复出厂设置并同步颜色选择器的当前色块。
    fn reset_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .app
            .update(cx, |app, app_cx| app.reset_preferences(window, app_cx));
        match result {
            Ok(()) => {
                self.refresh(cx);
                self.sync_compression_selects(window, cx);
                let accent = self.settings.accent_color.clone();
                if let Ok(color) = try_parse_color(&accent) {
                    self.accent_picker
                        .update(cx, |state, cx| state.set_value(color, window, cx));
                }
                cx.notify();
            }
            Err(error) => {
                let status = format!(
                    "{}{error}",
                    tr("无法恢复默认设置：", "Cannot restore defaults: ")
                );
                self.status = status.clone();
                self.app.update(cx, |app, cx| {
                    app.status = status;
                    cx.notify();
                });
                cx.notify();
            }
        }
    }
}

impl Render for SettingsWindowView {
    /// 渲染独立设置窗口。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_header(cx))
            .child(div().w_full().px_4().pt_3().child(self.render_tabs(cx)))
            .child(self.render_content(cx))
            .child(self.render_footer(cx))
            .when(self.vault_location_open, |this| {
                this.child(self.render_vault_location_modal(cx))
            })
    }
}

impl LumarkApp {
    /// 打开独立设置窗口；已有窗口时直接带到前台。
    pub(crate) fn open_settings_window(&mut self, parent: &Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        self.settings_window = None;
        let size = gpui_kit::size(px(760.), px(560.));
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
                title: Some(tr("设置 - Lumark", "Settings - Lumark").into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(760.), px(560.))),
            ..TitleBar::window_options()
        };
        let app = cx.entity();
        let registered_formats = platform::shell::registered_formats();
        let selected_formats = if registered_formats.is_empty() {
            platform::shell::ARCHIVE_EXTENSIONS
                .iter()
                .map(|extension| extension.to_string())
                .collect()
        } else {
            registered_formats.into_iter().collect()
        };
        let snapshot = SettingsWindowSnapshot {
            settings: self.settings.get(),
            settings_path: self.settings.path().to_path_buf(),
            vault_path: self.vault.path().to_path_buf(),
            vault_state: self.vault.state(),
            vault_has_master: self.vault.has_master_password(),
            vault_error: self.vault_error.clone(),
            file_association_count: self.file_association_count,
            selected_formats,
            status: self.status.clone(),
        };
        match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| SettingsWindowView::new(app.clone(), snapshot.clone(), window, cx))
        }) {
            Ok((handle, _)) => self.settings_window = Some(handle),
            Err(error) => {
                self.status = format!(
                    "{}{error}",
                    tr("无法打开设置窗口：", "Cannot open settings window: ")
                )
            }
        }
    }
}

/// 设置窗口使用的标准卡片。
pub(super) fn settings_card(cx: &gpui_kit::App) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(10.))
        .shadow_xs()
        .overflow_hidden()
}

/// 设置窗口的分组标题，使用比主页面更紧凑的尺寸。
pub(super) fn settings_section(
    cx: &gpui_kit::App,
    icon: AppIcon,
    title: &str,
    caption: Option<String>,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .items_center()
        .gap_3()
        .child(
            div()
                .size(px(32.))
                .flex_shrink_0()
                .rounded(px(9.))
                .bg(cx.theme().primary.opacity(0.12))
                .text_color(cx.theme().primary)
                .flex()
                .items_center()
                .justify_center()
                .child(icon.element(16.)),
        )
        .child(
            v_flex()
                .min_w_0()
                .gap(px(1.))
                .child(div().text_sm().font_semibold().child(title.to_string()))
                .when(caption.is_some(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(caption.unwrap_or_default()),
                    )
                }),
        )
        .child(div().flex_1().h(px(1.)).bg(cx.theme().border))
}

/// 渲染双栏设置行。
pub(super) fn settings_row(
    cx: &gpui_kit::App,
    title: &str,
    description: &str,
    control: impl IntoElement,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_4()
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_sm().font_semibold().child(title.to_string()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .text_ellipsis()
                        .child(description.to_string()),
                ),
        )
        .child(control)
}

/// 渲染设置卡片内部的分隔线。
pub(super) fn settings_divider(cx: &gpui_kit::App) -> gpui_kit::Div {
    div().w_full().h(px(1.)).bg(cx.theme().border)
}
