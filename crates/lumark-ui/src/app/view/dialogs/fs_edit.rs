//! 新建文件夹和重命名弹窗。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 收集当前位置中新文件夹的名称。
    pub(crate) fn render_new_folder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, tr("新建文件夹", "New folder"))
            .child(
                Input::new(&self.new_folder_name)
                    .id("new-folder-name")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("new-folder-cancel")
                            .label(tr("取消", "Cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_new_folder = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("new-folder-create")
                            .primary()
                            .label(tr("创建", "Create"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_folder(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 收集所选文件或归档条目的新名称。
    pub(crate) fn render_rename(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, tr("重命名", "Rename"))
            .child(Input::new(&self.rename_name).id("rename-name").w_full())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("rename-cancel")
                            .label(tr("取消", "Cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_rename = false;
                                this.rename_target = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("rename-confirm")
                            .primary()
                            .label(tr("重命名", "Rename"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_rename(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }
}
