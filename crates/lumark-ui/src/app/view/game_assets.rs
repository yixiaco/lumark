//! 游戏资产设置页、游戏侧栏和上下文条。
use super::super::*;
use super::components::{AppIcon, brand_mark, icon_button};
use crate::i18n::tr;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::TitleBar;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use lumark_settings::GameLibraryEntry;

impl LumarkApp {
    /// 渲染游戏设置页：独立编辑窗口显示表单，主窗口只在兼容路径下显示。
    pub(crate) fn render_game_setup(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(setup) = &self.game_setup else {
            return div().into_any_element();
        };
        let mut games = self.settings.get().games;
        games.sort_by(|left, right| right.last_opened.cmp(&left.last_opened));
        let editing = setup.editing_id.is_some();
        let title = if editing {
            tr("编辑游戏", "Edit game")
        } else {
            tr("添加游戏", "Add game")
        };
        let subtitle = if self.game_editor_only {
            tr(
                "配置游戏目录、密钥和映射；保存后回到游戏库。",
                "Configure the game folder, key and mappings; saving returns to the game library.",
            )
        } else {
            tr(
                "选择 UE 游戏目录，浏览 pak/IoStore 容器中的资产。只读，不改动游戏文件。",
                "Choose a UE game folder to browse assets in pak/IoStore containers. Read-only.",
            )
        };
        let page = v_flex()
            .id("game-setup-page")
            .size_full()
            .overflow_y_scrollbar()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(880.))
                    .mx_auto()
                    .p_6()
                    .gap_4()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(38.))
                                    .rounded(cx.theme().radius_lg)
                                    .bg(cx.theme().primary.opacity(0.12))
                                    .text_color(cx.theme().primary)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(AppIcon::Game.element(20.)),
                            )
                            .child(
                                v_flex()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_semibold()
                                            .child(tr("游戏资产", "Game assets")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(subtitle),
                                    ),
                            )
                            .child(
                                button("game-setup-close")
                                    .ghost()
                                    .ml_auto()
                                    .label(if self.game_editor_only {
                                        tr("取消", "Cancel")
                                    } else {
                                        tr("返回文件浏览", "Back to files")
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_game_setup(window, cx);
                                    })),
                            ),
                    )
                    .when(!self.game_editor_only, |this| {
                        this.child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .child(section_label(cx, tr("已保存的游戏", "Saved games")))
                                .when(games.is_empty(), |this| {
                                    this.child(hint_line(
                                        cx,
                                        tr(
                                            "还没有游戏记录。选择下方目录即可开始。",
                                            "No games yet. Choose a folder below to begin.",
                                        ),
                                    ))
                                })
                                .children(
                                    games.iter().enumerate().map(|(index, game)| {
                                        saved_game_row(index, game, false, cx)
                                    }),
                                ),
                        )
                    })
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().popover)
                            .p_4()
                            .child(div().text_sm().font_semibold().child(title.to_string()))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(field_label(cx, tr("游戏目录", "Game folder")))
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div().flex_1().min_w_0().child(
                                                    Input::new(&setup.directory)
                                                        .id("game-directory")
                                                        .w_full(),
                                                ),
                                            )
                                            .child(
                                                button("game-choose-folder")
                                                    .label(tr("选择目录…", "Choose folder..."))
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.choose_game_directory(window, cx);
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(hint_line(
                                        cx,
                                        tr(
                                            "也可以把游戏文件夹拖到这里。",
                                            "You can also drag the game folder here.",
                                        ),
                                    )),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(field_label(cx, tr("游戏名称", "Game name")))
                                    .child(Input::new(&setup.name).id("game-name").w_full()),
                            )
                            .child(detection_summary(setup, cx))
                            .child(key_section(setup, cx))
                            .child(oodle_section(setup, cx))
                            .child(mapping_section(setup, cx))
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_end()
                                    .gap_2()
                                    .child(
                                        button("game-setup-cancel")
                                            .secondary()
                                            .label(tr("取消", "Cancel"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_game_setup(window, cx);
                                            })),
                                    )
                                    .child(
                                        button("game-setup-save")
                                            .primary()
                                            .disabled(!setup.ready())
                                            .label(if self.game_editor_only {
                                                tr("保存", "Save")
                                            } else {
                                                tr("保存并打开", "Save and open")
                                            })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_game_setup(window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .on_drop(
                cx.listener(|this, paths: &gpui_kit::ExternalPaths, window, cx| {
                    if let Some(directory) = paths.0.iter().find(|path| path.is_dir()).cloned() {
                        this.scan_game_directory(directory, window, cx);
                    }
                }),
            );
        page.into_any_element()
    }
}

