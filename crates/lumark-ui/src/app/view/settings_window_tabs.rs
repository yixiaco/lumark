//! 设置窗口的压缩、文件关联和密码库选项卡。
use super::super::*;
use super::components::AppIcon;
use super::settings_window::{
    SettingsWindowView, settings_card, settings_divider, settings_row, settings_section,
};

impl SettingsWindowView {
    /// 渲染“压缩”选项卡：格式、预设、覆盖策略和归档备份。
    pub(super) fn render_compression_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        let preset_label = match settings.default_preset {
            SettingsPreset::Speed => "速度优先",
            SettingsPreset::Balanced => "平衡",
            SettingsPreset::Size => "体积优先",
        };
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
                        "压缩与解压",
                        Some("新建任务会继承这些默认值".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "默认格式",
                        "ZIP 兼容性最好，7Z 适合更小体积。",
                        button("settings-cycle-format")
                            .label(settings.default_format.to_ascii_uppercase())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.cycle_default_format();
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        "压缩预设",
                        "控制默认压缩级别、字典和固实策略。",
                        button("settings-cycle-preset")
                            .label(preset_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.cycle_default_preset();
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        "覆盖策略",
                        "同名文件默认询问，也可设为覆盖、跳过或重命名。",
                        button("settings-cycle-overwrite")
                            .label(match settings.overwrite_policy {
                                lumark_settings::OverwritePolicy::Ask => "询问",
                                lumark_settings::OverwritePolicy::Overwrite => "覆盖",
                                lumark_settings::OverwritePolicy::Skip => "跳过",
                                lumark_settings::OverwritePolicy::Rename => "重命名",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.cycle_overwrite_policy();
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        "归档编辑备份",
                        "修改归档前保留上一版本，便于恢复。",
                        button("settings-toggle-archive-backup")
                            .label(if settings.archive_edit_backup {
                                "已开启"
                            } else {
                                "已关闭"
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
                settings_card(cx).p_4().gap_2().child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "密码、固实压缩、分卷和 ZipCrypto 等高级参数仍在新建压缩窗口中填写。",
                        ),
                ),
            )
    }

    /// 渲染“文件关联”选项卡：右键菜单、压缩格式注册和默认应用入口。
    pub(super) fn render_association_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.file_association_count;
        let total = platform::shell::ARCHIVE_EXTENSIONS.len();
        let description = if count == 0 {
            format!("注册 ZIP、7Z、RAR 等 {total} 种压缩格式，默认应用由 Windows 确认。")
        } else if count < total {
            format!("已注册 {count}/{total} 种格式，重新关联可补齐缺失项。")
        } else {
            format!("已注册 {total} 种格式，默认打开方式由 Windows 设置管理。")
        };
        let action_label = if count == 0 {
            "关联文件"
        } else if count < total {
            "修复关联"
        } else {
            "重新关联"
        };
        let formats = platform::shell::ARCHIVE_EXTENSIONS
            .iter()
            .map(|extension| extension.trim_start_matches('.').to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("、");
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
                        "Windows 集成",
                        Some("按当前用户注册，无需管理员权限".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "右键菜单",
                        "在文件、文件夹和目录背景中添加 Lumark 命令。",
                        h_flex()
                            .gap_2()
                            .child(
                                button("settings-register-shell")
                                    .icon(AppIcon::Check.element(15.))
                                    .label("注册")
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
                                    .label("移除")
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
                        "文件关联",
                        &description,
                        h_flex()
                            .gap_2()
                            .child(
                                button("settings-register-files")
                                    .icon(AppIcon::FileZip.element(15.))
                                    .label(action_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.update_app(cx, |app, cx| {
                                            app.register_file_associations();
                                            cx.notify();
                                        });
                                    })),
                            )
                            .child(
                                button("settings-unregister-files")
                                    .icon(AppIcon::Close.element(15.))
                                    .label("移除关联")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.update_app(cx, |app, cx| {
                                            app.unregister_file_associations();
                                            cx.notify();
                                        });
                                    })),
                            )
                            .child(
                                button("settings-default-apps")
                                    .icon(AppIcon::Tool.element(15.))
                                    .label("默认应用")
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
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .child(format!("支持的压缩格式（{total}）")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(formats),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "Windows 10/11 不允许程序静默接管默认应用；注册后请在系统设置中确认。",
                            ),
                    ),
            )
    }

    /// 渲染“密码库”选项卡：状态、位置和入口。
    pub(super) fn render_vault_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state_label = match self.vault_state {
            VaultState::Unlocked => "已解锁",
            VaultState::Locked => "已锁定",
            VaultState::Uninitialized => "尚未创建",
        };
        let master_label = if self.vault_has_master {
            "已设置"
        } else {
            "未设置"
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
                            "密码库读取失败：{error}。当前使用临时密码库，请不要写入重要密码。"
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
                        "本地加密密码库",
                        Some("记录只保存在当前计算机".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "密码库状态",
                        &format!("{state_label} · 主密码{master_label}"),
                        button("settings-manage-passwords")
                            .icon(AppIcon::Key.element(15.))
                            .label("管理密码")
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
                        "密码库位置",
                        &path,
                        h_flex().gap_2().child(
                            button("settings-open-vault-location")
                                .icon(AppIcon::FolderOpen.element(15.))
                                .label("更改位置")
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
                            "密码库使用 XChaCha20-Poly1305 加密；未设置主密码时由与当前计算机绑定的 Windows DPAPI 包裹。设置主密码后会移除 DPAPI 包裹，忘记主密码无法恢复。",
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
            .bg(hsla(222. / 360., 0.18, 0.08, 0.42))
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
                                    .child("更改密码库位置"),
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
                            .child(format!("当前密码库文件：{current}")),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().font_semibold().child("保存文件夹"))
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
                            .child("密码库会保存为“文件夹\\vault.dat”，迁移成功后删除原位置文件。"),
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
                                    .label("浏览…")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browse_vault_location(cx);
                                    })),
                            )
                            .child(
                                button("settings-vault-location-cancel")
                                    .ghost()
                                    .label("取消")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.vault_location_open = false;
                                        this.vault_location_error = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("settings-vault-location-move")
                                    .primary()
                                    .label("迁移密码库")
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
            prompt: Some("选择密码库保存文件夹".into()),
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
                    view.vault_location_error = Some(format!("无法打开文件夹选择器：{error}"));
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = this.update(cx, |view, cx| {
                    view.vault_location_error = Some(format!("文件夹选择器已关闭：{error}"));
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
            self.vault_location_error = Some("请输入密码库保存文件夹".to_string());
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
