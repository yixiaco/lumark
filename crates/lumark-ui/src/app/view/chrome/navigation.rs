//! 页面标签和路径/搜索导航行。

use super::super::{ArchiveDragItem, path_bar_drop_handler};
use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 渲染密码库和任务中心共享的页头横幅。
    pub(crate) fn render_page_band(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, title, caption) = match self.screen {
            Screen::Passwords => (
                AppIcon::Key,
                tr("密码管理", "Password manager"),
                format!(
                    "{}{} · {}",
                    self.vault.entry_count(),
                    tr(" 条加密记录", " encrypted records"),
                    if self.vault.state() == VaultState::Locked {
                        tr("已锁定", "Locked")
                    } else {
                        tr("已解锁", "Unlocked")
                    }
                ),
            ),
            Screen::Tasks => (
                AppIcon::List,
                tr("任务中心", "Task center"),
                format!(
                    "{}{} · {}{}",
                    self.task_records.len(),
                    tr(" 个任务", " tasks"),
                    self.task_records
                        .iter()
                        .filter(|record| !is_terminal_task_state(record.state))
                        .count(),
                    tr(" 个进行中", " running")
                ),
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
                        button("banner-back-files")
                            .ghost()
                            .small()
                            .rounded(px(6.))
                            .icon(AppIcon::ArrowLeft.element(15.))
                            .label(tr("返回文件", "Back to files"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.switch_screen(Screen::Browser);
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button("banner-tasks", AppIcon::List, tr("任务中心", "Task center"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.switch_screen(Screen::Tasks);
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button(
                            "banner-theme",
                            AppIcon::Tool,
                            tr("切换主题", "Toggle theme"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cycle_theme(window, cx);
                            cx.notify();
                        })),
                    ),
            )
    }

    /// 渲染全宽导航行：浏览按钮、路径栏、搜索栏和视图开关。
    ///
    /// 参考图片把地址栏与搜索框放在同一行，这里按窗口宽度收缩搜索框和
    /// 搜索模式标签，保证 960 像素最小窗口下主操作仍然可见。
    pub(crate) fn render_navigation_bar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // 用可绘制宽度做断点：最大化时 `inner_window_bounds` 返回还原尺寸。
        let logical_width = window.viewport_size().width.as_f32();
        let narrow = logical_width < 1100.;
        let search_active = self.search_results.is_some();
        let mode_label = match self.search_mode {
            SearchMode::Contains => tr("包含", "Contains"),
            SearchMode::Wildcard => tr("通配", "Wildcard"),
            SearchMode::Regex => tr("正则", "Regex"),
        };
        let contains_checked = self.search_mode == SearchMode::Contains;
        let wildcard_checked = self.search_mode == SearchMode::Wildcard;
        let regex_checked = self.search_mode == SearchMode::Regex;
        let category_label = asset_category_label(self.search_category);
        let category_checked = self.search_category;
        let app = cx.entity();
        let app_for_type = app.clone();
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
                                tr("隐藏文件夹栏", "Hide folder pane")
                            } else {
                                tr("显示文件夹栏", "Show folder pane")
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_visible = !this.sidebar_visible;
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button("browser-back", AppIcon::ArrowLeft, tr("后退", "Back"))
                            .on_click(cx.listener(|this, _, window, cx| {
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
                            })),
                    )
                    .child(
                        icon_button(
                            "browser-forward",
                            AppIcon::ArrowRight,
                            tr("前进", "Forward"),
                        )
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
                    .child(
                        icon_button("up", AppIcon::ArrowUp, tr("上一级", "Up")).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.navigate_up(window, cx);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(div().w(px(1.)).h(px(20.)).mx_1().bg(cx.theme().border))
                    .child(
                        icon_button("refresh", AppIcon::Refresh, tr("刷新", "Refresh")).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.refresh();
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        icon_button(
                            "new-folder",
                            AppIcon::FolderPlus,
                            tr("新建文件夹", "New folder"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_new_folder(window, cx);
                            cx.notify();
                        })),
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
                                tr("详细视图", "Details view")
                            } else {
                                tr("平铺视图", "Tile view")
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tile_view = !this.tile_view;
                            this.marquee = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button(
                            "details-toggle-panel",
                            AppIcon::Doc,
                            tr("显示或隐藏详情", "Show or hide details"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_details();
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(150.))
                    .on_drop::<ArchiveDragItem>(path_bar_drop_handler(app.clone()))
                    .child(
                        InputGroup::new("location-group")
                            .w_full()
                            .input(
                                InputGroupInput::new(&self.location_input)
                                    .id("location-input")
                                    .aria_label(tr("当前路径", "Current path")),
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
                                            .cursor(gpui_kit::CursorStyle::PointingHand)
                                            .label(tr("转到", "Go"))
                                            .tooltip(tr(
                                                "打开输入的路径；把归档条目拖到此处可解压到当前文件夹",
                                                "Open entered path; drop archive entries here to extract into the current folder",
                                            ))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.go_location(window, cx);
                                                cx.notify();
                                            })),
                                    ),
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
                            .aria_label(tr("搜索名称或通配符", "Search names or wildcards")),
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
                                    .cursor(gpui_kit::CursorStyle::PointingHand)
                                    .label(tr("搜索", "Search"))
                                    .tooltip(tr("在当前目录搜索", "Search current folder"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_search(cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                button("search-mode")
                    .ghost()
                    .small()
                    .rounded(px(6.))
                    .label(if narrow {
                        tr("高级", "Advanced").to_string()
                    } else {
                        format!("{} · {mode_label}", tr("高级", "Advanced"))
                    })
                    .dropdown_caret(true)
                    .tooltip(tr(
                        "选择搜索匹配方式：包含、通配符、正则",
                        "Choose search mode: contains, wildcard or regex",
                    ))
                    .accessibility_label(tr("高级搜索模式", "Advanced search mode"))
                    .dropdown_menu_with_anchor(
                        gpui_kit::Anchor::TopRight,
                        move |menu, window, _| {
                            let app_contains = app.clone();
                            let app_wildcard = app.clone();
                            let app_regex = app.clone();
                            menu.item(
                                PopupMenuItem::new(tr("包含", "Contains"))
                                    .checked(contains_checked)
                                    .on_click(window.listener_for(
                                        &app_contains,
                                        |app, _: &gpui_kit::ClickEvent, _, cx| {
                                            app.search_mode = SearchMode::Contains;
                                            app.apply_search_mode(cx);
                                            cx.notify();
                                        },
                                    )),
                            )
                            .item(
                                PopupMenuItem::new(tr("通配符", "Wildcard"))
                                    .checked(wildcard_checked)
                                    .on_click(window.listener_for(
                                        &app_wildcard,
                                        |app, _: &gpui_kit::ClickEvent, _, cx| {
                                            app.search_mode = SearchMode::Wildcard;
                                            app.apply_search_mode(cx);
                                            cx.notify();
                                        },
                                    )),
                            )
                            .item(
                                PopupMenuItem::new(tr("正则表达式", "Regex"))
                                    .checked(regex_checked)
                                    .on_click(window.listener_for(
                                        &app_regex,
                                        |app, _: &gpui_kit::ClickEvent, _, cx| {
                                            app.search_mode = SearchMode::Regex;
                                            app.apply_search_mode(cx);
                                            cx.notify();
                                        },
                                    )),
                            )
                        },
                    ),
            )
            .child(
                button("search-type")
                    .ghost()
                    .small()
                    .rounded(px(6.))
                    .label(if narrow {
                        tr("类型", "Type").to_string()
                    } else {
                        format!("{} · {category_label}", tr("类型", "Type"))
                    })
                    .dropdown_caret(true)
                    .tooltip(tr(
                        "按资产类型筛选搜索结果：图片、模型、骨骼、动画等",
                        "Filter results by asset type: images, models, skeletons, animations",
                    ))
                    .accessibility_label(tr("资产类型筛选", "Asset type filter"))
                    .dropdown_menu_with_anchor(
                        gpui_kit::Anchor::TopRight,
                        move |menu, window, _| {
                            let mut menu = menu;
                            for category in std::iter::once(None::<lumark_browser::AssetCategory>)
                                .chain(lumark_browser::AssetCategory::ALL.map(Some))
                            {
                                let app_category = app_for_type.clone();
                                let checked = category_checked == category;
                                menu = menu.item(
                                    PopupMenuItem::new(asset_category_label(category))
                                        .checked(checked)
                                        .on_click(window.listener_for(
                                            &app_category,
                                            move |app,
                                                  _: &gpui_kit::ClickEvent,
                                                  _,
                                                  cx| {
                                                app.apply_search_category(category, cx);
                                                cx.notify();
                                            },
                                        )),
                                );
                            }
                            menu
                        },
                    ),
            )
            .when(search_active, |this| {
                this.child(
                    button("search-clear")
                        .ghost()
                        .small()
                        .rounded(px(6.))
                        .icon(AppIcon::Close.element(14.))
                        .tooltip(tr("清除搜索", "Clear search"))
                        .accessibility_label(tr("清除搜索", "Clear search"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // 清除搜索结果时同时重置资产类型筛选，避免下一次搜索
                            // 仍被上一次的类型条件限制。
                            this.clear_search();
                            cx.notify();
                        })),
                )
            })
    }
}

/// 返回资产类型筛选的界面标签。
fn asset_category_label(category: Option<lumark_browser::AssetCategory>) -> &'static str {
    match category {
        None => tr("全部", "All"),
        Some(lumark_browser::AssetCategory::Image) => tr("图片/纹理", "Images"),
        Some(lumark_browser::AssetCategory::Model) => tr("模型", "Models"),
        Some(lumark_browser::AssetCategory::Skeleton) => tr("骨骼", "Skeletons"),
        Some(lumark_browser::AssetCategory::Animation) => tr("动画", "Animations"),
        Some(lumark_browser::AssetCategory::Audio) => tr("音频", "Audio"),
        Some(lumark_browser::AssetCategory::Text) => tr("文本", "Text"),
        Some(lumark_browser::AssetCategory::Archive) => tr("压缩包", "Archives"),
        Some(lumark_browser::AssetCategory::Other) => tr("其他", "Other"),
    }
}