impl LumarkApp {
    /// 渲染游戏库列表页：主窗口的游戏资产入口。
    ///
    /// 页面只负责“已有游戏列表 + 新增/编辑/打开/移除”这几个决定；游戏配置
    /// 表单在独立编辑窗口中打开，避免列表页被长表单挤占。
    pub(crate) fn render_game_library(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let mut games = self.settings.get().games;
        games.sort_by(|left, right| right.last_opened.cmp(&left.last_opened));
        let active_id = self
            .game_session
            .as_ref()
            .map(|session| session.entry_id.clone());
        v_flex()
            .id("game-library-page")
            .size_full()
            .overflow_y_scrollbar()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(880.))
                    .mx_auto()
                    .p_6()
                    .gap_4()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(38.))
                                    .rounded(cx.theme().radius_lg)
                                    .bg(cx.theme().primary.opacity(0.12))
                                    .text_color(cx.theme().primary)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(AppIcon::Game.element(20.)),
                            )
                            .child(
                                v_flex()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_semibold()
                                            .child(tr("游戏资产", "Game assets")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(tr(
                                                "选择游戏进入容器浏览，或新增一个 UE 游戏目录。只读，不改动游戏文件。",
                                                "Open a game to browse its containers, or add a UE game folder. Read-only.",
                                            )),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .ml_auto()
                                    .gap_2()
                                    .child(
                                        button("game-library-add")
                                            .label(tr("新增游戏", "Add game"))
                                            .icon(AppIcon::Add.element(14.))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_game_setup_new(window, cx);
                                            })),
                                    )
                                    .child(
                                        button("game-library-back")
                                            .ghost()
                                            .label(tr("返回文件浏览", "Back to files"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.exit_game_assets(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(section_label(cx, tr("已保存的游戏", "Saved games")))
                            .when(games.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .w_full()
                                        .gap_1()
                                        .rounded(cx.theme().radius_lg)
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .bg(cx.theme().popover)
                                        .p_4()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .child(tr("还没有游戏记录", "No games yet")),
                                        )
                                        .child(hint_line(
                                            cx,
                                            tr(
                                                "点击“新增游戏”选择 UE 游戏目录，Lumark 会扫描容器和密钥。",
                                                "Choose a UE game folder with Add game; Lumark scans its containers and keys.",
                                            ),
                                        )),
                                )
                            })
                            .children(games.iter().enumerate().map(|(index, game)| {
                                let active = active_id.as_deref() == Some(game.id.as_str());
                                saved_game_row(index, game, active, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 渲染独立游戏编辑窗口：标题栏、设置表单和状态栏。
    pub(crate) fn render_game_editor_window(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .id("game-editor-window")
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
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(brand_mark(22.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_sm()
                                    .font_semibold()
                                    .child(
                                        if self
                                            .game_setup
                                            .as_ref()
                                            .is_some_and(|setup| setup.editing_id.is_some())
                                        {
                                            tr("编辑游戏 - Lumark", "Edit game - Lumark")
                                        } else {
                                            tr("添加游戏 - Lumark", "Add game - Lumark")
                                        },
                                    ),
                            )
                            .child(
                                icon_button(
                                    "game-editor-close",
                                    AppIcon::Close,
                                    tr("关闭编辑窗口", "Close editor"),
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.close_game_setup(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .child(self.render_game_setup(window, cx))
            .child(self.render_status_bar(cx))
            .into_any_element()
    }
}

/// 渲染一行已保存游戏。
fn saved_game_row(
    index: usize,
    game: &GameLibraryEntry,
    active: bool,
    cx: &mut Context<LumarkApp>,
) -> gpui_kit::AnyElement {
    let id = game.id.clone();
    let edit_id = game.id.clone();
    let remove_id = game.id.clone();
    let engine = if game.engine.is_empty() {
        tr("未知版本", "Unknown").to_string()
    } else {
        game.engine.clone()
    };
    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if active {
            cx.theme().primary.opacity(0.35)
        } else {
            cx.theme().border
        })
        .bg(if active {
            cx.theme().primary.opacity(0.06)
        } else {
            cx.theme().background
        })
        .p_2()
        .child(
            div()
                .size(px(28.))
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .text_color(cx.theme().primary)
                .flex()
                .items_center()
                .justify_center()
                .child(AppIcon::Game.element(15.)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(0.))
                .child(div().text_sm().font_semibold().child(game.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(format!("{engine} · {}", game.root.display())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            match game.last_opened.as_deref().and_then(|text| text.get(..10)) {
                                Some(date) => format!("{} {date}", tr("最近打开", "Last opened")),
                                None => tr("尚未打开", "Not opened yet").to_string(),
                            },
                        ),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .child(
                    button(("game-open", index))
                        .ghost()
                        .xsmall()
                        .label(tr("打开", "Open"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_saved_game(&id, window, cx);
                        })),
                )
                .child(
                    button(("game-edit", index))
                        .ghost()
                        .xsmall()
                        .icon(AppIcon::Edit.element(12.))
                        .tooltip(tr("编辑", "Edit"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let entry = this
                                .settings
                                .get()
                                .games
                                .into_iter()
                                .find(|game| game.id == edit_id);
                            if let Some(entry) = entry {
                                this.spawn_game_editor(Some(entry), window, cx);
                            }
                        })),
                )
                .child(
                    button(("game-remove", index))
                        .ghost()
                        .xsmall()
                        .icon(AppIcon::Delete.element(12.))
                        .tooltip(tr("移除游戏", "Remove game"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.remove_saved_game(&remove_id, window, cx);
                        })),
                ),
        )
        .into_any_element()
}

/// 渲染目录扫描结果摘要。
fn detection_summary(setup: &GameSetup, cx: &mut Context<LumarkApp>) -> gpui_kit::AnyElement {
    if setup.busy {
        return h_flex()
            .gap_2()
            .items_center()
            .child(Spinner::new())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("正在扫描游戏目录…", "Scanning game folder...")),
            )
            .into_any_element();
    }
    if let Some(error) = &setup.scan_error {
        return div()
            .w_full()
            .rounded(cx.theme().radius)
            .bg(cx.theme().danger.opacity(0.08))
            .border_1()
            .border_color(cx.theme().danger.opacity(0.35))
            .p_2()
            .text_xs()
            .text_color(StatusTone::Danger.color(cx))
            .child(error.clone())
            .into_any_element();
    }
    let Some(game) = &setup.scanned else {
        return hint_line(
            cx,
            tr(
                "选择目录后会显示引擎版本、容器和加密状态。",
                "Choose a folder to detect the engine version, containers and encryption.",
            ),
        )
        .into_any_element();
    };
    let encrypted = game
        .containers
        .iter()
        .filter(|container| container.encrypted == Some(true))
        .count();
    v_flex()
        .w_full()
        .gap_2()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .p_3()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().text_sm().font_semibold().child(game.name.clone()))
                .child(Tag::secondary().xsmall().child(game.engine.label()))
                .when(encrypted > 0, |this| {
                    this.child(
                        Tag::secondary()
                            .xsmall()
                            .child(format!("{encrypted} {}", tr("个加密容器", "encrypted"))),
                    )
                }),
        )
        .child(info_line(
            cx,
            tr("项目目录", "Project"),
            &game.project_dir.display().to_string(),
        ))
        .child(info_line(
            cx,
            tr("Shipping.exe", "Shipping.exe"),
            &game
                .shipping_exe
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| tr("未找到", "Not found").to_string()),
        ))
        .child(info_line(
            cx,
            tr("容器", "Containers"),
            &format!("{} {}", game.containers.len(), tr("个", "containers")),
        ))
        .when(!game.mapping_candidates.is_empty(), |this| {
            this.child(info_line(
                cx,
                tr("映射候选", "Mapping candidates"),
                &format!(
                    "{} · {}",
                    game.mapping_candidates.len(),
                    game.mapping_candidates
                        .first()
                        .and_then(|path| path.file_name())
                        .and_then(|name| name.to_str())
                        .unwrap_or(".usmap")
                ),
            ))
        })
        .children(game.containers.iter().take(8).map(|container| {
            info_line(
                cx,
                container.kind.label(),
                &format!(
                    "{} · {}{}",
                    container
                        .path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("container"),
                    format_size(container.size),
                    if container.encrypted == Some(true) {
                        tr(" · 加密", " · encrypted")
                    } else {
                        ""
                    },
                ),
            )
        }))
        .when(setup.message.is_some(), |this| {
            this.child(hint_line(cx, setup.message.clone().unwrap_or_default()))
        })
        .into_any_element()
}

/// 渲染 AES 密钥设置区。
fn key_section(setup: &GameSetup, cx: &mut Context<LumarkApp>) -> gpui_kit::AnyElement {
    let state = match &setup.key_state {
        SetupKeyState::None => tr("未设置密钥", "No key set").to_string(),
        SetupKeyState::Extracting => {
            tr("正在分析 Shipping.exe…", "Analyzing Shipping.exe...").to_string()
        }
        SetupKeyState::Found(_) => tr("已提取并验证密钥", "Key extracted and verified").to_string(),
        SetupKeyState::Error(error) => error.clone(),
    };
    v_flex()
        .gap_2()
        .child(field_label(cx, tr("AES 密钥", "AES key")))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&setup.key_input).id("game-aes-key").w_full()),
                )
                .child(
                    button("game-extract-key")
                        .label(tr("从 Shipping.exe 提取", "Extract from Shipping.exe"))
                        .disabled(setup.busy || setup.scanned.is_none())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.extract_setup_key(window, cx);
                        })),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(state),
                )
                .when(setup.key_state.key().is_some(), |this| {
                    this.child(
                        button("game-view-key")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Key.element(12.))
                            .label(tr("查看密钥", "View key"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.view_setup_key(window, cx);
                            })),
                    )
                }),
        )
        .into_any_element()
}

