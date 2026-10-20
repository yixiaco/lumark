use super::super::*;
use super::components::{
    AppIcon, BannerTone, banner_action, banner_gradient, brand_lockup, icon_button, location_icon,
};
use gpui_kit::component::input::{
    InputGroup, InputGroupAddon, InputGroupAddonAlignment, InputGroupButton, InputGroupInput,
};

impl LumarkApp {
    /// 渲染自绘标题栏：当前位置、工作区切换和全局工具入口。
    ///
    /// `TitleBar` 负责 Windows 拖拽区域和系统按钮，因此标题内容保持单行，
    /// 长路径会收缩而不是把工作区入口挤出窗口。
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match self.screen {
            Screen::Browser => format!("{} - Lumark", self.browser.current_display()),
            Screen::Passwords => "密码库 - Lumark".to_string(),
            Screen::Settings => "设置 - Lumark".to_string(),
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
                    .child(
                        div()
                            .px_2()
                            .py(px(1.))
                            .rounded_full()
                            .bg(cx.theme().success)
                            .text_color(cx.theme().success_foreground)
                            .text_xs()
                            .child("AES-256"),
                    )
                    .child(div().w(px(1.)).h(px(20.)).bg(cx.theme().border))
                    .child(self.top_nav_button(
                        "title-nav-files",
                        AppIcon::FileZip,
                        "文件",
                        Screen::Browser,
                        cx,
                    ))
                    .child(self.top_nav_button(
                        "title-nav-passwords",
                        AppIcon::Key,
                        "密码库",
                        Screen::Passwords,
                        cx,
                    ))
                    .child(self.top_nav_button(
                        "title-nav-settings",
                        AppIcon::Setting,
                        "设置",
                        Screen::Settings,
                        cx,
                    ))
                    .child(div().w(px(1.)).h(px(20.)).bg(cx.theme().border))
                    .child(
                        icon_button("title-theme", AppIcon::Tool, "切换主题")
                            .occlude()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cycle_theme(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 渲染标题栏中的一个工作区切换按钮。
    fn top_nav_button(
        &self,
        id: &'static str,
        icon: AppIcon,
        label: &'static str,
        target: Screen,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.screen == target;
        Button::new(id)
            .ghost()
            .small()
            .selected(active)
            .occlude()
            .icon(icon.element(14.))
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.switch_screen(target);
                cx.notify();
            }))
    }

