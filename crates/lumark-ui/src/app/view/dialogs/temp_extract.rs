//! 临时解压进度对话框；内容由官方 Dialog 承载，进度更新时刷新窗口。

use super::*;

impl LumarkApp {
    /// 打开官方 Dialog 展示临时解压进度；没有任务时不做任何事。
    pub(crate) fn open_temp_extract_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.temp_extract.is_none() {
            return;
        }
        let app = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(state) = app.read(cx).temp_extract.as_ref() else {
                return dialog;
            };
            dialog
                .title(state.title.clone())
                .child(
                    Progress::new("temp-extract-progress")
                        .loading(state.progress <= 0.1)
                        .value(state.progress * 100.0)
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
                                .child(state.message.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{:.0}%", state.progress)),
                        ),
                )
        });
    }
}
