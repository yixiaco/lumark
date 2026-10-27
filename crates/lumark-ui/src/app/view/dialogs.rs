use super::super::*;
use super::components::{AppIcon, card, form_label, hint_text, option_radio};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;

impl LumarkApp {
    /// 渲染归档密码输入卡片，供打开、解压、测试和条目打开流程共用。
    pub(crate) fn render_archive_password_prompt(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let target = self
            .archive_password_action
            .as_ref()
            .map(password_target_label)
            .unwrap_or_else(|| "加密压缩包".to_string());
        let auto_hint = if self.vault.state() == VaultState::Unlocked {
            "自动输入【密码管理】中密码匹配解密"
        } else {
            "解锁密码库后自动匹配记录密码"
        };
        centered_modal(cx, "输入密码")
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(30.))
                            .flex_shrink_0()
                            .rounded(px(8.))
                            .bg(cx.theme().primary.opacity(0.12))
                            .text_color(cx.theme().primary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Lock.element(16.)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("为加密的文件输入密码："),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .bg(cx.theme().secondary)
                    .border_1()
                    .border_color(cx.theme().input)
                    .text_sm()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(target),
            )
            .child(labeled_input(
                cx,
                "密码",
                Input::new(&self.archive_password_input)
                    .id("archive-password")
                    .w_full(),
            ))
            .child(
                Checkbox::new("archive-password-show")
                    .label("显示密码")
                    .checked(self.archive_password_visible)
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        this.archive_password_visible = *checked;
                        this.archive_password_input.update(cx, |state, cx| {
                            state.set_masked(!*checked, window, cx);
                        });
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("archive-password-auto-vault")
                    .label(auto_hint)
                    .checked(self.archive_password_use_vault)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.archive_password_use_vault = *checked;
                        cx.notify();
                    })),
            )
            .when(self.archive_password_after_failure, |this| {
                this.child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child("记录密码均不正确。输入正确密码后可直接保存，下次自动匹配。"),
                )
            })
            .when(self.vault.state() == VaultState::Unlocked, |this| {
                this.child(
                    Checkbox::new("archive-password-save")
                        .label("保存到密码库（验证成功后写入）")
                        .checked(self.archive_password_save_to_vault)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.archive_password_save_to_vault = *checked;
                            cx.notify();
                        })),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        button("archive-password-manage")
                            .icon(AppIcon::Key.element(16.))
                            .label("密码管理")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_archive_password_prompt = false;
                                this.archive_password_action = None;
                                this.switch_screen(Screen::Passwords);
                                this.status = "请在密码管理中新增匹配密码".to_string();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        button("archive-password-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_archive_password_prompt = false;
                                this.archive_password_action = None;
                                this.status = "已取消输入压缩包密码".to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        button("archive-password-confirm")
                            .primary()
                            .label("确定")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_archive_password(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 提示保存成功打开归档的密码。
    pub(crate) fn render_save_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "保存解压密码")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("密码将写入本地加密密码库，下次可直接用于同类压缩包。"),
            )
            .child(labeled_input(
                cx,
                "名称",
                Input::new(&self.password_label).id("save-label").w_full(),
            ))
            .child(labeled_input(
                cx,
                "密码",
                Input::new(&self.password_value).id("save-value").w_full(),
            ))
            .child(labeled_input(
                cx,
                "适用压缩包（路径或通配符）",
                Input::new(&self.password_pattern)
                    .id("save-pattern")
                    .w_full(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(button("save-dismiss").secondary().label("不保存").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.show_save_prompt = false;
                            cx.notify();
                        }),
                    ))
                    .child(
                        button("save-confirm")
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

    /// 在自动密码尝试开始前请求密码库主密码。
    pub(crate) fn render_unlock_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "解锁密码库")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("解锁后只会尝试密码库中的记录密码，不会显示密码内容。"),
            )
            .child(
                Input::new(&self.master_extract_unlock)
                    .id("unlock-for-extract")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(button("unlock-cancel").label("取消").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.show_unlock_for_extract = false;
                            this.pending_unlock_archive = None;
                            this.unlock_for_browse = false;
                            cx.notify();
                        },
                    )))
                    .child(
                        button("unlock-confirm")
                            .primary()
                            .label("解锁并自动尝试")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.unlock_for_extract(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 自动密码搜索超过前 32 个候选密码时请求确认。
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
                        button("extra-passwords-decline")
                            .label("手动输入")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.decline_extra_password_candidates(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        button("extra-passwords-try")
                            .primary()
                            .label(format!("尝试其余 {count} 条"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.try_extra_password_candidates(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 渲染记录密码校验进度，并提供停止当前候选的入口。
    ///
    /// 只有校验持续到可见时长才返回元素；快速命中时这个弹窗根本不会出现，
    /// 用户只会看到状态栏一闪而过的验证提示。
    pub(crate) fn render_password_attempt_prompt(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(attempt) = self
            .password_attempt
            .as_ref()
            .filter(|attempt| attempt.is_visible())
        else {
            return div().into_any_element();
        };
        let ratio = if attempt.total == 0 {
            0.0
        } else {
            (attempt.index as f32 / attempt.total as f32).clamp(0.0, 1.0)
        };
        centered_modal(cx, "正在尝试记录密码")
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("验证期间不会显示密码内容；可以停止后手动输入。"),
            )
            .child(
                div()
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .bg(cx.theme().secondary)
                    .border_1()
                    .border_color(cx.theme().input)
                    .text_sm()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(attempt.archive.display().to_string()),
            )
            .child(
                Progress::new("password-attempt-progress")
                    .value(ratio)
                    .w_full(),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "第 {} / {} 条候选密码",
                                attempt.index, attempt.total
                            )),
                    )
                    .child(
                        button("password-attempt-hide")
                            .ghost()
                            .label("后台继续")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(attempt) = this.password_attempt.as_mut() {
                                    attempt.hidden = true;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        button("password-attempt-stop")
                            .secondary()
                            .label("停止自动尝试")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_password_attempt(cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 渲染添加到归档的更新模式选择卡片。
    ///
    /// 更新模式只在确认后生效，取消不会触发归档重建，因此可以先浏览选项
    /// 再决定是否写入。
    pub(crate) fn render_archive_add_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let file_count = self.pending_archive_additions.len();
        let folder_count = self.pending_archive_roots.len();
        let target = match &self.browser.location {
            BrowserLocation::Archive { archive, .. } => archive
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("当前归档")
                .to_string(),
            BrowserLocation::FileSystem(_) => "当前归档".to_string(),
        };
        let summary = match (file_count, folder_count) {
            (0, folders) => format!("{folders} 个文件夹将写入 {target}，请选择更新方式："),
            (files, 0) => format!("{files} 个文件将写入 {target}，请选择同名条目的处理方式："),
            (files, folders) => {
                format!("{files} 个文件、{folders} 个文件夹将写入 {target}，请选择更新方式：")
            }
        };
        let hint = match self.archive_add_mode {
            UpdateMode::Replace => "同名条目直接覆盖，新文件全部加入。",
            UpdateMode::Newer => "仅当磁盘文件比归档条目更新时替换，较旧条目保留原内容。",
            UpdateMode::Freshen => "只更新归档中已存在的条目，不会加入新文件。",
            UpdateMode::Sync => {
                "同步会新增缺少的文件、更新较新文件，并删除归档中磁盘已不存在的条目。"
            }
        };
        let hint_color = if self.archive_add_mode == UpdateMode::Sync {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };
        centered_modal(cx, "添加到归档")
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(option_radio(
                        cx,
                        "archive-add-replace",
                        "添加并替换同名文件".to_string(),
                        self.archive_add_mode == UpdateMode::Replace,
                        |app, _, _| app.archive_add_mode = UpdateMode::Replace,
                    ))
                    .child(option_radio(
                        cx,
                        "archive-add-newer",
                        "只更新较新的文件".to_string(),
                        self.archive_add_mode == UpdateMode::Newer,
                        |app, _, _| app.archive_add_mode = UpdateMode::Newer,
                    ))
                    .child(option_radio(
                        cx,
                        "archive-add-freshen",
                        "仅刷新已有条目".to_string(),
                        self.archive_add_mode == UpdateMode::Freshen,
                        |app, _, _| app.archive_add_mode = UpdateMode::Freshen,
                    ))
                    .when(folder_count > 0, |this| {
                        this.child(option_radio(
                            cx,
                            "archive-add-sync",
                            "同步文件夹内容（会删除多余条目）".to_string(),
                            self.archive_add_mode == UpdateMode::Sync,
                            |app, _, _| app.archive_add_mode = UpdateMode::Sync,
                        ))
                    }),
            )
            .child(div().w_full().text_xs().text_color(hint_color).child(hint))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("archive-add-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_archive_add(cx);
                            })),
                    )
                    .child(
                        button("archive-add-confirm")
                            .primary()
                            .label("开始添加")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_archive_add(cx);
                            })),
                    ),
            )
    }

    /// 确认是否执行从归档解压到受控临时目录的可执行文件或脚本。
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
                        button("open-executable-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending_open_target = None;
                                this.show_open_executable_confirm = false;
                                this.status = "已取消打开可执行文件".to_string();
                                cx.notify();
                            })),
                    )
                    .child(
                        button("open-executable-confirm")
                            .danger()
                            .label("仍然打开")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_open_executable(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 确认从归档中永久删除，或把普通文件系统项目移入回收站。
    pub(crate) fn render_delete_confirm(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let archive = matches!(&self.browser.location, BrowserLocation::Archive { .. });
        let count = self.pending_delete_paths.len();
        let names: Vec<String> = self
            .pending_delete_paths
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .take(3)
            .map(ToString::to_string)
            .collect();
        let title = if archive {
            "从归档删除"
        } else {
            "移入回收站"
        };
        let subtitle = if archive {
            "归档会被重建，操作失败时原归档保持不变"
        } else {
            "项目将进入 Windows 回收站"
        };
        let description = if archive {
            "这些项目将从当前归档中永久移除。删除过程会重建归档，请不要同时进行其他写入操作。"
        } else {
            "删除后可以在回收站中还原；清空回收站后将无法恢复。"
        };
        destructive_confirm_card(cx, title, subtitle, "个项目", count, names, description).child(
            h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .p_4()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    button("delete-cancel")
                        .secondary()
                        .label("取消")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_delete_confirm = false;
                            this.pending_delete_paths.clear();
                            cx.notify();
                        })),
                )
                .child(
                    button("delete-confirm")
                        .danger()
                        .icon(AppIcon::Delete.element(14.))
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

    /// 收集当前位置中新文件夹的名称。
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
                        button("new-folder-cancel")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_new_folder = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("new-folder-create")
                            .primary()
                            .label("创建")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_folder(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 收集所选文件或归档条目的新名称。
    pub(crate) fn render_rename(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "重命名")
            .child(Input::new(&self.rename_name).id("rename-name").w_full())
            .child(
                h_flex()
                    .gap_2()
                    .child(button("rename-cancel").label("取消").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.show_rename = false;
                            this.rename_target = None;
                            cx.notify();
                        },
                    )))
                    .child(button("rename-confirm").primary().label("重命名").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.confirm_rename(window, cx);
                            cx.notify();
                        }),
                    )),
            )
    }

    /// 使用独立口令导入或导出加密密码库，并可导出明文 CSV。
    ///
    /// 主密码保护的密码库锁定时不保留记录，因此弹窗先提供解锁入口；
    /// 解锁后同一层立即切换为导入/导出表单。
    pub(crate) fn render_vault_transfer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.vault.state() == VaultState::Locked;
        centered_modal(cx, "加密密码库导入 / 导出")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "加密备份使用独立口令和 Argon2id + XChaCha20-Poly1305；CSV 为明文，任何程序都能读取。",
                    ),
            )
            .when(locked, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(cx.theme().warning.opacity(0.45))
                        .bg(cx.theme().warning.opacity(0.14))
                        .p_3()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(AppIcon::Lock.element(15.))
                                .child(div().text_sm().font_semibold().child("密码库已锁定")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("锁定时不保留记录，导入、导出和 CSV 都需要先解锁。"),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(
                                    Input::new(&self.vault_transfer_unlock)
                                        .id("vault-transfer-unlock")
                                        .flex_1()
                                        .min_w_0(),
                                )
                                .child(
                                    button("vault-transfer-unlock-confirm")
                                        .primary()
                                        .label("解锁")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.unlock_vault_transfer(window, cx);
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
            .when(!locked, |this| {
                this.child(labeled_input(
                    cx,
                    "导出位置",
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            Input::new(&self.vault_export_dir)
                                .id("vault-export-dir")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            button("vault-export-browse")
                                .ghost()
                                .icon(AppIcon::FolderOpen.element(15.))
                                .label("浏览…")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.browse_vault_export_dir(cx);
                                })),
                        ),
                ))
                .child(hint_text(
                    cx,
                    "导出文件写入所选文件夹，文件名自动附带本地时间戳，不覆盖旧备份。",
                ))
                .child(labeled_input(
                    cx,
                    "独立口令",
                    Input::new(&self.vault_transfer_passphrase)
                        .id("vault-transfer-passphrase")
                        .w_full(),
                ))
                .child(labeled_input(
                    cx,
                    "导入文件",
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            Input::new(&self.vault_import_path)
                                .id("vault-import-path")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            button("vault-import-browse")
                                .ghost()
                                .icon(AppIcon::FolderOpen.element(15.))
                                .label("浏览…")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.browse_vault_import(cx);
                                })),
                        ),
                ))
                .child(
                    button("toggle-import-replace")
                        .cursor_pointer()
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
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .flex_wrap()
                    .when(!locked, |this| {
                        this.child(
                            button("vault-export")
                                .primary()
                                .icon(AppIcon::Save.element(15.))
                                .label("导出加密备份")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_vault(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            button("vault-export-csv")
                                .warning()
                                .icon(AppIcon::Doc.element(15.))
                                .label("导出明文 CSV")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_vault_csv(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            button("vault-import")
                                .label("导入")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.import_vault(window, cx);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        button("vault-transfer-cancel")
                            .label("关闭")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_vault_transfer = false;
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 删除主密码前验证当前主密码，并让密码库恢复 DPAPI 包裹。
    pub(crate) fn render_confirm_remove_master(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "删除主密码")
            .child(
                "删除后密码库将改由与当前计算机绑定的 Windows DPAPI 保护。请输入当前主密码确认。",
            )
            .child(
                Input::new(&self.master_remove)
                    .id("confirm-remove-master")
                    .w_full(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("cancel-remove-master")
                            .label("取消")
                            .on_click(cx.listener(|this, _, window, cx| {
                                set_input(&this.master_remove, "", window, cx);
                                this.show_confirm_remove_master = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("confirm-remove-master")
                            .danger()
                            .label("确认删除")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_remove_master(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 警告清除密码库不可恢复。
    pub(crate) fn render_confirm_clear_vault(&self, cx: &mut Context<Self>) -> impl IntoElement {
        centered_modal(cx, "清除密码库")
            .child("此操作会删除密码库中的所有记录，无法撤销。建议先加密导出备份。")
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        button("cancel-clear-vault")
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_confirm_clear_vault = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("confirm-clear-vault")
                            .danger()
                            .label("确认清除")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_vault(window, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 确认从密码库永久删除当前记录。
    pub(crate) fn render_confirm_delete_password(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self
            .selected_password
            .and_then(|id| self.find_password(id))
            .map(|entry| entry.display_name().to_string())
            .unwrap_or_else(|| "所选密码".to_string());
        destructive_confirm_card(
            cx,
            "删除密码记录",
            "记录将从本地加密密码库移除",
            "条记录",
            1,
            vec![label],
            "删除后无法恢复，但不会影响已经解压的文件。",
        )
        .child(
            h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .p_4()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    button("cancel-delete-password")
                        .secondary()
                        .label("取消")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_confirm_delete_password = false;
                            cx.notify();
                        })),
                )
                .child(
                    button("confirm-delete-password")
                        .danger()
                        .icon(AppIcon::Delete.element(14.))
                        .label("删除记录")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete_password();
                            cx.notify();
                        })),
                ),
        )
    }

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

/// 构建“标签 + 输入框”的纵向字段，避免表单只露出无说明的输入框。
fn labeled_input(cx: &gpui_kit::App, label: &str, input: impl IntoElement) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(form_label(cx, label))
        .child(input)
}

/// 返回密码弹窗需要展示的归档或条目名称。
fn password_target_label(action: &PendingPasswordAction) -> String {
    match action {
        PendingPasswordAction::OpenArchive { archive, .. }
        | PendingPasswordAction::ExtractDialog { archive }
        | PendingPasswordAction::Test { archive } => archive.display().to_string(),
        PendingPasswordAction::OpenEntry {
            archive, relative, ..
        } => {
            format!("{} · {relative}", archive.display())
        }
    }
}

/// 构建文件或密码删除共用的危险确认卡片。
///
/// 顶部按危险级别着色的图标、待删除项目清单和底部操作区保持一致，
/// 避免“移入回收站”和“删除密码记录”出现两种不同的确认样式。
fn destructive_confirm_card(
    cx: &Context<LumarkApp>,
    title: &str,
    subtitle: &str,
    unit: &str,
    count: usize,
    names: Vec<String>,
    description: &str,
) -> gpui_kit::Div {
    let names: Vec<String> = names.into_iter().take(3).collect();
    let hidden = count.saturating_sub(names.len());
    v_flex()
        .w(px(460.))
        .overflow_hidden()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(14.))
        .shadow_xl()
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .p_5()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .size(px(40.))
                        .flex_shrink_0()
                        .rounded(px(12.))
                        .bg(cx.theme().danger.opacity(0.14))
                        .text_color(cx.theme().danger_foreground)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(AppIcon::Delete.element(20.)),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .gap(px(2.))
                        .child(div().text_lg().font_semibold().child(title.to_string()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .whitespace_normal()
                                .child(subtitle.to_string()),
                        ),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p_5()
                .gap_3()
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .p_3()
                        .rounded(px(8.))
                        .bg(cx.theme().muted)
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_color(cx.theme().danger_foreground)
                                        .child(AppIcon::Delete.element(14.)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .child(format!("{count} {unit}")),
                                ),
                        )
                        .children(names.into_iter().map(|name| {
                            div()
                                .w_full()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("· {name}"))
                        }))
                        .when(hidden > 0, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("还有 {hidden} {unit}…")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(description.to_string()),
                ),
        )
}
