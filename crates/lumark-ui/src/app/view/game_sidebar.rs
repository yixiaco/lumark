//! 游戏资产侧栏和上下文条。

use super::super::*;
use super::components::{AppIcon, icon_button};
use crate::i18n::tr;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tag::Tag;

impl LumarkApp {
    /// 渲染游戏资产侧栏：游戏卡片、容器列表和详情检查器。
    pub(crate) fn render_game_sidebar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(session) = &self.game_session else {
            return div().into_any_element();
        };
        let height = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .height
            .as_f32();
        let details_max_height = if height < 720. { 170. } else { 200. };
        let key_tag = match session.key_state {
            GameKeyState::NotNeeded => ("未加密", "Not encrypted"),
            GameKeyState::Valid => ("密钥已就绪", "Key ready"),
            GameKeyState::Missing => ("需要密钥", "Key needed"),
            GameKeyState::Invalid => ("密钥无效", "Invalid key"),
        };
        v_flex()
            .id("game-sidebar")
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
                            .child(tr("游戏资产", "Game assets")),
                    )
                    .child(
                        h_flex()
                            .ml_auto()
                            .gap_1()
                            .child(
                                icon_button(
                                    "game-sidebar-add",
                                    AppIcon::Add,
                                    tr("添加游戏", "Add game"),
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.open_game_setup_new(window, cx);
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "game-sidebar-exit",
                                    AppIcon::Close,
                                    tr("退出游戏资产", "Exit game assets"),
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.exit_game_assets(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .child(v_flex().px_2().pt_2().gap_1().when(
                self.open_game_entries().len() > 1,
                |this| {
                    this.child(
                        div()
                            .px_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("打开的游戏", "Open games")),
                    )
                    .children(self.open_game_entries().into_iter().map(
                        |(game_id, name, active)| {
                            let switch_id = game_id.clone();
                            let mut tab = button(format!("game-tab-{game_id}"))
                                .xsmall()
                                .w_full()
                                .label(name)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.switch_game_session(&switch_id, window, cx);
                                }));
                            if active {
                                tab = tab.primary();
                            } else {
                                tab = tab.ghost();
                            }
                            tab
                        },
                    ))
                },
            ))
            .child(
                v_flex()
                    .flex_shrink_0()
                    .m_2()
                    .p_3()
                    .gap_2()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().sidebar_accent)
                    .border_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .size(px(30.))
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().primary.opacity(0.16))
                                    .text_color(cx.theme().primary)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(AppIcon::Game.element(17.)),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap(px(0.))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_semibold()
                                            .whitespace_nowrap()
                                            .child(session.game.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(session.engine_label()),
                                    ),
                            )
                            .child(
                                icon_button(
                                    "game-close-current",
                                    AppIcon::Close,
                                    tr("关闭当前游戏", "Close current game"),
                                )
                                .ml_auto()
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.close_current_game(window, cx);
                                    },
                                )),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_wrap()
                            .child(Tag::secondary().xsmall().child(key_tag.0))
                            .when_some(session.oodle.as_ref(), |this, _| {
                                this.child(
                                    Tag::secondary()
                                        .xsmall()
                                        .child(tr("Oodle 已加载", "Oodle loaded")),
                                )
                            })
                            .when(session.mappings.is_some(), |this| {
                                this.child(
                                    Tag::secondary()
                                        .xsmall()
                                        .child(tr("已配置映射", "Mappings set")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(session.game.project_dir.display().to_string()),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                button("game-sidebar-edit")
                                    .ghost()
                                    .xsmall()
                                    .icon(AppIcon::Edit.element(13.))
                                    .label(tr("编辑", "Edit"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.edit_current_game(window, cx);
                                    })),
                            )
                            .child(
                                button("game-sidebar-files")
                                    .ghost()
                                    .xsmall()
                                    .icon(AppIcon::FolderOpen.element(13.))
                                    .label(tr("文件浏览", "Files"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.exit_game_assets(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .gap(px(1.))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("归档", "Archives")),
                    )
                    .children(session.game.containers.iter().enumerate().map(
                        |(index, container)| {
                            let path = container.path.clone();
                            let name = container
                                .path
                                .file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or("container")
                                .to_string();
                            let encrypted = container.encrypted == Some(true);
                            h_flex()
                                .id(("game-container", index))
                                .h(px(27.))
                                .w_full()
                                .gap_2()
                                .px_2()
                                .rounded(cx.theme().radius)
                                .text_sm()
                                .hover(|style| style.bg(cx.theme().secondary))
                                .child(
                                    div()
                                        .text_color(StatusTone::Warning.color(cx))
                                        .child(AppIcon::FileZip.element(14.)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(name),
                                )
                                .when(encrypted, |this| {
                                    this.child(
                                        div()
                                            .text_color(StatusTone::Warning.color(cx))
                                            .child(AppIcon::FileLock.element(12.)),
                                    )
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_game_container_path(path.clone(), window, cx);
                                }))
                        },
                    )),
            )
            .child(self.render_details(details_max_height, cx))
            .into_any_element()
    }

    /// 渲染游戏资产上下文条：面包屑、密钥/Oodle 状态和返回操作。
    pub(crate) fn render_game_context_bar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(session) = &self.game_session else {
            return div().into_any_element();
        };
        let (breadcrumb, inside_container) = match &self.browser.location {
            BrowserLocation::GameArchives { .. } => (tr("归档列表", "Archives").to_string(), false),
            BrowserLocation::Archive { archive, inner } => {
                let name = archive
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("archive");
                if inner.is_empty() {
                    (name.to_string(), true)
                } else {
                    (format!("{name} / {inner}"), true)
                }
            }
            BrowserLocation::FileSystem(path) => (path.display().to_string(), false),
        };
        let key_tag = match session.key_state {
            GameKeyState::NotNeeded => tr("未加密", "Not encrypted"),
            GameKeyState::Valid => tr("密钥已就绪", "Key ready"),
            GameKeyState::Missing => tr("需要 AES 密钥", "AES key needed"),
            GameKeyState::Invalid => tr("密钥无效", "Invalid key"),
        };
        h_flex()
            .id("game-context-bar")
            .w_full()
            .h(px(32.))
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .items_center()
            .bg(cx.theme().secondary)
            .border_b_1()
            .border_color(cx.theme().border)
            .text_xs()
            .child(AppIcon::Game.element(13.))
            .child(
                div()
                    .font_semibold()
                    .whitespace_nowrap()
                    .child(session.game.name.clone()),
            )
            .child(div().text_color(cx.theme().muted_foreground).child("›"))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(breadcrumb),
            )
            .child(Tag::secondary().xsmall().child(session.engine_label()))
            .child(Tag::secondary().xsmall().child(key_tag))
            .child(
                h_flex()
                    .ml_auto()
                    .gap_1()
                    .child(
                        button("game-context-settings")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Setting.element(13.))
                            .tooltip(tr("游戏设置", "Game settings"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.edit_current_game(window, cx);
                            })),
                    )
                    .when(inside_container, |this| {
                        this.child(
                            button("game-context-back")
                                .ghost()
                                .xsmall()
                                .label(tr("返回归档列表", "Back to archives"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.leave_game_container_to_archives() {
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
                    })
                    .child(
                        button("game-context-exit")
                            .ghost()
                            .xsmall()
                            .label(tr("退出", "Exit"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.exit_game_assets(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}
