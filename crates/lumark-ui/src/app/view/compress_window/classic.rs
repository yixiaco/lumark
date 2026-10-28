//! 压缩窗口经典排版和选项卡入口。

use super::*;
use crate::i18n::tr;

impl CompressWindowView {
    /// 渲染经典模式的六个分段选项卡。
    pub(super) fn render_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected = usize::from(self.tab);
        let view = cx.entity().downgrade();
        let tabs = [
            (AppIcon::Setting, tr("常规", "General")),
            (AppIcon::Key, tr("密码", "Password")),
            (AppIcon::Doc, tr("文件", "Files")),
            (AppIcon::Tool, tr("算法", "Algorithm")),
            (AppIcon::Time, tr("时间", "Time")),
            (AppIcon::Edit, tr("注释", "Comment")),
            (AppIcon::Video, tr("伪装", "Disguise")),
        ];
        let items = tabs
            .iter()
            .enumerate()
            .map(|(index, (icon, label))| {
                let view = view.clone();
                segmented_tab(
                    cx,
                    ("compress-window-tab", index),
                    *icon,
                    label,
                    selected == index,
                )
                .set_position(index + 1, tabs.len())
                .on_click(move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.tab = index as u8;
                        cx.notify();
                    })
                    .ok();
                })
                .into_any_element()
            })
            .collect::<Vec<_>>();

        segmented_tab_bar(cx, "compress-window-tabs", items)
    }

    /// 渲染经典模式的六个选项卡。
    pub(super) fn render_classic(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let tab = self.tab;
        let content = v_flex()
            .id("compress-window-classic-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(match tab {
                0 => self.render_general_tab(cx),
                1 => self.render_password_tab(cx),
                2 => self.render_files_tab(cx),
                3 => self.render_algorithm_tab(cx),
                4 => self.render_time_tab(cx),
                5 => self.render_comment_tab(cx),
                _ => self.render_disguise_tab(cx),
            });
        v_flex()
            .flex_1()
            .min_h_0()
            .child(div().w_full().px_4().pt_3().child(self.render_tabs(cx)))
            .child(content)
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .w_full()
                        .px_4()
                        .pb_1()
                        .text_xs()
                        .text_color(cx.theme().danger_foreground)
                        .child(error),
                )
            })
            .child(self.render_footer(cx, true))
            .into_any_element()
    }
}
