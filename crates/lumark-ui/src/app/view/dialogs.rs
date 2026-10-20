use super::super::*;
use super::components::AppIcon;

impl LumarkApp {
    /// Renders the quick-compression dialog with all advanced options grouped
    /// into tabs. Secrets are read only when the user starts the task.
    pub(crate) fn render_compress_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.compress_tab;
        let format_label = self.compress_format.label();
        let preset_label = match self.compress_preset {
            SettingsPreset::Speed => "速度优先",
            SettingsPreset::Balanced => "平衡",
            SettingsPreset::Size => "体积优先",
        };
        centered_modal(cx, "压缩")
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "将 {} 个项目压缩",
                                self.browser.selection.count().max(1)
                            )),
                    )
                    .child(
                        Button::new("compress-format")
                            .secondary()
                            .small()
                            .icon(AppIcon::Compression.element(14.))
                            .label(format_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.compress_format = next_writable_format(this.compress_format);
                                let current = PathBuf::from(input_value(&this.compress_output, cx));
                                let replacement =
                                    replace_archive_extension(&current, this.compress_format);
                                set_input(
                                    &this.compress_output,
                                    replacement.to_string_lossy().as_ref(),
                                    window,
                                    cx,
                                );
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("compress-preset")
                            .secondary()
                            .small()
                            .icon(AppIcon::Tool.element(14.))
                            .label(preset_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.compress_preset = next_preset(this.compress_preset);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .p_1()
                    .rounded_md()
                    .bg(cx.theme().muted)
                    .children(
                        ["常规", "密码", "文件", "算法", "时间", "注释"]
                            .into_iter()
                            .enumerate()
                            .map(|(index, label)| {
                                Button::new(format!("compress-tab-{index}"))
                                    .small()
                                    .label(label)
                                    .when(index as u8 == tab, |this| this.primary())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.compress_tab = index as u8;
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .when(tab == 0, |this| {
                this.child(
                    Input::new(&self.compress_output)
                        .id("compress-output")
                        .w_full(),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("compress-solid")
                                .small()
                                .label(if self.compress_solid {
                                    "固实压缩：开"
                                } else {
                                    "固实压缩：关"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.compress_solid = !this.compress_solid;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("compress-header")
                                .small()
                                .label(if self.compress_encrypt_header {
                                    "加密文件名：开"
                                } else {
                                    "加密文件名：关"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.compress_encrypt_header = !this.compress_encrypt_header;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    Input::new(&self.compress_split)
                        .id("compress-split")
                        .w_full(),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("compress-test-after")
                                .small()
                                .label(if self.compress_test_after {
                                    "完成后测试：开"
                                } else {
                                    "完成后测试：关"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.compress_test_after = !this.compress_test_after;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("compress-delete-source")
                                .small()
                                .label(if self.compress_delete_source {
                                    "删除源文件：开"
                                } else {
                                    "删除源文件：关"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.compress_delete_source = !this.compress_delete_source;
                                    cx.notify();
                                })),
                        ),
                )
                .when(self.compress_delete_source, |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child("删除源文件仅在归档成功写入并通过测试（若启用）后执行。"),
                    )
                })
            })
            .when(tab == 1, |this| {
                this.child(
                    Input::new(&self.compress_password)
                        .id("compress-password")
                        .w_full(),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("ZIP/7Z 使用 AES-256；密码只保存在当前任务内存或加密密码库。"),
                )
                .when(self.compress_format == ArchiveFormat::Zip, |this| {
                    this.child(
                        Button::new("compress-zipcrypto")
                            .small()
                            .label(if self.compress_zipcrypto {
                                "ZIP 加密：ZipCrypto（兼容）"
                            } else {
                                "ZIP 加密：AES-256"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.compress_zipcrypto = !this.compress_zipcrypto;
                                cx.notify();
                            })),
                    )
                    .when(self.compress_zipcrypto, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child("ZipCrypto 不安全，只应在旧软件无法读取 AES ZIP 时启用。"),
                        )
                    })
                })
            })
            .when(tab == 2, |this| {
                this.child(
                    Input::new(&self.compress_exclude)
                        .id("compress-exclude")
                        .w_full(),
                )
                .child(
                    Input::new(&self.compress_store)
                        .id("compress-store")
                        .w_full(),
                )
                .child(
                    Button::new("compress-path-mode")
                        .small()
                        .label(match self.compress_path_mode {
                            PathMode::Relative => "路径：保留相对路径",
                            PathMode::NoRoot => "路径：不保留顶层目录",
                            PathMode::Flatten => "路径：平铺文件",
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.compress_path_mode = match this.compress_path_mode {
                                PathMode::Relative => PathMode::NoRoot,
                                PathMode::NoRoot => PathMode::Flatten,
                                PathMode::Flatten => PathMode::Relative,
                            };
                            cx.notify();
                        })),
                )
            })
            .when(tab == 3, |this| {
                this.child(format!(
                    "压缩级别：{}；固实：{}；文件头加密：{}",
                    level_for_preset(self.compress_preset),
                    if self.compress_solid { "开" } else { "关" },
                    if self.compress_encrypt_header {
                        "开"
                    } else {
                        "关"
                    }
                ))
                .child(
                    Input::new(&self.compress_dictionary)
                        .id("compress-dictionary")
                        .w_full(),
                )
                .child(
                    Input::new(&self.compress_threads)
                        .id("compress-threads")
                        .w_full(),
                )
                .child("7Z 当前使用 LZMA2；字典和线程参数会传入压缩后端。")
            })
            .when(tab == 4, |this| {
                this.child(
                    Input::new(&self.compress_after)
                        .id("compress-after")
                        .w_full(),
                )
                .child(
                    Input::new(&self.compress_before)
                        .id("compress-before")
                        .w_full(),
                )
                .child("只纳入修改时间落在区间内的文件；留空表示不限。")
            })
            .when(tab == 5, |this| {
                this.child(
                    Input::new(&self.compress_comment)
                        .id("compress-comment")
                        .w_full(),
                )
                .child("TAR 不支持注释；ZIP 注释会写入归档。")
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("compress-cancel")
                            .ghost()
                            .icon(AppIcon::Close.element(15.))
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_compress = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("compress-start")
                            .primary()
                            .icon(AppIcon::Compression.element(16.))
                            .label("立即压缩")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_compress(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Renders the extraction target, optional manual password, and
    /// selection-only control.
    pub(crate) fn render_extract_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "解压")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("记录密码会自动尝试，失败后可手动输入"),
            )
            .child(
                Input::new(&self.extract_output)
                    .id("extract-output")
                    .w_full(),
            )
            .child(
                Input::new(&self.extract_password)
                    .id("extract-password")
                    .w_full(),
            )
            .child(
                Button::new("extract-selection-only")
                    .small()
                    .label(if self.extract_selection_only {
                        "仅解压选中条目：开"
                    } else {
                        "仅解压选中条目：关"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.extract_selection_only = !this.extract_selection_only;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("extract-cancel")
                            .ghost()
                            .icon(AppIcon::Close.element(15.))
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_extract = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("extract-start")
                            .primary()
                            .icon(AppIcon::Zip.element(16.))
                            .label("开始解压")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_extract(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Offers to persist a password that successfully opened an archive.
    pub(crate) fn render_save_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "保存解压密码")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("密码将写入本地加密密码库，下次可直接用于同类压缩包。"),
            )
            .child(Input::new(&self.password_label).id("save-label").w_full())
            .child(Input::new(&self.password_value).id("save-value").w_full())
            .child(
                Input::new(&self.password_confirm)
                    .id("save-confirm-value")
                    .w_full(),
            )
            .child(
                Input::new(&self.password_pattern)
                    .id("save-pattern")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-dismiss")
                            .ghost()
                            .label("不保存")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_save_prompt = false;
                                this.pending_save = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("save-confirm")
                            .primary()
                            .icon(AppIcon::Save.element(16.))
                            .label("保存密码")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_password(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Prompts for the vault master password before automatic password
    /// attempts can begin.
    pub(crate) fn render_unlock_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "解锁密码库")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("解锁后只会尝试密码库中的记录密码，不会显示密码内容。"),
            )
            .child(
                Input::new(&self.master_current)
                    .id("unlock-for-extract")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("unlock-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_unlock_for_extract = false;
                                this.pending_unlock_archive = None;
                                this.unlock_for_browse = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("unlock-confirm")
                            .primary()
                            .label("解锁并自动尝试")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.unlock_for_extract(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Asks before expanding the automatic password search beyond the first
    /// 32 candidates.
    pub(crate) fn render_extra_password_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self
            .pending_extra_password
            .as_ref()
            .map_or(0, |job| job.candidates.len());
        centered_modal(cx, "尝试其余记录密码？")
            .child(format!(
                "前 32 条记录密码均不正确，密码库中还有 {count} 条候选。\
                 继续尝试可能需要较长时间；也可以停止自动尝试并手动输入。"
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("extra-passwords-decline")
                            .label("手动输入")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.decline_extra_password_candidates(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("extra-passwords-try")
                            .primary()
                            .label(format!("尝试其余 {count} 条"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.try_extra_password_candidates(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Confirms execution of an executable or script extracted from an
    /// archive into the managed temporary directory.
    pub(crate) fn render_open_executable_confirm(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = self
            .pending_open_target
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        centered_modal(cx, "打开归档中的可执行文件？")
            .child(format!(
                "该条目已解压到临时目录，但可执行文件或脚本可能修改系统。\n{path}"
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("open-executable-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending_open_target = None;
                                this.show_open_executable_confirm = false;
                                this.status = "已取消打开可执行文件".to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("open-executable-confirm")
                            .danger()
                            .label("仍然打开")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_open_executable(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Confirms permanent removal from an archive or recycling for a regular
    /// filesystem item.
    pub(crate) fn render_delete_confirm(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let archive = matches!(&self.browser.location, BrowserLocation::Archive { .. });
        centered_modal(
            cx,
            if archive {
                "从归档删除"
            } else {
                "移入回收站"
            },
        )
        .child(if archive {
            format!(
                "将从当前归档中删除 {} 个项目，并重建归档。失败时原归档保持不变。",
                self.pending_delete_paths.len()
            )
        } else {
            format!(
                "将删除 {} 个项目。默认进入 Windows 回收站，可在资源管理器中恢复。",
                self.pending_delete_paths.len()
            )
        })
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("delete-cancel")
                        .label("取消")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_delete_confirm = false;
                            this.pending_delete_paths.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("delete-confirm")
                        .danger()
                        .label(if archive {
                            "从归档删除"
                        } else {
                            "移入回收站"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete();
                            cx.notify();
                        })),
                ),
        )
    }

    /// Collects the name for a new folder in the current location.
    pub(crate) fn render_new_folder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "新建文件夹")
            .child(
                Input::new(&self.new_folder_name)
                    .id("new-folder-name")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("new-folder-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_new_folder = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("new-folder-create")
                            .primary()
                            .label("创建")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_folder(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Collects a replacement name for the selected file or archive entry.
    pub(crate) fn render_rename(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "重命名")
            .child(Input::new(&self.rename_name).id("rename-name").w_full())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("rename-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_rename = false;
                                this.rename_target = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("rename-confirm")
                            .primary()
                            .label("重命名")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_rename(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Imports or exports the encrypted vault using an independent passphrase.
    pub(crate) fn render_vault_transfer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "加密密码库导入 / 导出")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "导出文件使用独立口令和 Argon2id + XChaCha20-Poly1305，不提供明文 CSV。",
                    ),
            )
            .child(
                Input::new(&self.vault_transfer_path)
                    .id("vault-transfer-path")
                    .w_full(),
            )
            .child(
                Input::new(&self.vault_transfer_passphrase)
                    .id("vault-transfer-passphrase")
                    .w_full(),
            )
            .child(
                Button::new("toggle-import-replace")
                    .small()
                    .label(if self.vault_transfer_replace {
                        "导入策略：替换现有记录"
                    } else {
                        "导入策略：合并去重"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vault_transfer_replace = !this.vault_transfer_replace;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("vault-export")
                            .primary()
                            .label("导出")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.export_vault(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("vault-import")
                            .label("导入")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.import_vault(cx);
                                cx.notify();
                            })),
                    )
                    .child(Button::new("vault-transfer-cancel").label("关闭").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.show_vault_transfer = false;
                            cx.notify();
                        }),
                    )),
            )
    }

    /// Verifies the current master password before removing it and returning
    /// the vault to DPAPI wrapping.
    pub(crate) fn render_confirm_remove_master(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "删除主密码")
            .child("删除后密码库将改由当前 Windows 用户的 DPAPI 保护。请输入当前主密码确认。")
            .child(
                Input::new(&self.master_current)
                    .id("confirm-remove-master")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("cancel-remove-master")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_confirm_remove_master = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("confirm-remove-master")
                            .danger()
                            .label("确认删除")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_remove_master(cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Warns that clearing the vault is irreversible.
    pub(crate) fn render_confirm_clear_vault(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "清除密码库")
            .child("此操作会删除密码库中的所有记录，无法撤销。建议先加密导出备份。")
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("cancel-clear-vault")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_confirm_clear_vault = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("confirm-clear-vault")
                            .danger()
                            .label("确认清除")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_vault(cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Prevents accidental loss of uncommitted password edits when leaving
    /// the password workspace.
    pub(crate) fn render_discard_password_draft(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "放弃未保存的密码修改？")
            .child("密码管理中的添加、编辑和删除尚未写入密码库。离开前可以继续编辑或丢弃草稿。")
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("keep-password-draft")
                            .label("继续编辑")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_discard_password_draft = false;
                                this.pending_screen = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("discard-draft-and-switch")
                            .danger()
                            .label("丢弃并离开")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.discard_draft_and_switch();
                                cx.notify();
                            })),
                    ),
            )
    }
}
