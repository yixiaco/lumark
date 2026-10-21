use super::super::*;
use super::components::{AppIcon, card, section_heading};

impl LumarkApp {
    /// 渲染密码工作区。
    ///
    /// 用户在提交前以草稿形式编辑记录，因此可以整体丢弃未确认的修改。
    /// 页面不会主动暴露密码，除非用户明确显示或复制密码。
    pub(crate) fn render_passwords(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let search = input_value(&self.password_search, cx).to_lowercase();
        let entries: Vec<PasswordEntry> = self
            .password_draft
            .clone()
            .unwrap_or_else(|| self.vault.entries())
            .into_iter()
            .filter(|entry| {
                search.is_empty()
                    || entry.label.to_lowercase().contains(&search)
                    || entry.note.to_lowercase().contains(&search)
                    || entry
                        .archive_pattern
                        .as_deref()
                        .is_some_and(|pattern| pattern.to_lowercase().contains(&search))
            })
            .collect();
        let settings = self.vault.settings();
        let locked = self.vault.state() == VaultState::Locked;
        let auto_lock_label = if settings.auto_lock_minutes == 0 {
            "空闲锁定：关".to_string()
        } else {
            format!("空闲锁定：{} 分钟", settings.auto_lock_minutes)
        };
        let total_records = self
            .password_draft
            .as_ref()
            .map_or(self.vault.entry_count(), Vec::len);

        v_flex()
            .id("passwords-page")
            .w_full()
            .h_full()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .w_full()
                    .child(password_stat(
                        cx,
                        AppIcon::Key,
                        "记录总数",
                        &total_records.to_string(),
                    ))
                    .child(password_stat(
                        cx,
                        if locked {
                            AppIcon::Lock
                        } else {
                            AppIcon::Unlock
                        },
                        "密码库状态",
                        if locked { "已锁定" } else { "已解锁" },
                    ))
                    .child(password_stat(
                        cx,
                        AppIcon::Shield,
                        "自动尝试",
                        if settings.auto_extract { "已开启" } else { "已关闭" },
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_h_0()
                    .flex_shrink_0()
                    .items_stretch()
                    .gap_3()
                    .child(
                        card(cx)
                            .flex_1()
                            .min_w(px(420.))
                            .min_h(px(430.))
                            .child(
                                h_flex()
                                    .p_4()
                                    .gap_2()
                                    .items_center()
                                    .bg(cx.theme().muted.opacity(0.45))
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_semibold()
                                            .child("记录列表"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py(px(1.))
                                            .rounded_full()
                                            .bg(cx.theme().muted)
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{} 条", entries.len())),
                                    )
                                    .child(
                                        h_flex()
                                            .ml_auto()
                                            .w(px(320.))
                                            .h(px(34.))
                                            .px_2()
                                            .gap_2()
                                            .rounded_md()
                                            .bg(cx.theme().secondary)
                                            .border_1()
                                            .border_color(cx.theme().input)
                                            .child(AppIcon::Search.element(14.))
                                            .child(
                                                Input::new(&self.password_search)
                                                    .id("password-search")
                                                    .flex_1()
                                                    .min_w_0(),
                                            ),
                                    )
                                    .child(
                                        button("password-add-new")
                                            .ghost()
                                            .small()
                                            .cursor_pointer()
                                            .icon(AppIcon::Add.element(15.))
                                            .tooltip("新建密码记录")
                                            .on_click(cx.listener(
                                                |this, _, window, cx| {
                                                    this.selected_password = None;
                                                    this.revealed_password = None;
                                                    set_input(
                                                        &this.password_label,
                                                        "",
                                                        window,
                                                        cx,
                                                    );
                                                    set_input(
                                                        &this.password_value,
                                                        "",
                                                        window,
                                                        cx,
                                                    );
                                                    set_input(
                                                        &this.password_confirm,
                                                        "",
                                                        window,
                                                        cx,
                                                    );
                                                    set_input(
                                                        &this.password_pattern,
                                                        "",
                                                        window,
                                                        cx,
                                                    );
                                                    set_input(
                                                        &this.password_note,
                                                        "",
                                                        window,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                },
                                            )),
                                    ),
                            )
                            .when(locked, |this| {
                                this.child(
                                    v_flex()
                                        .flex_1()
                                        .min_h(px(310.))
                                        .items_center()
                                        .justify_center()
                                        .gap_3()
                                        .p_8()
                                        .child(
                                            div()
                                                .size(px(58.))
                                                .rounded_full()
                                                .bg(cx.theme().warning)
                                                .text_color(cx.theme().warning_foreground)
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(AppIcon::Lock.element(26.)),
                                        )
                                        .child(
                                            div()
                                                .font_semibold()
                                                .child("密码库已锁定"),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("输入主密码后才能查看、编辑或自动尝试记录密码。"),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    Input::new(&self.master_current)
                                                        .id("unlock-master")
                                                        .w(px(260.)),
                                                )
                                                .child(
                                                    button("unlock")
                                                        .primary()
                                                        .icon(AppIcon::Unlock.element(15.))
                                                        .label("解锁")
                                                        .on_click(cx.listener(
                                                            |this, _, _, cx| {
                                                                this.unlock_vault(cx);
                                                                cx.notify();
                                                            },
                                                        )),
                                                ),
                                        ),
                                )
                            })
                            .when(!locked, |this| {
                                this.child(
                                    v_flex()
                                        .id("password-list")
                                        .flex_1()
                                        .min_h(px(310.))
                                        .overflow_y_scroll()
                                        .when(entries.is_empty(), |this| {
                                            this.child(
                                                v_flex()
                                                    .p_8()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(AppIcon::Key.element(28.))
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_semibold()
                                                            .child("还没有保存的密码"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(
                                                                "在右侧填写密码并保存，或解压成功后选择保存。",
                                                            ),
                                                    ),
                                            )
                                        })
                                        .children(entries.into_iter().map(|entry| {
                                            let id = entry.id;
                                            let reveal_id = entry.id;
                                            let pattern_label = entry
                                                .archive_pattern
                                                .clone()
                                                .unwrap_or_else(|| "通用密码".to_string());
                                            let selected =
                                                self.selected_password == Some(id);
                                            let revealed =
                                                self.revealed_password == Some(id);
                                            let password_text = if revealed {
                                                entry.password.expose().to_string()
                                            } else {
                                                "••••••••".to_string()
                                            };
                                            h_flex()
                                                .id(format!("password-row-{id}"))
                                                .w_full()
                                                .min_h(px(58.))
                                                .gap_3()
                                                .px_4()
                                                .py_2()
                                                .border_b_1()
                                                .border_color(cx.theme().border)
                                                .when(selected, |this| {
                                                    this.bg(cx.theme().list_active)
                                                })
                                                .when(!selected, |this| {
                                                    this.hover(|style| {
                                                        style.bg(cx.theme().list_hover)
                                                    })
                                                })
                                                .child(
                                                    div()
                                                        .size(px(32.))
                                                        .rounded_md()
                                                        .bg(cx.theme().accent)
                                                        .text_color(
                                                            cx.theme().accent_foreground,
                                                        )
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .child(AppIcon::Key.element(16.)),
                                                )
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .gap_1()
                                                        .child(
                                                            button(format!(
                                                                "password-select-{id}"
                                                            ))
                                                            .ghost()
                                                            .small()
                                                            .cursor_pointer()
                                                            .w_full()
                                                            .justify_start()
                                                            .label(
                                                                entry
                                                                    .display_name()
                                                                    .to_string(),
                                                            )
                                                            .on_click(cx.listener(
                                                                move |this,
                                                                      _,
                                                                      window,
                                                                      cx| {
                                                                    this.selected_password =
                                                                        Some(id);
                                                                    if let Some(entry) =
                                                                        this.find_password(id)
                                                                    {
                                                                        set_input(
                                                                            &this.password_label,
                                                                            &entry.label,
                                                                            window,
                                                                            cx,
                                                                        );
                                                                        set_input(
                                                                            &this.password_value,
                                                                            entry
                                                                                .password
                                                                                .expose(),
                                                                            window,
                                                                            cx,
                                                                        );
                                                                        set_input(
                                                                            &this.password_confirm,
                                                                            entry
                                                                                .password
                                                                                .expose(),
                                                                            window,
                                                                            cx,
                                                                        );
                                                                        set_input(
                                                                            &this.password_pattern,
                                                                            entry
                                                                                .archive_pattern
                                                                                .as_deref()
                                                                                .unwrap_or(""),
                                                                            window,
                                                                            cx,
                                                                        );
                                                                        set_input(
                                                                            &this.password_note,
                                                                            &entry.note,
                                                                            window,
                                                                            cx,
                                                                        );
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(pattern_label),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(104.))
                                                        .text_sm()
                                                        .font_semibold()
                                                        .child(password_text),
                                                )
                                                .child(
                                                    button(format!(
                                                        "password-reveal-{id}"
                                                    ))
                                                    .ghost()
                                                    .small()
                                                    .cursor_pointer()
                                                    .icon(
                                                        if revealed {
                                                            AppIcon::Lock
                                                        } else {
                                                            AppIcon::Unlock
                                                        }
                                                        .element(14.),
                                                    )
                                                    .tooltip(if revealed {
                                                        "隐藏密码"
                                                    } else {
                                                        "显示 10 秒"
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.toggle_password_reveal(
                                                                reveal_id,
                                                                cx,
                                                            );
                                                            cx.notify();
                                                        },
                                                    )),
                                                )
                                        })),
                                )
                            })
                            .child(
                                h_flex()
                                    .p_3()
                                    .gap_2()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        button("commit-password-draft")
                                            .primary()
                                            .icon(AppIcon::Save.element(15.))
                                            .label("确定")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.commit_password_draft();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        button("discard-password-draft")
                                            .label("取消草稿")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.discard_password_draft();
                                                cx.notify();
                                            })),
                                    )
                                    .when(self.password_draft_dirty, |this| {
                                        this.child(
                                            div()
                                                .ml_auto()
                                                .text_xs()
                                                .text_color(cx.theme().warning)
                                                .child("有未提交的修改"),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        card(cx)
                            .w(px(372.))
                            .flex_shrink_0()
                            .child(
                                h_flex()
                                    .p_4()
                                    .gap_2()
                                    .items_center()
                                    .bg(cx.theme().muted.opacity(0.45))
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(AppIcon::Edit.element(16.))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_semibold()
                                            .child(if self.selected_password.is_some() {
                                                "编辑记录"
                                            } else {
                                                "新建记录"
                                            }),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .p_4()
                                    .gap_3()
                                    .child(form_label(cx, "名称"))
                                    .child(
                                        Input::new(&self.password_label)
                                            .id("password-label")
                                            .w_full(),
                                    )
                                    .child(form_label(cx, "密码"))
                                    .child(
                                        Input::new(&self.password_value)
                                            .id("password-value")
                                            .w_full(),
                                    )
                                    .child(form_label(cx, "确认密码"))
                                    .child(
                                        Input::new(&self.password_confirm)
                                            .id("password-confirm")
                                            .w_full(),
                                    )
                                    .child(form_label(cx, "适用压缩包"))
                                    .child(
                                        Input::new(&self.password_pattern)
                                            .id("password-pattern")
                                            .w_full(),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("支持完整路径或 * ? 通配符；留空表示通用密码。"),
                                    )
                                    .child(form_label(cx, "备注"))
                                    .child(
                                        Input::new(&self.password_note)
                                            .id("password-note")
                                            .w_full(),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                button("save-password")
                                                    .primary()
                                                    .icon(AppIcon::Save.element(15.))
                                                    .label(if self.selected_password.is_some() {
                                                        "更新草稿"
                                                    } else {
                                                        "添加到草稿"
                                                    })
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.draft_save_password(
                                                                window, cx,
                                                            );
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button("copy-password")
                                                    .icon(AppIcon::Copy.element(15.))
                                                    .label("复制")
                                                    .on_click(cx.listener(
                                                        |this, _, _, cx| {
                                                            this.copy_password(cx);
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button("delete-password")
                                                    .danger()
                                                    .icon(AppIcon::Delete.element(15.))
                                                    .label("删除")
                                                    .on_click(cx.listener(
                                                        |this, _, _, cx| {
                                                            this.draft_delete_password();
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mx_4()
                                    .h(px(1.))
                                    .bg(cx.theme().border),
                            )
                            .child(
                                v_flex()
                                    .p_4()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(AppIcon::Shield.element(16.))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_semibold()
                                                    .child("主密码"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                "设置主密码后会移除 DPAPI 包裹；忘记主密码将无法恢复。",
                                            ),
                                    )
                                    .child(
                                        Input::new(&self.master_current)
                                            .id("master-current")
                                            .w_full(),
                                    )
                                    .child(
                                        Input::new(&self.master_new)
                                            .id("master-new")
                                            .w_full(),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                button("set-master")
                                                    .small()
                                                    .cursor_pointer()
                                                    .label("设置/修改主密码")
                                                    .on_click(cx.listener(
                                                        |this, _, _, cx| {
                                                            this.set_master_password(cx);
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button("remove-master")
                                                    .small()
                                                    .cursor_pointer()
                                                    .label("删除主密码")
                                                    .on_click(cx.listener(
                                                        |this, _, _, cx| {
                                                            this.show_confirm_remove_master =
                                                                true;
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    ),
            )
            .child(
                card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .child(section_heading(
                        cx,
                        AppIcon::Shield,
                        "自动解压与安全",
                        Some("只在本机验证记录密码".to_string()),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                button("toggle-auto")
                                    .icon(AppIcon::Key.element(15.))
                                    .label(if settings.auto_extract {
                                        "自动尝试密码：开"
                                    } else {
                                        "自动尝试密码：关"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_auto_extract(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("toggle-save")
                                    .icon(AppIcon::Save.element(15.))
                                    .label(if settings.prompt_save {
                                        "保存密码提示：开"
                                    } else {
                                        "保存密码提示：关"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_prompt_save(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("toggle-auto-lock")
                                    .icon(AppIcon::Time.element(15.))
                                    .label(auto_lock_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_auto_lock(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("lock-vault")
                                    .icon(AppIcon::Lock.element(15.))
                                    .label("立即锁定")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.vault.lock();
                                        this.password_draft = None;
                                        this.password_draft_dirty = false;
                                        this.status = "密码库已锁定".to_string();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("vault-transfer")
                                    .icon(AppIcon::Copy.element(15.))
                                    .label("导入 / 导出")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if input_value(&this.vault_transfer_path, cx)
                                            .trim()
                                            .is_empty()
                                        {
                                            let target = dirs::document_dir()
                                                .unwrap_or_else(std::env::temp_dir)
                                                .join("Lumark.lumarkvault");
                                            set_input(
                                                &this.vault_transfer_path,
                                                target.to_string_lossy().as_ref(),
                                                window,
                                                cx,
                                            );
                                        }
                                        this.show_vault_transfer = true;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("vault-clear")
                                    .danger()
                                    .icon(AppIcon::Delete.element(15.))
                                    .label("清除密码库")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_confirm_clear_vault = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .rounded_md()
                            .bg(cx.theme().muted)
                            .p_3()
                            .child(AppIcon::Shield.element(16.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "密码库使用 XChaCha20-Poly1305 加密；设置主密码后由 Argon2id 派生密钥保护。",
                                    ),
                            ),
                    ),
            )
    }

    /// 使用分组卡片和简洁的面向操作设置行，而不是冗长且缺乏层次的表单。
    pub(crate) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.get();
        let theme_label = match settings.theme {
            ThemePreference::System => "跟随系统",
            ThemePreference::Light => "浅色",
            ThemePreference::Dark => "深色",
        };
        let preset_label = match settings.default_preset {
            SettingsPreset::Speed => "速度优先",
            SettingsPreset::Balanced => "平衡",
            SettingsPreset::Size => "体积优先",
        };
        v_flex()
            .id("settings-page")
            .w_full()
            .h_full()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .w_full()
                    .child(password_stat(
                        cx,
                        AppIcon::Tool,
                        "界面主题",
                        theme_label,
                    ))
                    .child(password_stat(
                        cx,
                        AppIcon::Compression,
                        "默认格式",
                        &settings.default_format.to_ascii_uppercase(),
                    ))
                    .child(password_stat(
                        cx,
                        AppIcon::Shield,
                        "本地数据",
                        if self.vault_error.is_some() {
                            "需要检查"
                        } else {
                            "仅本机"
                        },
                    )),
            )
            .when_some(self.settings_error.clone(), |this, error| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .text_sm()
                        .child(format!("设置文件读取失败：{error}")),
                )
            })
            .child(
                card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .child(section_heading(
                        cx,
                        AppIcon::Tool,
                        "外观与体验",
                        Some("界面会随系统外观自动切换".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "主题",
                        "浅色、深色或跟随 Windows。",
                        button("cycle-theme")
                            .label(theme_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cycle_theme(window, cx);
                                cx.notify();
                            })),
                    ))
                    .when_some(self.theme_color_picker.as_ref(), |this, picker| {
                        this.child(
                            div().h(px(1.)).bg(cx.theme().border),
                        )
                        .child(settings_row(
                            cx,
                            "主题色",
                            "强调色默认蓝色，点击色块可选择任意颜色。",
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    ColorPicker::new(picker)
                                        .featured_colors(theme_accent_presets())
                                        .small()
                                        .accessibility_label("主题色"),
                                )
                                .child(
                                    div()
                                        .w(px(64.))
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(settings.accent_color.clone()),
                                ),
                        ))
                    })
                    .child(
                        div().h(px(1.)).bg(cx.theme().border),
                    )
                    .child(settings_row(
                        cx,
                        "显示详细信息区",
                        "在文件侧栏底部显示大小、时间、CRC 和压缩方法。",
                        button("toggle-details-visible")
                            .label(if settings.details_visible {
                                "已显示"
                            } else {
                                "已隐藏"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_details();
                                cx.notify();
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        "文件列表列宽",
                        "在紧凑、标准和宽松三档列宽之间切换。",
                        button("cycle-column-layout")
                            .label(match self.column_layout {
                                0 => "紧凑列",
                                2 => "宽松列",
                                _ => "标准列",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_column_layout();
                                cx.notify();
                            })),
                    )),
            )
            .child(
                card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .child(section_heading(
                        cx,
                        AppIcon::Compression,
                        "压缩与解压",
                        Some("新任务会继承这些默认值".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "默认格式",
                        "ZIP 兼容性最好，7Z 适合更小体积。",
                        button("cycle-format")
                            .label(settings.default_format.to_ascii_uppercase())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_default_format();
                                cx.notify();
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        "压缩预设",
                        "控制默认压缩级别、字典和固实策略。",
                        button("cycle-preset")
                            .label(preset_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_default_preset();
                                cx.notify();
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        "解压后打开目录",
                        "任务完成后直接定位到输出位置。",
                        button("toggle-open-after")
                            .label(if settings.open_after_extract {
                                "已开启"
                            } else {
                                "已关闭"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_open_after_extract();
                                cx.notify();
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        "覆盖策略",
                        "同名文件默认询问，也可设为覆盖、跳过或重命名。",
                        button("cycle-overwrite")
                            .label(match settings.overwrite_policy {
                                lumark_settings::OverwritePolicy::Ask => "询问",
                                lumark_settings::OverwritePolicy::Overwrite => "覆盖",
                                lumark_settings::OverwritePolicy::Skip => "跳过",
                                lumark_settings::OverwritePolicy::Rename => "重命名",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_overwrite_policy();
                                cx.notify();
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        "归档编辑备份",
                        "修改归档前保留上一版本，便于恢复。",
                        button("toggle-archive-backup")
                            .label(if settings.archive_edit_backup {
                                "已开启"
                            } else {
                                "已关闭"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_archive_backup();
                                cx.notify();
                            })),
                    )),
            )
            .child(
                card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .child(section_heading(
                        cx,
                        AppIcon::Key,
                        "安全与密码",
                        None,
                    ))
                    .when_some(self.vault_error.clone(), |this, error| {
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
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "密码库使用 XChaCha20-Poly1305 加密；未设置主密码时由 Windows DPAPI 包裹。设置主密码后会移除 DPAPI 包裹，忘记主密码无法恢复。",
                            ),
                    )
                    .child(settings_row(
                        cx,
                        "密码库状态",
                        &format!(
                            "{} · 主密码{}",
                            match self.vault.state() {
                                VaultState::Unlocked => "已解锁",
                                VaultState::Locked => "已锁定",
                                VaultState::Uninitialized => "尚未创建",
                            },
                            if self.vault.has_master_password() {
                                "已设置"
                            } else {
                                "未设置"
                            }
                        ),
                        button("settings-open-passwords")
                            .icon(AppIcon::Key.element(15.))
                            .label("管理密码")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.switch_screen(Screen::Passwords);
                                cx.notify();
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        "密码库位置",
                        &self.vault.path().display().to_string(),
                        button("settings-vault-transfer")
                            .icon(AppIcon::Copy.element(15.))
                            .label("导入 / 导出")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if input_value(&this.vault_transfer_path, cx)
                                    .trim()
                                    .is_empty()
                                {
                                    let target = dirs::document_dir()
                                        .unwrap_or_else(std::env::temp_dir)
                                        .join("Lumark.lumarkvault");
                                    set_input(
                                        &this.vault_transfer_path,
                                        target.to_string_lossy().as_ref(),
                                        window,
                                        cx,
                                    );
                                }
                                this.show_vault_transfer = true;
                                cx.notify();
                            })),
                    )),
            )
            .child(
                card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .child(section_heading(
                        cx,
                        AppIcon::Setting,
                        "Windows 集成",
                        Some("按当前用户注册，无需管理员权限".to_string()),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                button("register-shell")
                                    .icon(AppIcon::Check.element(15.))
                                    .label("注册右键菜单")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.register_shell_integration();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("unregister-shell")
                                    .icon(AppIcon::Close.element(15.))
                                    .label("移除右键菜单")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.unregister_shell_integration();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("default-apps")
                                    .icon(AppIcon::Tool.element(15.))
                                    .label("打开默认应用设置")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_default_apps_page();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_2()
                    .child(section_heading(cx, AppIcon::Shield, "关于 Lumark", None))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("本地优先的压缩管理器 · Rust + GPUI · v0.1.0"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("设置文件：{}", self.settings.path().display())),
                    ),
            )
    }

    /// 渲染任务中心页面。
    ///
    /// 任务列表使用完整页面而不是浮层，避免浮层下方文件表继续接收鼠标事件；
    /// 每条记录保留实时进度和最终结果，点击行可重新打开独立进度窗口。
    pub(crate) fn render_task_center(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self
            .task_records
            .iter()
            .filter(|record| !is_terminal_task_state(record.state))
            .count();
        let completed = self
            .task_records
            .iter()
            .filter(|record| record.state == TaskState::Completed)
            .count();
        let total = self.task_records.len();
        v_flex()
            .size_full()
            .p_5()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(task_stat_card(
                        cx,
                        AppIcon::Time,
                        "进行中",
                        running,
                        cx.theme().primary,
                    ))
                    .child(task_stat_card(
                        cx,
                        AppIcon::Check,
                        "已完成",
                        completed,
                        cx.theme().success_foreground,
                    ))
                    .child(task_stat_card(
                        cx,
                        AppIcon::List,
                        "任务总数",
                        total,
                        cx.theme().muted_foreground,
                    )),
            )
            .child(
                card(cx)
                    .flex_1()
                    .min_h_0()
                    .child(
                        h_flex()
                            .w_full()
                            .flex_shrink_0()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .bg(cx.theme().muted.opacity(0.45))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(AppIcon::List.element(16.))
                                    .child(div().font_semibold().child("任务队列")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("共 {total} 个任务")),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("task-center-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(self.task_records.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .items_center()
                                        .justify_center()
                                        .gap_2()
                                        .p_8()
                                        .child(AppIcon::List.element(28.))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("还没有任务"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("压缩、解压和测试任务会显示在这里，点击任务可查看进度。"),
                                        ),
                                )
                            })
                            .children(
                                self.task_records
                                    .iter()
                                    .rev()
                                    .map(|record| self.render_task_row(record, cx)),
                            ),
                    ),
            )
    }

    /// 渲染任务中心中的一条任务记录。
    fn render_task_row(&self, record: &TaskRecord, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let live_state = self
            .tasks
            .control(record.id)
            .map(|control| control.state())
            .unwrap_or(record.state);
        let state = match record.state {
            TaskState::Queued => TaskState::Queued,
            state if is_terminal_task_state(state) => state,
            _ => match live_state {
                lumark_tasks::TaskState::Paused => TaskState::Paused,
                lumark_tasks::TaskState::Cancelled => TaskState::Cancelling,
                _ => TaskState::Running,
            },
        };
        let (badge_bg, badge_fg) = task_state_colors(cx, state);
        let ratio = if state == TaskState::Completed {
            1.0
        } else {
            record.progress.ratio().clamp(0.0, 1.0)
        };
        let task_id = record.id;
        let icon = match record.kind {
            TaskKind::Compress => AppIcon::Compression,
            TaskKind::Extract => AppIcon::Zip,
            TaskKind::Test => AppIcon::Shield,
            TaskKind::Edit => AppIcon::Edit,
            TaskKind::LoadArchive => AppIcon::FileZip,
            TaskKind::PasswordResolve => AppIcon::Key,
        };
        let detail = record
            .message
            .clone()
            .or_else(|| {
                (!record.progress.current.is_empty())
                    .then(|| format!("正在处理：{}", record.progress.current))
            })
            .unwrap_or_else(|| {
                if state == TaskState::Queued {
                    "等待开始…".to_string()
                } else {
                    "正在准备文件…".to_string()
                }
            });
        let metrics = format!(
            "{} / {} · {} / {} 项 · {}/秒 · {}",
            format_size(record.progress.bytes_done),
            format_size(record.progress.bytes_total),
            record.progress.entries_done,
            record.progress.entries_total,
            format_size(record.progress.speed_bps.max(0.0) as u64),
            format_duration(record.progress.elapsed_seconds),
        );
        h_flex()
            .id(format!("task-row-{task_id}"))
            .w_full()
            .gap_3()
            .items_start()
            .p_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .hover(|style| style.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_progress_window_for(task_id, window, cx);
                cx.notify();
            }))
            .child(
                div()
                    .size(px(34.))
                    .mt(px(2.))
                    .flex_shrink_0()
                    .rounded(px(8.))
                    .bg(badge_bg)
                    .text_color(badge_fg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.element(17.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .font_semibold()
                                    .child(record.label.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_2()
                                    .py(px(1.))
                                    .rounded(px(5.))
                                    .bg(badge_bg)
                                    .text_color(badge_fg)
                                    .text_xs()
                                    .child(task_state_label(state)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(5.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .w(gpui_kit::relative(ratio))
                                    .h_full()
                                    .rounded_full()
                                    .bg(if state == TaskState::Failed {
                                        cx.theme().danger
                                    } else {
                                        cx.theme().primary
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(metrics),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        button(format!("task-open-{task_id}"))
                            .ghost()
                            .xsmall()
                            .label("查看")
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_progress_window_for(task_id, window, cx);
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .when(
                        matches!(state, TaskState::Running | TaskState::Paused),
                        |this| {
                            this.child(
                                button(format!("task-pause-{task_id}"))
                                    .ghost()
                                    .xsmall()
                                    .label(if state == TaskState::Paused {
                                        "继续"
                                    } else {
                                        "暂停"
                                    })
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(control) = this.tasks.control(task_id) {
                                            if control.state() == lumark_tasks::TaskState::Paused {
                                                control.resume();
                                            } else {
                                                control.pause();
                                            }
                                        }
                                        cx.stop_propagation();
                                        cx.notify();
                                    })),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}

/// 渲染任务中心的统计卡片。
fn task_stat_card(
    cx: &Context<LumarkApp>,
    icon: AppIcon,
    label: &str,
    value: usize,
    color: gpui_kit::Hsla,
) -> impl IntoElement {
    card(cx).flex_1().p_4().child(
        h_flex()
            .gap_3()
            .items_center()
            .child(
                div()
                    .size(px(34.))
                    .rounded(px(8.))
                    .bg(color.opacity(0.12))
                    .text_color(color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.element(17.)),
            )
            .child(
                v_flex()
                    .gap(px(0.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    )
                    .child(div().text_lg().font_semibold().child(value.to_string())),
            ),
    )
}

/// 返回任务状态徽标的背景色和前景色。
fn task_state_colors(
    cx: &Context<LumarkApp>,
    state: TaskState,
) -> (gpui_kit::Hsla, gpui_kit::Hsla) {
    match state {
        TaskState::Queued | TaskState::Cancelled => {
            (cx.theme().secondary, cx.theme().secondary_foreground)
        }
        TaskState::Running => (cx.theme().accent, cx.theme().accent_foreground),
        TaskState::Paused => (cx.theme().warning, cx.theme().warning_foreground),
        TaskState::Cancelling | TaskState::Failed => {
            (cx.theme().danger, cx.theme().danger_foreground)
        }
        TaskState::Completed => (cx.theme().success, cx.theme().success_foreground),
    }
}

/// 渲染内容页上方的紧凑指标卡片。
fn password_stat(
    cx: &Context<LumarkApp>,
    icon: AppIcon,
    label: &str,
    value: &str,
) -> impl IntoElement {
    card(cx).flex_1().p_4().child(
        h_flex()
            .gap_3()
            .items_center()
            .child(
                div()
                    .size(px(36.))
                    .rounded(px(10.))
                    .bg(cx.theme().primary.opacity(0.12))
                    .text_color(cx.theme().primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.element(18.)),
            )
            .child(
                v_flex()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    )
                    .child(div().text_lg().font_semibold().child(value.to_string())),
            ),
    )
}

/// 渲染输入框上方的小号字段标题。
fn form_label(cx: &Context<LumarkApp>, label: &str) -> gpui_kit::Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(label.to_string())
}

/// 渲染带说明文字的双栏设置行。
fn settings_row(
    cx: &Context<LumarkApp>,
    title: &str,
    description: &str,
    control: impl IntoElement,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_4()
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_sm().font_semibold().child(title.to_string()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description.to_string()),
                ),
        )
        .child(control)
}
