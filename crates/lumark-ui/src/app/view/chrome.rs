//! 主窗口框架：自绘标题栏、动作带、导航行、侧栏、状态栏和模态层入口。

use super::super::*;
use super::components::{
    AppIcon, BannerTone, banner_action, banner_gradient, brand_lockup, brand_mark, icon_button,
    location_icon,
};
use gpui_kit::component::input::{
    InputGroup, InputGroupAddon, InputGroupAddonAlignment, InputGroupButton, InputGroupInput,
};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

mod action_band;
mod navigation;
mod sidebar;
mod status_bar;
mod title_bar;

impl Render for LumarkApp {
    /// 组合标题栏、动作带、导航行、侧栏、当前页面、状态栏和模态层。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_details_preview(cx);
        // 双击资产时详情还在后台解析，渲染结束后再打开查看器：窗口创建
        // 不能发生在渲染过程中，否则会和布局/窗口句柄刷新互相影响。
        if self.pending_asset_viewer_ready() {
            cx.defer_in(window, |this, window, cx| {
                this.flush_pending_asset_viewer(window, cx);
            });
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

        // 游戏编辑窗口是独立的小窗口：只保留自己的标题栏和设置表单，
        // 不渲染主窗口的动作带、导航行和侧栏，避免出现两套全局导航。
        if self.game_editor_only {
            return v_flex()
                .size_full()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(self.render_game_editor_window(window, cx))
                .into_any_element();
        }

        let content = if self.game_library {
            self.render_game_library(window, cx).into_any_element()
        } else {
            match self.screen {
                Screen::Browser => self.render_browser(window, cx).into_any_element(),
                Screen::Passwords => self.render_passwords(window, cx).into_any_element(),
                Screen::Tasks => self.render_task_center(cx).into_any_element(),
            }
        };
        // 密码库的整页滚动条向窗口内侧留出空间，避免与 Windows 窗口
        // 缩放边框重叠而无法拖动。
        let content = if self.screen == Screen::Passwords && !self.game_library {
            v_flex()
                .w_full()
                .h_full()
                .min_h_0()
                .pr_3()
                .child(content)
                .into_any_element()
        } else {
            content
        };
        // 游戏库列表页没有文件表导航行和侧栏，保持页面专注。
        let browser = self.screen == Screen::Browser && !self.game_library;
        let game = self.game_session.is_some();
        let show_sidebar = browser && self.sidebar_visible;
        let content_panel = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(content);
        // 侧栏宽度可拖动；内容面板始终占据剩余宽度。
        let main_area = if show_sidebar {
            let sidebar = if game {
                self.render_game_sidebar(window, cx).into_any_element()
            } else {
                self.render_sidebar(window, cx).into_any_element()
            };
            gpui_kit::base::h_resizable("main-split")
                .with_state(&self.sidebar_state)
                .child(
                    gpui_kit::base::resizable_panel()
                        .size(px(self.sidebar_width))
                        .size_range(px(200.)..px(480.))
                        .flex_none()
                        .child(sidebar),
                )
                .child(gpui_kit::base::resizable_panel().child(content_panel))
                .into_any_element()
        } else {
            content_panel.into_any_element()
        };

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
                    .child(main_area),
            )
            .child(self.render_status_bar(cx))
            .into_any_element()
    }
}