    /// 渲染参考经典归档器的大图标渐变动作带。
    ///
    /// 主操作、破坏性操作和任务入口被分隔线分组；窄窗口只隐藏品牌标语，
    /// 不隐藏任何动作，避免功能在笔记本宽度下消失。
    fn render_action_band(&self, window: &Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if self.screen != Screen::Browser {
            return self.render_page_band(cx).into_any_element();
        }
        let logical_width = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .width
            .as_f32();
        let wide = logical_width >= 1100.;
        banner_gradient(cx)
            .id("action-band")
            .w_full()
            .h(px(if wide { 72. } else { 68. }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(banner_action(
                        cx,
                        "compress",
                        AppIcon::Compression,
                        "压缩",
                        BannerTone::Primary,
                        |this, window, cx| this.open_compress_dialog(window, cx),
                    ))
                    .child(banner_action(
                        cx,
                        "extract",
                        AppIcon::Zip,
                        "解压到",
                        BannerTone::Success,
                        |this, window, cx| this.open_extract_dialog(window, cx),
                    ))
                    .child(banner_action(
                        cx,
                        "browser-delete",
                        AppIcon::Delete,
                        "删除",
                        BannerTone::Danger,
                        |this, _, _| this.request_delete(),
                    ))
                    .child(banner_action(
                        cx,
                        "test-archive",
                        AppIcon::Shield,
                        "测试",
                        BannerTone::Warning,
                        |this, window, cx| this.start_test(window, cx),
                    ))
                    .child(banner_action(
                        cx,
                        "task-center",
                        AppIcon::List,
                        "任务",
                        BannerTone::Neutral,
                        |this, _, _| this.show_task_center = !this.show_task_center,
                    )),
            )
            .child(h_flex().ml_auto().pr_2().child(brand_lockup(cx, wide)))
            .into_any_element()
    }

    /// 渲染密码库和设置页共享的页头横幅。
    fn render_page_band(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        banner_gradient(cx)
            .w_full()
            .h(px(78.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size(px(38.))
                    .rounded(px(10.))
                    .bg(cx.theme().button_primary)
                    .text_color(cx.theme().button_primary_foreground)
                    .flex()
                    .items_center()
                    .justify_center()
                    .shadow_sm()
                    .child(icon.element(20.)),
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
            )
            .child(
                h_flex()
                    .ml_auto()
                    .gap_2()
                    .child(
                        Button::new("banner-back-files")
                            .secondary()
                            .small()
                            .icon(AppIcon::ArrowLeft.element(15.))
                            .label("返回文件")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.switch_screen(Screen::Browser);
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button("banner-tasks", AppIcon::List, "任务中心").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.show_task_center = !this.show_task_center;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        icon_button("banner-theme", AppIcon::Tool, "切换主题").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.cycle_theme(window, cx);
                                cx.notify();
                            }),
                        ),
                    ),
            )
    }

    /// 渲染全宽导航行：浏览按钮、路径栏、搜索栏和视图开关。
    ///
    /// 参考图片把地址栏与搜索框放在同一行，这里按窗口宽度收缩搜索框和
    /// 搜索模式标签，保证 960 像素最小窗口下主操作仍然可见。
    fn render_navigation_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logical_width = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .width
            .as_f32();
        let narrow = logical_width < 1100.;
        let search_active = self.search_results.is_some();
        let mode_label = match self.search_mode {
            SearchMode::Contains => "包含",
            SearchMode::Wildcard => "通配",
            SearchMode::Regex => "正则",
        };
        h_flex()
            .id("command-bar")
            .h(px(42.))
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .px_3()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_1()
                    .flex_shrink_0()
                    .child(
                        icon_button(
                            "sidebar-toggle",
                            if self.sidebar_visible {
                                AppIcon::MenuFold
                            } else {
                                AppIcon::MenuUnfold
                            },
                            if self.sidebar_visible {
                                "隐藏文件夹栏"
                            } else {
                                "显示文件夹栏"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_visible = !this.sidebar_visible;
                            cx.notify();
                        })),
                    )
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
                    .child(
                        icon_button("up", AppIcon::ArrowUp, "上一级").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.go_parent();
                                set_input(
                                    &this.location_input,
                                    this.browser.current_display().as_str(),
                                    window,
                                    cx,
                                );
                                cx.notify();
                            },
                        )),
                    )
                    .child(div().w(px(1.)).h(px(20.)).mx_1().bg(cx.theme().border))
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
                    ),
            )
            .child(
                InputGroup::new("location-group")
                    .flex_1()
                    .min_w(px(150.))
                    .input(
                        InputGroupInput::new(&self.location_input)
                            .id("location-input")
                            .aria_label("当前路径"),
                    )
                    .addon(
                        InputGroupAddon::new("location-icon").child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(AppIcon::FolderOpen.element(15.)),
                        ),
                    )
                    .addon(
                        InputGroupAddon::new("location-go")
                            .align(InputGroupAddonAlignment::InlineEnd)
                            .child(
                                InputGroupButton::new("go-location")
                                    .label("转到")
                                    .tooltip("打开输入的路径")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.go_location(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                InputGroup::new("search-group")
                    .w(px(if narrow { 210. } else { 286. }))
                    .flex_shrink_0()
                    .input(
                        InputGroupInput::new(&self.search_input)
                            .id("search-input")
                            .aria_label("搜索名称或通配符"),
                    )
                    .addon(
                        InputGroupAddon::new("search-icon").child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(AppIcon::Search.element(14.)),
                        ),
                    )
                    .addon(
                        InputGroupAddon::new("search-run-addon")
                            .align(InputGroupAddonAlignment::InlineEnd)
                            .child(
                                InputGroupButton::new("search-run")
                                    .label("搜索")
                                    .tooltip("在当前目录搜索")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_search(cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                Button::new("search-mode")
                    .secondary()
                    .small()
                    .label(if narrow {
                        "高级".to_string()
                    } else {
                        format!("高级 · {mode_label}")
                    })
                    .tooltip("切换搜索匹配方式：包含、通配符、正则")
                    .accessibility_label("高级搜索模式")
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
                        .tooltip("清除搜索")
                        .accessibility_label("清除搜索")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.search_results = None;
                            cx.notify();
                        })),
                )
            })
    }

    /// 渲染持久文件侧栏：常用位置、文件夹树、详细信息区和密码库状态。
    ///
    /// 常用位置和磁盘树合并为一个列表，对应参考图片中单一的“文件夹”区域；
    /// 窗口较矮时压缩详细信息区的高度，保证目录树仍有一行以上可见。
    fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locations = quick_locations(&self.settings.get());
        let tree_rows = self.tree_rows();
        let vault_locked = self.vault.state() == VaultState::Locked;
        let window_height = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .height
            .as_f32();
        let details_max_height = if window_height < 720. { 170. } else { 200. };
        v_flex()
            .id("global-sidebar")
            .w(px(252.))
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .text_color(cx.theme().sidebar_foreground)
            .child(
                h_flex()
                    .h(px(34.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_1()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("文件夹"),
                    )
                    .child(
                        h_flex()
                            .ml_auto()
                            .gap_1()
                            .child(
                                icon_button("sidebar-refresh-tree", AppIcon::Refresh, "刷新目录树")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.tree_children.clear();
                                        this.status = "目录树已刷新".to_string();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("sidebar-close", AppIcon::Close, "隐藏文件夹栏")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_visible = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .children(locations.iter().enumerate().map(|(index, (label, path))| {
                        self.sidebar_location_item(index, label, path, cx)
                    }))
                    .child(div().mx_3().my_2().h(px(1.)).bg(cx.theme().sidebar_border))
                    .children(
                        tree_rows
                            .iter()
                            .enumerate()
                            .map(|(index, row)| self.sidebar_tree_item(index, row, cx)),
                    ),
            )
            .when(self.details_visible, |this| {
                this.child(self.render_details(details_max_height, cx))
            })
            .child(
                h_flex()
                    .m_2()
                    .p_2()
                    .flex_shrink_0()
                    .gap_2()
                    .items_center()
                    .rounded_md()
                    .bg(cx.theme().sidebar_primary)
                    .border_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        div()
                            .size(px(26.))
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
                                .element(14.),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap(px(0.))
                            .min_w_0()
                            .child(div().text_xs().font_semibold().child(if vault_locked {
                                "密码库已锁定"
                            } else {
                                "密码库已解锁"
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} 条记录", self.vault.entry_count())),
                            ),
                    )
                    .child(
                        icon_button("sidebar-vault-open", AppIcon::ArrowRight, "打开密码库")
                            .ml_auto()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.switch_screen(Screen::Passwords);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 渲染侧栏中的一个快捷位置行。
    fn sidebar_location_item(
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
            .h(px(25.))
            .w_full()
            .gap_2()
            .px_3()
            .cursor_pointer()
            .text_sm()
            .text_color(if active {
                cx.theme().sidebar_accent_foreground
            } else {
                cx.theme().sidebar_foreground
            })
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .when(!active, |this| {
                this.hover(|style| style.bg(cx.theme().sidebar_primary))
            })
            .child(location_icon(label).element(14.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label.to_string()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.navigate_to(path.clone(), window, cx);
                cx.notify();
            }))
    }

    /// 渲染侧栏目录树中的一个懒加载节点。
    fn sidebar_tree_item(
        &self,
        index: usize,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let expand_path = row.path.clone();
        let open_path = row.path.clone();
        let active = self.browser.current_display() == row.path.display().to_string();
        h_flex()
            .id(("sidebar-tree", index))
            .role(gpui_kit::Role::Button)
            .aria_label(row.label.clone())
            .h(px(25.))
            .w_full()
            .gap_1()
            .pl(px(6. + row.depth as f32 * 12.))
            .pr_2()
            .cursor_pointer()
            .text_sm()
            .text_color(if active {
                cx.theme().sidebar_accent_foreground
            } else {
                cx.theme().sidebar_foreground
            })
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .when(!active, |this| {
                this.hover(|style| style.bg(cx.theme().sidebar_primary))
            })
            .child(
                div()
                    .id(("tree-expand", index))
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|style| style.bg(cx.theme().secondary))
                    .child(
                        if row.expanded {
                            AppIcon::Down
                        } else {
                            AppIcon::Right
                        }
                        .element(11.),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_tree_path(expand_path.clone());
                        cx.notify();
                    })),
            )
            .child(
                div().text_color(cx.theme().warning_foreground).child(
                    if row.expanded {
                        AppIcon::FolderOpen
                    } else {
                        AppIcon::Folder
                    }
                    .element(14.),
                ),
            )
            .child(
                div()
                    .id(("tree-open", index))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(row.label.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.navigate_to(open_path.clone(), window, cx);
                        cx.notify();
                    })),
            )
    }

    /// 渲染底部状态栏和紧凑任务控件。
    ///
    /// 参考图片的状态栏横跨整个窗口；这里同时保留运行状态、暂停/取消
    /// 和当前目录的总项目数与字节数。
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
            .h(px(28.))
            .flex_shrink_0()
            .w_full()
            .px_3()
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
                    .max_w(px(620.))
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
                        .w(px(110.))
                        .h(px(5.))
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .w(px(110. * ratio))
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
                .child(
                    Button::new("show-progress")
                        .ghost()
                        .xsmall()
                        .icon(AppIcon::List.element(13.))
                        .label("进度")
                        .tooltip("显示任务进度窗口")
                        .on_click(cx.listener(|_, _, window, cx| {
                            cx.defer_in(window, |app, window, cx| {
                                app.open_progress_window(window, cx);
                            });
                            cx.notify();
                        })),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .whitespace_nowrap()
                    .child(if self.screen == Screen::Browser {
                        format!(
                            "总计 {} 个项目 · {}",
                            self.displayed_count(),
                            format_size(total_size)
                        )
                    } else {
                        String::new()
                    }),
            )
            .child(
                div()
                    .pl_3()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .child("v0.1.0"),
            )
    }
}

impl Render for LumarkApp {
    /// 组合标题栏、动作带、导航行、侧栏、当前页面、状态栏和模态层。
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

        if self.active_task.is_none() && self.progress_window.is_some() {
            self.dismiss_progress_window(cx);
        }

        let content = match self.screen {
            Screen::Browser => self.render_browser(window, cx).into_any_element(),
            Screen::Passwords => self.render_passwords(cx).into_any_element(),
            Screen::Settings => self.render_settings(cx).into_any_element(),
        };
        let browser = self.screen == Screen::Browser;

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(self.render_action_band(window, cx))
            .when(browser, |this| {
                this.child(self.render_navigation_bar(window, cx))
            })
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .min_w_0()
                    .items_stretch()
                    .when(browser && self.sidebar_visible, |this| {
                        this.child(self.render_sidebar(window, cx))
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(content),
                    ),
            )
            .child(self.render_status_bar(cx))
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

/// 把模态卡片放入吸收外部点击的遮罩层。
fn dialog_layer(content: impl IntoElement) -> gpui_kit::Div {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(222. / 360., 0.18, 0.08, 0.42))
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, _| {})
        .on_mouse_down(gpui_kit::MouseButton::Right, |_, _, _| {})
        .child(content)
}
