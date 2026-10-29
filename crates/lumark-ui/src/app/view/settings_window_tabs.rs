//! 设置窗口的压缩、文件关联和密码库选项卡。
use super::super::*;
use super::components::{AppIcon, choice_select, hint_text};
use super::settings_window::{
    SettingsWindowView, settings_card, settings_divider, settings_row, settings_section,
};
use crate::i18n::tr;
use gpui_kit::component::checkbox::Checkbox;

impl SettingsWindowView {
    /// 渲染“压缩”选项卡：格式、预设、覆盖策略和归档备份。
    pub(super) fn render_compression_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                settings_card(cx)
                    .p_4()
                    .gap_3()
                    .child(settings_section(
                        cx,
                        AppIcon::Compression,
                        tr("压缩与解压", "Compression"),
                        Some(tr("新建任务会继承这些默认值", "New tasks inherit these defaults").to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        tr("默认格式", "Default format"),
                        tr("ZIP 兼容性最好，7Z 适合更小体积。", "ZIP is the most compatible; 7Z is usually smaller."),
                        choice_select(
                            "settings-format-select",
                            &self.format_select,
                            160.,
                            tr("默认格式", "Default format"),
                        ),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("压缩预设", "Compression preset"),
                        tr("控制默认压缩级别、字典和固实策略。", "Controls default level, dictionary and solid mode."),
                        choice_select(
                            "settings-preset-select",
                            &self.preset_select,
                            160.,
                            tr("压缩预设", "Compression preset"),
                        ),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("覆盖策略", "Overwrite policy"),
                        tr("同名文件默认询问，也可设为覆盖、跳过或重命名。", "Ask by default for existing files, or overwrite, skip or rename."),
                        choice_select(
                            "settings-overwrite-select",
                            &self.overwrite_select,
                            160.,
                            tr("覆盖策略", "Overwrite policy"),
                        ),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("归档编辑备份", "Archive edit backup"),
                        tr("修改归档前保留上一版本，便于恢复。", "Keep the previous version before editing for recovery."),
                        button("settings-toggle-archive-backup")
                            .label(if settings.archive_edit_backup {
                                tr("已开启", "On")
                            } else {
                                tr("已关闭", "Off")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.toggle_archive_backup();
                                    cx.notify();
                                });
                            })),
                    )),
            )
            .child(
                settings_card(cx)
                    .p_4()
                    .gap_3()
                    .child(settings_section(
                        cx,
                        AppIcon::Video,
                        tr("伪装压缩", "Disguise"),
                        Some(tr("把归档输出包装成可播放/查看的媒体外观", "Wrap archive output as playable/viewable media").to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        tr("默认启用", "Enable by default"),
                        tr("新建压缩窗口默认勾选伪装；不会影响已提交的任务。", "Enable disguise in new compression windows; does not affect submitted tasks."),
                        button("settings-toggle-disguise")
                            .label(if settings.disguise_enabled {
                                tr("已开启", "On")
                            } else {
                                tr("已关闭", "Off")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    let next = !app.settings.get().disguise_enabled;
                                    app.set_disguise_enabled(next);
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("默认面具", "Default mask"),
                        tr("视频/图片使用内置面具；自定义文件跟随其扩展名。", "Use built-in video/image masks; custom files follow their extension."),
                        choice_select(
                            "settings-disguise-select",
                            &self.disguise_select,
                            160.,
                            tr("默认伪装面具", "Default disguise mask"),
                        ),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("自定义面具", "Custom mask"),
                        tr("仅“自定义文件”时使用；伪装不提供加密。", "Used only for Custom; disguise does not encrypt."),
                        button("settings-disguise-choose")
                            .label(tr("选择文件…", "Choose file..."))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_disguise_mask(window, cx);
                            })),
                    ))
                    .when_some(settings.disguise_custom_mask.clone(), |this, path| {
                        this.child(hint_text(cx, format!("{}: {}", tr("当前自定义面具", "Current custom mask"), path.display())))
                    }),
            )
            .child(
                settings_card(cx).p_4().gap_2().child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            tr("密码、固实压缩、分卷和 ZipCrypto 等高级参数仍在新建压缩窗口中填写。", "Password, solid compression, volumes and ZipCrypto are configured in the new-compression window."),
                        ),
                ),
            )
    }

    /// 选择自定义面具文件，并写入设置。
    fn choose_disguise_mask(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("选择伪装面具文件", "Choose disguise mask").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |view, window, cx| {
                    view.update_app(cx, |app, cx| {
                        app.set_disguise_custom_mask(path);
                        cx.notify();
                    });
                    view.sync_compression_selects(window, cx);
                });
            }
        })
        .detach();
    }

    /// 渲染“文件关联”选项卡：格式勾选、右键菜单和默认应用入口。
    pub(super) fn render_association_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.file_association_count;
        let total = platform::shell::ARCHIVE_EXTENSIONS.len();
        let selected_count = self.selected_formats.len();
        let description = if count == 0 {
            tr("未注册任何格式。勾选格式并应用后，再到 Windows 设置中确认默认应用。", "No formats registered. Select formats and apply, then confirm default apps in Windows Settings.").to_string()
        } else {
            format!(
                "{}{count}/{total}{}",
                tr("已注册 ", "Registered "),
                tr(
                    " 种格式；默认应用由 Windows 设置确认。",
                    " formats; default apps are confirmed in Windows Settings."
                )
            )
        };
        let checkboxes = platform::shell::ARCHIVE_EXTENSIONS
            .iter()
            .map(|extension| {
                let checked = self.selected_formats.contains(*extension);
                div().w(px(96.)).child(
                    Checkbox::new(format!("settings-format-{extension}"))
                        .label(extension.trim_start_matches('.').to_ascii_uppercase())
                        .checked(checked)
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            if *checked {
                                this.selected_formats.insert((*extension).to_string());
                            } else {
                                this.selected_formats.remove(*extension);
                            }
                            cx.notify();
                        })),
                )
            })
            .collect::<Vec<_>>();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                settings_card(cx)
                    .p_4()
                    .gap_3()
                    .child(settings_section(
                        cx,
                        AppIcon::FileZip,
                        tr("Windows 集成", "Windows integration"),
                        Some(
                            tr(
                                "按当前用户注册，无需管理员权限",
                                "Register for the current user; no admin required",
                            )
                            .to_string(),
                        ),
                    ))
                    .child(settings_row(
                        cx,
                        tr("右键菜单", "Context menu"),
                        tr(
                            "在文件、文件夹和目录背景中添加 Lumark 命令。",
                            "Add Lumark commands to files, folders and directory backgrounds.",
                        ),
                        h_flex()
                            .gap_2()
                            .child(
                                button("settings-register-shell")
                                    .icon(AppIcon::Check.element(15.))
                                    .label(tr("注册", "Register"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.update_app(cx, |app, cx| {
                                            app.register_shell_menu();
                                            cx.notify();
                                        });
                                    })),
                            )
                            .child(
                                button("settings-unregister-shell")
                                    .icon(AppIcon::Close.element(15.))
                                    .label(tr("移除", "Remove"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.update_app(cx, |app, cx| {
                                            app.unregister_shell_menu();
                                            cx.notify();
                                        });
                                    })),
                            ),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("文件关联状态", "File association status"),
                        &description,
                        h_flex()
                            .gap_2()
                            .child(
                                button("settings-unregister-files")
                                    .icon(AppIcon::Close.element(15.))
                                    .label(tr("移除全部", "Remove all"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.update_app(cx, |app, cx| {
                                            app.unregister_file_associations();
                                            cx.notify();
                                        });
                                        this.selected_formats.clear();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("settings-default-apps")
                                    .icon(AppIcon::Tool.element(15.))
                                    .label(tr("Windows 默认应用", "Windows default apps"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.update_app(cx, |app, cx| {
                                            app.open_default_apps_page();
                                            cx.notify();
                                        });
                                    })),
                            ),
                    )),
            )
            .child(
                settings_card(cx)
                    .p_4()
                    .gap_3()
                    .child(settings_section(
                        cx,
                        AppIcon::Zip,
                        tr("关联格式", "Associated formats"),
                        Some(
                            tr(
                                "应用后会打开 Windows 设置，由系统确认默认应用",
                                "Apply opens Windows Settings to confirm default apps",
                            )
                            .to_string(),
                        ),
                    ))
                    .child(h_flex().w_full().flex_wrap().gap_2().children(checkboxes))
                    .child(settings_divider(cx))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                button("settings-select-all-formats")
                                    .label(tr("全选", "Select all"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.selected_formats = platform::shell::ARCHIVE_EXTENSIONS
                                            .iter()
                                            .map(|extension| extension.to_string())
                                            .collect();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("settings-clear-formats")
                                    .label(tr("全不选", "Clear all"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.selected_formats.clear();
                                        cx.notify();
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                button("settings-apply-formats")
                                    .primary()
                                    .icon(AppIcon::Check.element(15.))
                                    .label(format!(
                                        "{}{selected_count})",
                                        tr("应用关联（", "Apply associations (")
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let formats = platform::shell::ARCHIVE_EXTENSIONS
                                            .iter()
                                            .filter(|extension| {
                                                this.selected_formats.contains(**extension)
                                            })
                                            .map(|extension| extension.to_string())
                                            .collect::<Vec<_>>();
                                        this.update_app(cx, move |app, cx| {
                                            app.apply_file_associations(formats);
                                            cx.notify();
                                        });
                                    })),
                            ),
                    ),
            )
    }

    /// 渲染“密码库”选项卡：状态、位置和入口。
    pub(super) fn render_vault_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state_label = match self.vault_state {
            VaultState::Unlocked => tr("已解锁", "Unlocked"),
            VaultState::Locked => tr("已锁定", "Locked"),
            VaultState::Uninitialized => tr("尚未创建", "Not created"),
        };
        let master_label = if self.vault_has_master {
            tr("已设置", "Set")
        } else {
            tr("未设置", "Not set")
        };
        let path = self.vault_path.display().to_string();
        let error = self.vault_error.clone();
        v_flex()
            .w_full()
            .gap_3()
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .text_sm()
                        .child(format!(
                            "{}{error}{}",
                            tr("密码库读取失败：", "Vault load failed: "),
                            tr("。当前使用临时密码库，请不要写入重要密码。", ". Using a temporary vault; do not store important passwords.")
                        )),
                )
            })
            .child(
                settings_card(cx)
                    .p_4()
                    .gap_3()
                    .child(settings_section(
                        cx,
                        AppIcon::Key,
                        tr("本地加密密码库", "Local encrypted vault"),
                        Some(tr("记录只保存在当前计算机", "Records stay on this computer").to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        tr("密码库状态", "Vault status"),
                        &format!("{state_label} · {}{master_label}", tr("主密码", "Master password ")),
                        button("settings-manage-passwords")
                            .icon(AppIcon::Key.element(15.))
                            .label(tr("管理密码", "Manage passwords"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| {
                                    app.switch_screen(Screen::Passwords);
                                    app.activate_main_window(cx);
                                    cx.notify();
                                });
                                window.remove_window();
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("密码库位置", "Vault location"),
                        &path,
                        h_flex().gap_2().child(
                            button("settings-open-vault-location")
                                .icon(AppIcon::FolderOpen.element(15.))
                                .label(tr("更改位置", "Change location"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_vault_location_editor(window, cx);
                                })),
                        ),
                    )),
            )
            .child(
                settings_card(cx).p_4().gap_2().child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            tr("密码库使用 XChaCha20-Poly1305 加密；未设置主密码时由与当前计算机绑定的 Windows DPAPI 包裹。设置主密码后会移除 DPAPI 包裹，忘记主密码无法恢复。", "The vault uses XChaCha20-Poly1305; without a master password it is wrapped by machine-bound Windows DPAPI. Setting a master password removes DPAPI wrapping; a forgotten master password cannot be recovered."),
                        ),
                ),
            )
    }

    /// 打开密码库位置编辑模态层，并预填当前文件夹。
    fn open_vault_location_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self
            .vault_path
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        set_input(&self.vault_location_input, &folder, window, cx);
        self.vault_location_error = None;
        self.vault_location_open = true;
        cx.notify();
    }

    /// 渲染密码库位置模态层。
    pub(super) fn render_vault_location_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.vault_path.display().to_string();
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().overlay)
            .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, _| {})
            .on_mouse_down(gpui_kit::MouseButton::Right, |_, _, _| {})
            .child(
                v_flex()
                    .w(px(540.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .shadow_lg()
                    .p_5()
                    .gap_3()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(AppIcon::FolderOpen.element(18.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_base()
                                    .font_semibold()
                                    .child(tr("更改密码库位置", "Change vault location")),
                            )
                            .child(
                                button("settings-vault-location-close")
                                    .ghost()
                                    .small()
                                    .icon(AppIcon::Close.element(14.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.vault_location_open = false;
                                        this.vault_location_error = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}{current}", tr("当前密码库文件：", "Current vault file: "))),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().font_semibold().child(tr("保存文件夹", "Save folder")))
                            .child(
                                Input::new(&self.vault_location_input)
                                    .id("settings-vault-location-input")
                                    .w_full(),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("密码库会保存为“文件夹\\vault.dat”，迁移成功后删除原位置文件。", "The vault will be saved as \"folder\\vault.dat\"; the old file is removed after migration.")),
                    )
                    .when_some(self.vault_location_error.clone(), |this, error| {
                        this.child(
                            div()
                                .w_full()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(cx.theme().danger.opacity(0.35))
                                .bg(cx.theme().danger.opacity(0.14))
                                .px_3()
                                .py_2()
                                .text_xs()
                                .text_color(cx.theme().danger_foreground)
                                .child(error),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .justify_end()
                            .child(
                                button("settings-vault-location-browse")
                                    .ghost()
                                    .icon(AppIcon::FolderOpen.element(15.))
                                    .label(tr("浏览…", "Browse..."))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browse_vault_location(cx);
                                    })),
                            )
                            .child(
                                button("settings-vault-location-cancel")
                                    .ghost()
                                    .label(tr("取消", "Cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.vault_location_open = false;
                                        this.vault_location_error = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("settings-vault-location-move")
                                    .primary()
                                    .label(tr("迁移密码库", "Move vault"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.move_vault_from_settings(cx);
                                    })),
                            ),
                    ),
            )
    }

    /// 打开系统文件夹选择器并填入密码库位置输入框。
    fn browse_vault_location(&mut self, cx: &mut Context<Self>) {
        self.vault_location_error = None;
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("选择密码库保存文件夹", "Choose vault folder").into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(directory) = paths.into_iter().next() {
                    let _ = this.update_in(cx, |view, window, cx| {
                        set_input(
                            &view.vault_location_input,
                            directory.to_string_lossy().as_ref(),
                            window,
                            cx,
                        );
                        view.vault_location_error = None;
                        cx.notify();
                    });
                }
            }
            Ok(Ok(None)) => {
                let _ = this.update(cx, |view, cx| {
                    view.vault_location_error = None;
                    cx.notify();
                });
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |view, cx| {
                    view.vault_location_error = Some(format!(
                        "{}{error}",
                        tr("无法打开文件夹选择器：", "Cannot open folder picker: ")
                    ));
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = this.update(cx, |view, cx| {
                    view.vault_location_error = Some(format!(
                        "{}{error}",
                        tr("文件夹选择器已关闭：", "Folder picker closed: ")
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 把输入文件夹交给主应用迁移密码库，并刷新设置窗口。
    fn move_vault_from_settings(&mut self, cx: &mut Context<Self>) {
        let value = input_value(&self.vault_location_input, cx);
        let directory = value.trim();
        if directory.is_empty() {
            self.vault_location_error =
                Some(tr("请输入密码库保存文件夹", "Enter a vault folder").to_string());
            cx.notify();
            return;
        }
        let directory = PathBuf::from(directory);
        match self
            .app
            .update(cx, |app, cx| app.move_vault_to(directory, cx))
        {
            Ok(()) => {
                self.vault_location_open = false;
                self.vault_location_error = None;
                self.refresh(cx);
                cx.notify();
            }
            Err(error) => {
                self.vault_location_error = Some(error);
                cx.notify();
            }
        }
    }
}
