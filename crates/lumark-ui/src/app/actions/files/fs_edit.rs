//! 新建文件夹与重命名的官方对话框动作。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 打开新建文件夹对话框并清空名称输入框。
    pub(crate) fn start_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(&self.browser.location, BrowserLocation::FileSystem(_)) {
            self.status = tr(
                "只能在文件系统目录中新建文件夹",
                "New folders can only be created in the file system",
            )
            .to_string();
            return;
        }
        set_input(&self.new_folder_name, "", window, cx);
        let app = cx.entity();
        let input = self.new_folder_name.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let app_confirm = app.clone();
            let input = input.clone();
            dialog
                .title(tr("新建文件夹", "New folder"))
                .child(Input::new(&input).id("new-folder-name").w_full())
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("new-folder-cancel")
                                .label(tr("取消", "Cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            button("new-folder-create")
                                .primary()
                                .label(tr("创建", "Create"))
                                .on_click({
                                    let app = app_confirm.clone();
                                    move |_, window, cx| {
                                        let ok = app.update(cx, |app, cx| app.create_folder(cx));
                                        if ok {
                                            window.close_dialog(cx);
                                        }
                                    }
                                }),
                        ),
                )
        });
    }

    /// 在文件系统位置或当前归档中创建经过校验的文件夹。
    pub(crate) fn create_folder(&mut self, cx: &mut Context<Self>) -> bool {
        let name = input_value(&self.new_folder_name, cx);
        let Some(parent) = self.current_directory() else {
            self.status = tr(
                "当前不在文件系统目录",
                "Not currently in a file system folder",
            )
            .to_string();
            return false;
        };
        if !valid_file_name(&name) {
            self.status = tr("文件夹名称无效", "Invalid folder name").to_string();
            return false;
        }
        match std::fs::create_dir(parent.join(name.trim())) {
            Ok(()) => {
                self.status = tr("已创建文件夹", "Folder created").to_string();
                self.refresh();
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    /// 打开重命名对话框并预填所选项目名称。
    pub(crate) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.status =
                tr("请先选择要重命名的项目", "Select an item to rename first").to_string();
            return;
        };
        if !matches!(
            &self.browser.location,
            BrowserLocation::FileSystem(_) | BrowserLocation::Archive { .. }
        ) {
            self.status = tr(
                "归档内重命名尚未启用，请先解压",
                "Renaming inside archives is not enabled yet; extract first",
            )
            .to_string();
            return;
        }
        self.rename_target = Some(entry.path.clone());
        set_input(&self.rename_name, &entry.name, window, cx);
        let app = cx.entity();
        let input = self.rename_name.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let app_confirm = app.clone();
            let app_cancel = app.clone();
            let input = input.clone();
            dialog
                .title(tr("重命名", "Rename"))
                .child(Input::new(&input).id("rename-name").w_full())
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("rename-cancel")
                                .label(tr("取消", "Cancel"))
                                .on_click({
                                    let app = app_cancel.clone();
                                    move |_, window, cx| {
                                        app.update(cx, |app, cx| {
                                            app.rename_target = None;
                                            cx.notify();
                                        });
                                        window.close_dialog(cx);
                                    }
                                }),
                        )
                        .child(
                            button("rename-confirm")
                                .primary()
                                .label(tr("重命名", "Rename"))
                                .on_click({
                                    let app = app_confirm.clone();
                                    move |_, window, cx| {
                                        let ok = app.update(cx, |app, cx| app.confirm_rename(cx));
                                        if ok {
                                            window.close_dialog(cx);
                                        }
                                    }
                                }),
                        ),
                )
        });
    }

    /// 校验并应用文件系统重命名或归档条目重命名。
    pub(crate) fn confirm_rename(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.rename_target.clone() else {
            return false;
        };
        let name = input_value(&self.rename_name, cx);
        if !valid_file_name(&name) {
            self.status = tr("名称无效", "Invalid name").to_string();
            return false;
        }
        let destination = target.with_file_name(name.trim());
        if let BrowserLocation::Archive { archive, .. } = &self.browser.location {
            let archive = archive.clone();
            let old = target.to_string_lossy().replace('\\', "/");
            let new = {
                let mut parts: Vec<String> = old.split('/').map(ToString::to_string).collect();
                if let Some(last) = parts.last_mut() {
                    *last = name.trim().to_string();
                }
                parts.join("/")
            };
            let options = EditOptions {
                source_password: self.archive_password.clone(),
                rename: vec![(old, new)],
                keep_backup: self.settings.get().archive_edit_backup,
                ..Default::default()
            };
            self.submit_archive_edit(archive, options, PendingEditAction::Rename);
            return true;
        }
        match std::fs::rename(&target, &destination) {
            Ok(()) => {
                self.rename_target = None;
                self.status = tr("已重命名", "Renamed").to_string();
                self.refresh();
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }
}
