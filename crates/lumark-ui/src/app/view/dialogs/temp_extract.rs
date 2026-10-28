use super::*;

impl LumarkApp {
    /// 渲染拖出/复制归档条目时的临时解压进度弹窗；完成后由后台任务自动关闭。
    pub(crate) fn render_temp_extract_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(dialog) = self.temp_extract.as_ref() else {
            return div().into_any_element();
        };
        card(cx)
            .w(px(420.))
            .p_5()
            .gap_3()
            .child(div().text_sm().font_semibold().child(dialog.title.clone()))
            .child(
                Progress::new("temp-extract-progress")
                    .loading(dialog.progress <= 0.1)
                    .value(dialog.progress)
                    .w_full(),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(dialog.message.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{:.0}%", dialog.progress)),
                    ),
            )
            .into_any_element()
    }
}
