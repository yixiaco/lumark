//! 常用位置和目录树合并的文件夹侧栏。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 渲染持久文件侧栏：常用位置、文件夹树、详细信息区和密码库状态。
    ///
    /// 常用位置和磁盘树合并为一个列表，对应参考图片中单一的“文件夹”区域；
    /// 窗口较矮时压缩详细信息区的高度，保证目录树仍有一行以上可见。
    pub(crate) fn render_sidebar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                            .child(tr("文件夹", "Folders")),
                    )
                    .child(
                        h_flex()
                            .ml_auto()
                            .gap_1()
                            .child(
                                icon_button(
                                    "sidebar-refresh-tree",
                                    AppIcon::Refresh,
                                    tr("刷新目录树", "Refresh folder tree"),
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.tree_children.clear();
                                        this.status =
                                            tr("目录树已刷新", "Folder tree refreshed").to_string();
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "sidebar-close",
                                    AppIcon::Close,
                                    tr("隐藏文件夹栏", "Hide folder pane"),
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.sidebar_visible = false;
                                        cx.notify();
                                    },
                                )),
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
            .when(vault_locked, |this| {
                this.child(
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
                                .bg(cx.theme().warning)
                                .text_color(cx.theme().warning_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(AppIcon::Lock.element(14.)),
                        )
                        .child(
                            v_flex()
                                .gap(px(0.))
                                .min_w_0()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .child(tr("密码库已锁定", "Vault is locked")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{}{}",
                                            self.vault.entry_count(),
                                            tr(" 条记录", " records")
                                        )),
                                ),
                        )
                        .child(
                            icon_button(
                                "sidebar-vault-open",
                                AppIcon::ArrowRight,
                                tr("打开密码库", "Open vault"),
                            )
                            .ml_auto()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.switch_screen(Screen::Passwords);
                                cx.notify();
                            })),
                        ),
                )
            })
    }

    /// 渲染侧栏中的一个快捷位置行。
    pub(crate) fn sidebar_location_item(
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
    pub(crate) fn sidebar_tree_item(
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
}
