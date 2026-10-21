//! 设置页面：外观、压缩偏好、密码库位置和 Windows 集成。
use super::super::super::*;
use super::super::components::{AppIcon, card, section_heading};
use super::shared::stat_card;
use gpui_kit::component::scroll::ScrollableElement;

impl LumarkApp {
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
        let vault_location = format!(
            "{}{}",
            self.vault.path().display(),
            if self.vault_error.is_some() {
                " · 临时位置"
            } else if settings.vault_path.is_some() {
                " · 自定义位置"
            } else {
                " · 默认位置"
            }
        );
        v_flex()
            .id("settings-page")
            .w_full()
            .h_full()
            .min_h_0()
            .overflow_y_scrollbar()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .w_full()
                    .child(stat_card(
                        cx,
                        AppIcon::Tool,
                        "界面主题",
                        theme_label,
                    ))
                    .child(stat_card(
                        cx,
                        AppIcon::Compression,
                        "默认格式",
                        &settings.default_format.to_ascii_uppercase(),
                    ))
                    .child(stat_card(
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
                                "密码库使用 XChaCha20-Poly1305 加密；未设置主密码时由与当前计算机绑定的 Windows DPAPI 包裹。设置主密码后会移除 DPAPI 包裹，忘记主密码无法恢复。",
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
                        &vault_location,
                        h_flex()
                            .gap_2()
                            .child(
                                button("settings-vault-location")
                                    .icon(AppIcon::FolderOpen.element(15.))
                                    .label("更改位置")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_vault_location_dialog(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
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
                            ),
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
                        .text_ellipsis()
                        .child(description.to_string()),
                ),
        )
        .child(control)
}
