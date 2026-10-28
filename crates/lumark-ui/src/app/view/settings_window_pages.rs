//! 设置窗口的综合、外观和关于选项卡。
use super::super::*;
use super::components::AppIcon;
use super::settings_window::{
    SettingsWindowView, settings_card, settings_divider, settings_row, settings_section,
};
use crate::i18n::tr;
use gpui_kit::component::color_picker::ColorPicker;

impl SettingsWindowView {
    /// 渲染“综合”选项卡：文件列表和详细信息区偏好。
    pub(super) fn render_general_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                        AppIcon::Grid,
                        tr("文件列表", "File list"),
                        Some(tr("控制主界面文件表的密度和侧栏信息", "Control file-table density and sidebar information").to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        tr("界面语言", "Interface language"),
                        tr("动作带等核心文案随语言切换；其余页面将逐步迁移。", "Core action-band text follows the language; other pages are being migrated."),
                        button("settings-toggle-language")
                            .label(if settings.language.starts_with("en") {
                                "English"
                            } else {
                                "简体中文"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.toggle_language();
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("显示详细信息区", "Show details pane"),
                        tr("在文件侧栏底部显示大小、时间、CRC 和压缩方法。", "Show size, time, CRC and method at the bottom of the sidebar."),
                        button("settings-toggle-details")
                            .label(if settings.details_visible {
                                tr("已显示", "Shown")
                            } else {
                                tr("已隐藏", "Hidden")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.toggle_details();
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_divider(cx))
                    .child(settings_row(
                        cx,
                        tr("文件列表列宽", "File list column width"),
                        tr("在紧凑、标准和宽松三档列宽之间切换。", "Switch between compact, standard and relaxed column widths."),
                        button("settings-cycle-columns")
                            .label(match settings.column_layout {
                                0 => tr("紧凑列", "Compact"),
                                2 => tr("宽松列", "Relaxed"),
                                _ => tr("标准列", "Standard"),
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.cycle_column_layout();
                                    cx.notify();
                                });
                            })),
                    ))
                    .child(settings_row(
                        cx,
                        tr("自定义列宽", "Custom column widths"),
                        tr("拖动表头分隔线调整列宽；这里清除拖动结果并恢复当前预设。", "Drag header dividers to resize; reset clears custom widths."),
                        button("settings-reset-columns")
                            .label(tr("重置列宽", "Reset columns"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.reset_column_widths();
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
                        AppIcon::FolderOpen,
                        tr("解压行为", "Extraction behavior"),
                        Some(tr("新任务会继承这里的默认值", "New tasks inherit these defaults").to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        tr("解压后打开目录", "Open folder after extraction"),
                        tr("任务完成后直接定位到输出位置。", "Open the output location after completion."),
                        button("settings-toggle-open-after")
                            .label(if settings.open_after_extract {
                                tr("已开启", "On")
                            } else {
                                tr("已关闭", "Off")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.toggle_open_after_extract();
                                    cx.notify();
                                });
                            })),
                    )),
            )
    }

    /// 渲染“外观”选项卡：主题和强调色选择。
    pub(super) fn render_appearance_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        let theme_label = match settings.theme {
            ThemePreference::System => tr("跟随系统", "System"),
            ThemePreference::Light => tr("浅色", "Light"),
            ThemePreference::Dark => tr("深色", "Dark"),
        };
        settings_card(cx)
            .p_4()
            .gap_3()
            .child(settings_section(
                cx,
                AppIcon::Tool,
                tr("主题与强调色", "Theme and accent"),
                Some(
                    tr(
                        "界面会随系统外观自动切换",
                        "The UI follows the system appearance",
                    )
                    .to_string(),
                ),
            ))
            .child(settings_row(
                cx,
                tr("主题", "Theme"),
                tr(
                    "浅色、深色或跟随 Windows。",
                    "Light, dark or follow Windows.",
                ),
                button("settings-cycle-theme")
                    .label(theme_label)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.app.update(cx, |app, cx| {
                            app.cycle_theme(window, cx);
                            cx.notify();
                        });
                        this.refresh(cx);
                        cx.notify();
                    })),
            ))
            .child(settings_divider(cx))
            .child(settings_row(
                cx,
                tr("主题色", "Accent color"),
                tr(
                    "强调色默认蓝色，点击色块可选择任意颜色。",
                    "Accent defaults to blue; click the swatch to choose any color.",
                ),
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        ColorPicker::new(&self.accent_picker)
                            .featured_colors(theme_accent_presets())
                            .small()
                            .accessibility_label(tr("主题色", "Accent color")),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(settings.accent_color.clone()),
                    ),
            ))
    }

    /// 渲染“关于”选项卡：版本、设置文件和密码库位置。
    pub(super) fn render_about_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        settings_card(cx)
            .p_4()
            .gap_3()
            .child(settings_section(
                cx,
                AppIcon::Shield,
                tr("关于 Lumark", "About Lumark"),
                None,
            ))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(
                        tr(
                            "本地优先的压缩管理器 · Rust + GPUI · v0.1.0",
                            "Local-first archive manager · Rust + GPUI · v0.1.0",
                        ),
                        "Local-first archive manager · Rust + GPUI · v0.1.0",
                    )),
            )
            .child(settings_divider(cx))
            .child(info_row(
                cx,
                tr("设置文件", "Settings file"),
                &self.settings_path.display().to_string(),
            ))
            .child(info_row(
                cx,
                tr("密码库文件", "Vault file"),
                &self.vault_path.display().to_string(),
            ))
    }
}

/// 渲染关于页中的只读路径行。
fn info_row(cx: &gpui_kit::App, title: &str, value: &str) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(div().text_sm().font_semibold().child(title.to_string()))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(value.to_string()),
        )
}
