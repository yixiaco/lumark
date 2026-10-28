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
            .when(self.show_save_prompt, |this| {
                let dialog = self.render_save_prompt(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.show_archive_password_prompt, |this| {
                let dialog = self.render_archive_password_prompt(cx);
                this.child(dialog_layer(dialog))
            })
            .when(self.temp_extract.is_some(), |this| {
                let dialog = self.render_temp_extract_dialog(cx);
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
            .when(
                self.password_attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.is_visible()),
                |this| {
                    let dialog = self.render_password_attempt_prompt(cx);
                    this.child(dialog_layer(dialog))
                },
            )
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
            .when(self.show_archive_add_dialog, |this| {
                let dialog = self.render_archive_add_dialog(cx);
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
            .when(self.show_confirm_delete_password, |this| {
                let dialog = self.render_confirm_delete_password(cx);
                this.child(dialog_layer(dialog))
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
