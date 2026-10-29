//! 主窗口渐变动作带。

use super::*;
use gpui_kit::component::toolbar::Toolbar;

impl LumarkApp {
    /// 渲染参考经典归档器的大图标渐变动作带。
    ///
    /// 主操作、破坏性操作和任务入口被分隔线分组；窄窗口只隐藏品牌标语，
    /// 不隐藏任何动作，避免功能在笔记本宽度下消失。
    pub(crate) fn render_action_band(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
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
        Toolbar::new("action-band")
            .h(px(if wide { 72. } else { 68. }))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .content(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(banner_action(
                                cx,
                                "compress",
                                AppIcon::Compression,
                                crate::i18n::tr_key("app.compress"),
                                BannerTone::Primary,
                                |this, window, cx| this.open_compress_dialog(window, cx),
                            ))
                            .child(banner_action(
                                cx,
                                "extract",
                                AppIcon::Zip,
                                crate::i18n::tr_key("app.extract"),
                                BannerTone::Success,
                                |this, window, cx| this.open_extract_dialog(window, cx),
                            ))
                            .child(banner_action(
                                cx,
                                "browser-delete",
                                AppIcon::Delete,
                                crate::i18n::tr_key("app.delete"),
                                BannerTone::Danger,
                                |this, window, cx| this.request_delete(window, cx),
                            ))
                            .child(banner_action(
                                cx,
                                "test-archive",
                                AppIcon::Shield,
                                crate::i18n::tr_key("app.test"),
                                BannerTone::Warning,
                                |this, window, cx| this.start_test(window, cx),
                            ))
                            .child(banner_action(
                                cx,
                                "task-center",
                                AppIcon::List,
                                crate::i18n::tr_key("app.tasks"),
                                BannerTone::Neutral,
                                |this, _, cx| {
                                    this.switch_screen(Screen::Tasks);
                                    cx.notify();
                                },
                            )),
                    )
                    .child(h_flex().ml_auto().pr_2().child(brand_lockup(cx, wide))),
            )
            .into_any_element()
    }
}