/// 渲染 Oodle 库设置区。
fn oodle_section(setup: &GameSetup, cx: &mut Context<LumarkApp>) -> gpui_kit::AnyElement {
    let path = setup
        .oodle
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| {
            tr(
                "未找到（需要 Oodle 压缩的游戏必须提供）",
                "Not found (required for Oodle-compressed games)",
            )
            .to_string()
        });
    v_flex()
        .gap_2()
        .child(field_label(cx, tr("Oodle 解压库", "Oodle library")))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(path),
                )
                .child(
                    button("game-choose-oodle")
                        .label(tr("选择 Oodle 库…", "Choose Oodle library..."))
                        .disabled(setup.busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_game_oodle(window, cx);
                        })),
                ),
        )
        .into_any_element()
}

/// 渲染 `.usmap` 映射设置区。
fn mapping_section(setup: &GameSetup, cx: &mut Context<LumarkApp>) -> gpui_kit::AnyElement {
    let path = setup
        .mappings
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| {
            tr(
                "未选择（无版本属性需要 .usmap 映射）",
                "Not selected (unversioned properties need .usmap mappings)",
            )
            .to_string()
        });
    v_flex()
        .gap_2()
        .child(field_label(cx, tr("映射文件", "Mappings")))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(path),
                )
                .child(
                    button("game-choose-mappings")
                        .label(tr("选择映射…", "Choose mappings..."))
                        .disabled(setup.busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_game_mappings(window, cx);
                        })),
                ),
        )
        .into_any_element()
}

/// 渲染字段标签。
fn field_label(cx: &gpui_kit::App, label: &'static str) -> gpui_kit::Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// 渲染分组标题。
fn section_label(cx: &gpui_kit::App, label: &'static str) -> gpui_kit::Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// 渲染辅助说明。
fn hint_line(cx: &gpui_kit::App, text: impl Into<String>) -> gpui_kit::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// 渲染“标签：值”的一行。
fn info_line(cx: &gpui_kit::App, label: &str, value: &str) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .child(
            div()
                .w(px(82.))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .whitespace_normal()
                .child(value.to_string()),
        )
}
