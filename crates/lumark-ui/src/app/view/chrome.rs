use super::super::*;
use super::components::{
    AppIcon, icon_button, location_icon, primary_icon_button, secondary_icon_button,
};

impl LumarkApp {
    /// Renders the custom title bar and product identity.
    ///
    /// `TitleBar` owns the Windows drag region and system buttons. The content
    /// here is deliberately compact so the working area remains dominant on
    /// laptops while still giving Lumark a recognizable local-tool identity.
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(50.))
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(30.))
                            .rounded_md()
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Compression.element(18.)),
                    )
                    .child(
                        v_flex()
                            .gap(px(0.))
                            .child(div().text_sm().font_semibold().child("Lumark"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("本地归档工作台"),
                            ),
                    )
                    .child(
                        div()
                            .ml_2()
                            .px_2()
                            .py(px(2.))
                            .rounded_full()
                            .bg(cx.theme().success)
                            .text_color(cx.theme().success_foreground)
                            .text_xs()
                            .child("AES-256 就绪"),
                    ),
            )
    }

    /// Renders one navigation row in the global sidebar.
    fn navigation_item(
        &self,
        id: &'static str,
        icon: AppIcon,
        label: &'static str,
        target: Screen,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.screen == target;
        h_flex()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .h(px(38.))
            .w_full()
            .gap_2()
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .when(active, |this| {
                this.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .when(!active, |this| {
                this.text_color(cx.theme().sidebar_foreground)
                    .hover(|style| style.bg(cx.theme().sidebar_primary))
            })
            .child(icon.element(17.))
            .child(div().text_sm().font_semibold().child(label))
            .when(active, |this| {
                this.child(
                    div()
                        .ml_auto()
                        .w(px(3.))
                        .h(px(18.))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.switch_screen(target);
                cx.notify();
            }))
    }

    /// Renders one quick-access location in the sidebar.
    fn location_item(
        &self,
        index: usize,
        label: &str,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.browser.current_display() == path.display().to_string();
        let path = path.to_path_buf();
        h_flex()
            .id(("sidebar-location", index))
            .role(gpui_kit::Role::Button)
            .aria_label(label.to_string())
            .h(px(32.))
            .w_full()
            .gap_2()
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .text_color(if active {
                cx.theme().sidebar_accent_foreground
            } else {
                cx.theme().sidebar_foreground
            })
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .when(!active, |this| {
                this.hover(|style| style.bg(cx.theme().sidebar_primary))
            })
            .child(location_icon(label).element(15.))
            .child(div().text_sm().child(label.to_string()))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.navigate_to(path.clone(), window, cx);
                cx.notify();
            }))
    }

    /// Renders one lazily discovered directory-tree row.
    fn tree_item(
        &self,
        index: usize,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let expand_path = row.path.clone();
        let open_path = row.path.clone();
        h_flex()
            .id(("sidebar-tree", index))
            .role(gpui_kit::Role::Button)
            .aria_label(row.label.clone())
            .h(px(28.))
            .w_full()
            .gap_1()
            .pl(px(8. + row.depth as f32 * 12.))
            .pr_2()
            .rounded_md()
            .cursor_pointer()
            .text_color(cx.theme().sidebar_foreground)
            .hover(|style| style.bg(cx.theme().sidebar_primary))
            .child(
                div()
                    .id(("tree-expand", index))
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_xs()
                    .hover(|style| style.bg(cx.theme().secondary))
                    .child(if row.expanded { "▾" } else { "▸" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_tree_path(expand_path.clone());
                        cx.notify();
                    })),
            )
            .child(AppIcon::FolderOpen.element(14.))
            .child(
                div()
                    .id(("tree-open", index))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_sm()
                    .child(row.label.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.navigate_to(open_path.clone(), window, cx);
                        cx.notify();
                    })),
            )
    }

    /// Renders the persistent left sidebar shared by all application screens.
    ///
    /// Filesystem shortcuts and the lazy directory tree appear only while the
    /// browser is active. Password and settings screens keep the same navigation
    /// rail, which avoids reorienting users when they switch tools.
    fn render_navigation_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locations = quick_locations(&self.settings.get());
        let tree_rows = if self.screen == Screen::Browser {
            self.tree_rows()
        } else {
            Vec::new()
        };
        let vault_locked = self.vault.state() == VaultState::Locked;
        let vault_caption = if vault_locked {
            "密码库已锁定"
        } else {
            "密码库已解锁"
        };
        v_flex()
            .id("global-sidebar")
            .w(px(220.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .text_color(cx.theme().sidebar_foreground)
            .child(
                v_flex()
                    .p_3()
                    .gap_1()
                    .child(
                        div()
                            .px_3()
                            .pb_2()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("工作区"),
                    )
                    .child(self.navigation_item(
                        "nav-files",
                        AppIcon::FileZip,
                        "文件",
                        Screen::Browser,
                        cx,
                    ))
                    .child(self.navigation_item(
                        "nav-passwords",
                        AppIcon::Key,
                        "密码库",
                        Screen::Passwords,
                        cx,
                    ))
                    .child(self.navigation_item(
                        "nav-settings",
                        AppIcon::Setting,
                        "设置",
                        Screen::Settings,
                        cx,
                    )),
            )
            .child(div().mx_3().h(px(1.)).bg(cx.theme().sidebar_border))
            .child(
                v_flex()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3()
                    .py_3()
                    .gap_1()
                    .when(self.screen == Screen::Browser, |this| {
                        this.child(
                            div()
                                .px_3()
                                .pb_1()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .child("快速访问"),
                        )
                        .children(locations.iter().enumerate().map(|(index, (label, path))| {
                            self.location_item(index, label, path, cx)
                        }))
                        .child(
                            div()
                                .mt_2()
                                .mb_1()
                                .mx_3()
                                .h(px(1.))
                                .bg(cx.theme().sidebar_border),
                        )
                        .child(
                            h_flex()
                                .px_3()
                                .pb_1()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("文件夹"),
                                )
                                .child(
                                    icon_button(
                                        "sidebar-refresh-tree",
                                        AppIcon::Refresh,
                                        "刷新目录树",
                                    )
                                    .ml_auto()
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.tree_children.clear();
                                            this.status = "目录树已刷新".to_string();
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                        .children(
                            tree_rows
                                .iter()
                                .enumerate()
                                .map(|(index, row)| self.tree_item(index, row, cx)),
                        )
                    })
                    .when(self.screen != Screen::Browser, |this| {
                        this.child(
                            v_flex()
                                .p_3()
                                .gap_2()
                                .child(AppIcon::Shield.element(22.))
                                .child(div().text_sm().font_semibold().child("隐私优先"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("密码与归档记录仅保存在本机，不参与云同步。"),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .m_3()
                    .p_3()
                    .rounded_lg()
                    .bg(cx.theme().sidebar_primary)
                    .border_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .size(px(30.))
                                    .rounded_md()
                                    .bg(if vault_locked {
                                        cx.theme().warning
                                    } else {
                                        cx.theme().success
                                    })
                                    .text_color(if vault_locked {
                                        cx.theme().warning_foreground
                                    } else {
                                        cx.theme().success_foreground
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        if vault_locked {
                                            AppIcon::Lock
                                        } else {
                                            AppIcon::Unlock
                                        }
                                        .element(16.),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap(px(0.))
                                    .child(div().text_xs().font_semibold().child(vault_caption))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{} 条记录", self.vault.entry_count())),
                                    ),
                            )
                            .child(
                                icon_button(
                                    "sidebar-vault-open",
                                    AppIcon::ArrowRight,
                                    "打开密码库",
                                )
                                .ml_auto()
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.switch_screen(Screen::Passwords);
                                        cx.notify();
                                    },
                                )),
                            ),
                    ),
            )
    }

    /// Chooses the single-row command bar on wide windows and the two-row
    /// compact layout when DPI scaling or window size would otherwise clip the
    /// path, search field, and primary actions.
    fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logical_width = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .width
            .as_f32();
        if self.screen == Screen::Browser && logical_width < 1500. {
            self.render_toolbar_compact(cx).into_any_element()
        } else {
            self.render_toolbar_wide(cx).into_any_element()
        }
    }

    /// Renders the two-row browser command bar used at common laptop widths.
    fn render_toolbar_compact(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let search_active = self.search_results.is_some();
        v_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .id("command-bar")
                    .h(px(52.))
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                icon_button("browser-back", AppIcon::ArrowLeft, "后退").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        if let Err(error) = this.browser.go_back() {
                                            this.status = error.to_string();
                                        } else {
                                            this.search_results = None;
                                            set_input(
                                                &this.location_input,
                                                this.browser.current_display().as_str(),
                                                window,
                                                cx,
                                            );
                                        }
                                        cx.notify();
                                    }),
                                ),
                            )
                            .child(
                                icon_button("browser-forward", AppIcon::ArrowRight, "前进")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Err(error) = this.browser.go_forward() {
                                            this.status = error.to_string();
                                        } else {
                                            this.search_results = None;
                                            set_input(
                                                &this.location_input,
                                                this.browser.current_display().as_str(),
                                                window,
                                                cx,
                                            );
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(icon_button("up", AppIcon::ArrowUp, "上一级").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.go_parent();
                                    set_input(
                                        &this.location_input,
                                        this.browser.current_display().as_str(),
                                        window,
                                        cx,
                                    );
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .h(px(34.))
                            .px_2()
                            .gap_2()
                            .overflow_hidden()
                            .rounded_md()
                            .bg(cx.theme().secondary)
                            .border_1()
                            .border_color(cx.theme().input)
                            .child(AppIcon::FolderOpen.element(15.))
                            .child(
                                Input::new(&self.location_input)
                                    .id("location-input")
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                Button::new("go-location")
                                    .ghost()
                                    .small()
                                    .label("转到")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.go_location(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .h(px(38.))
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .px_4()
                    .bg(cx.theme().secondary)
                    .child(
                        h_flex()
                            .w(px(210.))
                            .h(px(30.))
                            .px_2()
                            .gap_1()
                            .rounded_md()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().input)
                            .child(AppIcon::Search.element(14.))
                            .child(
                                Input::new(&self.search_input)
                                    .id("search-input")
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                Button::new("search-run")
                                    .ghost()
                                    .small()
                                    .label("搜索")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_search(cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mr_2()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("快速操作"),
                    )
                    .child(
                        icon_button("refresh", AppIcon::Refresh, "刷新").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.refresh();
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        icon_button("new-folder", AppIcon::FolderPlus, "新建文件夹").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.start_new_folder(window, cx);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        icon_button(
                            "view-toggle",
                            if self.tile_view {
                                AppIcon::List
                            } else {
                                AppIcon::Grid
                            },
                            if self.tile_view {
                                "详细视图"
                            } else {
                                "平铺视图"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tile_view = !this.tile_view;
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button("details-toggle-panel", AppIcon::Doc, "显示或隐藏详情")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_details();
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button("test-archive", AppIcon::Shield, "测试压缩包").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.start_test(window, cx);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        icon_button("task-center", AppIcon::List, "任务中心").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.show_task_center = !this.show_task_center;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        icon_button("browser-delete", AppIcon::Delete, "删除所选").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.request_delete();
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        Button::new("column-layout")
                            .ghost()
                            .small()
                            .label(match self.column_layout {
                                0 => "紧凑列",
                                2 => "宽松列",
                                _ => "标准列",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_column_layout();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("search-mode")
                            .ghost()
                            .small()
                            .label(match self.search_mode {
                                SearchMode::Contains => "包含",
                                SearchMode::Wildcard => "通配",
                                SearchMode::Regex => "正则",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.search_mode = match this.search_mode {
                                    SearchMode::Contains => SearchMode::Wildcard,
                                    SearchMode::Wildcard => SearchMode::Regex,
                                    SearchMode::Regex => SearchMode::Contains,
                                };
                                cx.notify();
                            })),
                    )
                    .when(search_active, |this| {
                        this.child(
                            Button::new("search-clear")
                                .ghost()
                                .small()
                                .icon(AppIcon::Close.element(14.))
                                .label("清除搜索")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.search_results = None;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("compress")
                                    .primary()
                                    .small()
                                    .icon(AppIcon::Compression.element(16.))
                                    .tooltip("压缩")
                                    .accessibility_label("压缩")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_compress_dialog(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("extract")
                                    .secondary()
                                    .small()
                                    .icon(AppIcon::Zip.element(16.))
                                    .tooltip("解压")
                                    .accessibility_label("解压")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_extract_dialog(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

    /// Renders the full single-row command bar for wide windows.
    fn render_toolbar_wide(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let browser = self.screen == Screen::Browser;
        let search_active = self.search_results.is_some();
        h_flex()
            .id("command-bar")
            .h(px(58.))
            .flex_shrink_0()
            .w_full()
            .items_center()
            .gap_2()
            .px_4()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .when(browser, |this| {
                this.child(
                    h_flex()
                        .gap_1()
                        .child(
                            icon_button("browser-back", AppIcon::ArrowLeft, "后退").on_click(
                                cx.listener(|this, _, window, cx| {
                                    if let Err(error) = this.browser.go_back() {
                                        this.status = error.to_string();
                                    } else {
                                        this.search_results = None;
                                        set_input(
                                            &this.location_input,
                                            this.browser.current_display().as_str(),
                                            window,
                                            cx,
                                        );
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            icon_button("browser-forward", AppIcon::ArrowRight, "前进").on_click(
                                cx.listener(|this, _, window, cx| {
                                    if let Err(error) = this.browser.go_forward() {
                                        this.status = error.to_string();
                                    } else {
                                        this.search_results = None;
                                        set_input(
                                            &this.location_input,
                                            this.browser.current_display().as_str(),
                                            window,
                                            cx,
                                        );
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(icon_button("up", AppIcon::ArrowUp, "上一级").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.go_parent();
                                set_input(
                                    &this.location_input,
                                    this.browser.current_display().as_str(),
                                    window,
                                    cx,
                                );
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .h(px(36.))
                        .px_2()
                        .gap_2()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().input)
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(AppIcon::FolderOpen.element(16.)),
                        )
                        .child(
                            Input::new(&self.location_input)
                                .id("location-input")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            Button::new("go-location")
                                .ghost()
                                .small()
                                .label("转到")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.go_location(window, cx);
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .w(px(232.))
                        .h(px(36.))
                        .px_2()
                        .gap_2()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().input)
                        .child(AppIcon::Search.element(15.))
                        .child(
                            Input::new(&self.search_input)
                                .id("search-input")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            Button::new("search-mode")
                                .ghost()
                                .small()
                                .label(match self.search_mode {
                                    SearchMode::Contains => "包含",
                                    SearchMode::Wildcard => "通配",
                                    SearchMode::Regex => "正则",
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.search_mode = match this.search_mode {
                                        SearchMode::Contains => SearchMode::Wildcard,
                                        SearchMode::Wildcard => SearchMode::Regex,
                                        SearchMode::Regex => SearchMode::Contains,
                                    };
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("search-run")
                                .ghost()
                                .small()
                                .label("搜索")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.run_search(cx);
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(icon_button("refresh", AppIcon::Refresh, "刷新").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.refresh();
                                cx.notify();
                            }),
                        ))
                        .child(
                            icon_button("new-folder", AppIcon::FolderPlus, "新建文件夹").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.start_new_folder(window, cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            icon_button(
                                "view-toggle",
                                if self.tile_view {
                                    AppIcon::List
                                } else {
                                    AppIcon::Grid
                                },
                                if self.tile_view {
                                    "详细视图"
                                } else {
                                    "平铺视图"
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tile_view = !this.tile_view;
                                cx.notify();
                            })),
                        )
                        .child(
                            icon_button("details-toggle-panel", AppIcon::Doc, "显示或隐藏详情")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_details();
                                    cx.notify();
                                })),
                        )
                        .child(
                            icon_button("test-archive", AppIcon::Shield, "测试压缩包").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.start_test(window, cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            icon_button("task-center", AppIcon::List, "任务中心").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.show_task_center = !this.show_task_center;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            primary_icon_button("compress", AppIcon::Compression, "压缩").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_compress_dialog(window, cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            secondary_icon_button("extract", AppIcon::Zip, "解压").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_extract_dialog(window, cx);
                                    cx.notify();
                                }),
                            ),
                        ),
                )
                .when(search_active, |this| {
                    this.child(
                        Button::new("search-clear")
                            .ghost()
                            .small()
                            .icon(AppIcon::Close.element(14.))
                            .tooltip("清除搜索")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.search_results = None;
                                cx.notify();
                            })),
                    )
                })
            })
            .when(!browser, |this| {
                let (icon, title, caption) = match self.screen {
                    Screen::Passwords => (
                        AppIcon::Key,
                        "密码管理",
                        format!(
                            "{} 条加密记录 · {}",
                            self.vault.entry_count(),
                            if self.vault.state() == VaultState::Locked {
                                "已锁定"
                            } else {
                                "已解锁"
                            }
                        ),
                    ),
                    Screen::Settings => (
                        AppIcon::Setting,
                        "设置",
                        "外观、压缩偏好与本机安全".to_string(),
                    ),
                    Screen::Browser => unreachable!(),
                };
                this.child(
                    h_flex()
                        .flex_1()
                        .gap_3()
                        .child(
                            div()
                                .size(px(34.))
                                .rounded_md()
                                .bg(cx.theme().accent)
                                .text_color(cx.theme().accent_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon.element(18.)),
                        )
                        .child(
                            v_flex()
                                .gap(px(0.))
                                .child(div().text_lg().font_semibold().child(title))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(caption),
                                ),
                        ),
                )
                .child(
                    icon_button("command-tasks", AppIcon::List, "任务中心").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.show_task_center = !this.show_task_center;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    icon_button("command-theme", AppIcon::Tool, "切换主题").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.cycle_theme(window, cx);
                            cx.notify();
                        },
                    )),
                )
            })
    }

    /// Renders the bottom status strip and compact task controls.
    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.active_task.is_some();
        let progress_text = self
            .task_progress
            .as_ref()
            .map(|progress| {
                format!(
                    "{} · {:.0}% · {}",
                    progress.current,
                    progress.ratio() * 100.0,
                    format_size(progress.bytes_done)
                )
            })
            .unwrap_or_else(|| self.status.clone());
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        h_flex()
            .h(px(30.))
            .flex_shrink_0()
            .px_4()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().size(px(6.)).rounded_full().bg(if running {
                cx.theme().warning
            } else {
                cx.theme().success
            }))
            .child(
                div()
                    .max_w(px(560.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(progress_text),
            )
            .when(running, |this| {
                let ratio = self
                    .task_progress
                    .as_ref()
                    .map_or(0.0, TaskProgress::ratio)
                    .clamp(0.0, 1.0);
                this.child(
                    div()
                        .w(px(120.))
                        .h(px(5.))
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .w(px(120. * ratio))
                                .h_full()
                                .rounded_full()
                                .bg(cx.theme().primary),
                        ),
                )
                .child(
                    icon_button("pause-task", AppIcon::Time, "暂停或继续").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(id) = this.active_task
                                && let Some(control) = this.tasks.control(id)
                            {
                                if control.state() == lumark_tasks::TaskState::Paused {
                                    control.resume();
                                } else {
                                    control.pause();
                                }
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    icon_button("cancel-task", AppIcon::Close, "取消任务").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(id) = this.active_task
                                && let Some(control) = this.tasks.control(id)
                            {
                                control.cancel();
                            }
                            cx.notify();
                        },
                    )),
                )
            })
            .child(div().ml_auto().child(format!(
                "{} 项 · {}",
                self.displayed_count(),
                format_size(total_size)
            )))
            .child(div().child("v0.1.0"))
    }
}

impl Render for LumarkApp {
    /// Composes the shared application shell, active page, and modal layers.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.pending_requests.is_empty() {
            let request = self.pending_requests.remove(0);
            self.process_request(request, window, cx);
        }
        if self.last_size_save.elapsed() >= std::time::Duration::from_secs(2) {
            let size = window.inner_window_bounds().get_bounds().size;
            let current = (size.width.as_f32(), size.height.as_f32());
            if (current.0 - self.last_window_size.0).abs() >= 1.
                || (current.1 - self.last_window_size.1).abs() >= 1.
            {
                let _ = self.settings.update(|settings| {
                    settings.window_width = current.0;
                    settings.window_height = current.1;
                });
                self.last_window_size = current;
            }
            self.last_size_save = std::time::Instant::now();
        }

        let content = match self.screen {
            Screen::Browser => self.render_browser(window, cx).into_any_element(),
            Screen::Passwords => self.render_passwords(cx).into_any_element(),
            Screen::Settings => self.render_settings(cx).into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .min_w_0()
                    .items_stretch()
                    .child(self.render_navigation_sidebar(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.render_toolbar(window, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .w_full()
                                    .min_w_0()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(content),
                            )
                            .child(self.render_status_bar(cx)),
                    ),
            )
            .when(self.show_context_menu, |this| {
                this.child(self.render_file_context_menu(cx))
            })
            .when(self.show_compress, |this| {
                let dialog = self.render_compress_dialog(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_extract, |this| {
                let dialog = self.render_extract_dialog(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_save_prompt, |this| {
                let dialog = self.render_save_prompt(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_unlock_for_extract, |this| {
                let dialog = self.render_unlock_prompt(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_extra_password_prompt, |this| {
                let dialog = self.render_extra_password_prompt(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_open_executable_confirm, |this| {
                let dialog = self.render_open_executable_confirm(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_delete_confirm, |this| {
                let dialog = self.render_delete_confirm(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_new_folder, |this| {
                let dialog = self.render_new_folder(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_rename, |this| {
                let dialog = self.render_rename(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_vault_transfer, |this| {
                let dialog = self.render_vault_transfer(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_confirm_remove_master, |this| {
                let dialog = self.render_confirm_remove_master(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_confirm_clear_vault, |this| {
                let dialog = self.render_confirm_clear_vault(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_discard_password_draft, |this| {
                let dialog = self.render_discard_password_draft(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_task_center, |this| {
                this.child(self.render_task_center(cx))
            })
    }
}

/// Wraps a modal card in a dimming layer that absorbs clicks outside it.
fn dialog_layer(content: impl IntoElement) -> gpui_kit::Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(222. / 360., 0.18, 0.08, 0.42))
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, _| {})
        .child(content)
}
