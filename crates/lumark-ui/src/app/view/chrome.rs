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
            Screen::Passwords => self.render_passwords(window, cx).into_any_element(),
            Screen::Tasks => self.render_task_center(cx).into_any_element(),
        };
        // 密码库的整页滚动条向窗口内侧留出空间，避免与 Windows 窗口
        // 缩放边框重叠而无法拖动。
        let content = if self.screen == Screen::Passwords {
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
    }
}
