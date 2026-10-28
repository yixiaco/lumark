use super::*;

impl LumarkApp {
    /// 渲染自绘标题栏：当前位置、工作区切换和全局工具入口。
    ///
    /// `TitleBar` 负责 Windows 拖拽区域和系统按钮，因此标题内容保持单行，
    /// 长路径会收缩而不是把工作区入口挤出窗口。
    pub(crate) fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match self.screen {
            Screen::Browser => format!("{} - Lumark", self.browser.current_display()),
            Screen::Passwords => "密码库 - Lumark".to_string(),
            Screen::Tasks => "任务中心 - Lumark".to_string(),
        };
        // 标题栏宽度由窗口决定，超长路径必须在这里先按字符收口：
        // 只依赖文本省略号时弹性布局仍可能把右侧工作区入口挤出窗口。
        let title = truncate_title(&title, 96);
        let title_max = (self.last_window_size.0 - 600.).max(160.);
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
                    .child(brand_mark(24.))
                    .child(
                        div()
                            .min_w_0()
                            .max_w(px(title_max))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .text_sm()
                            .font_semibold()
                            .child(title),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .px_2()
                            .py(px(1.))
                            .rounded(cx.theme().radius / 2.)
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
                    .child(
                        button("title-nav-settings")
                            .ghost()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .occlude()
                            .icon(AppIcon::Setting.element(16.))
                            .label("设置")
                            .on_click(cx.listener(|_, _, window, cx| {
                                // 创建窗口会立即渲染新视图并读取主应用状态，
                                // 必须等当前按钮更新结束后再执行，避免重入。
                                cx.defer_in(window, |this, window, cx| {
                                    this.open_settings_window(window, cx);
                                });
                            })),
                    )
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
        button(id)
            .ghost()
            .rounded(px(6.))
            .cursor_pointer()
            .selected(active)
            .occlude()
            .icon(icon.element(16.))
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.switch_screen(target);
                cx.notify();
            }))
    }
}
