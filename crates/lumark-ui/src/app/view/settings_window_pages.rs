//! 设置窗口的综合、外观和关于选项卡。
use super::super::*;
use super::components::AppIcon;
use super::settings_window::{
    SettingsWindowView, settings_card, settings_divider, settings_row, settings_section,
};
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
                        "文件列表",
                        Some("控制主界面文件表的密度和侧栏信息".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "显示详细信息区",
                        "在文件侧栏底部显示大小、时间、CRC 和压缩方法。",
                        button("settings-toggle-details")
                            .label(if settings.details_visible {
                                "已显示"
                            } else {
                                "已隐藏"
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
                        "文件列表列宽",
                        "在紧凑、标准和宽松三档列宽之间切换。",
                        button("settings-cycle-columns")
                            .label(match settings.column_layout {
                                0 => "紧凑列",
                                2 => "宽松列",
                                _ => "标准列",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_app(cx, |app, cx| {
                                    app.cycle_column_layout();
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
                        "解压行为",
                        Some("新任务会继承这里的默认值".to_string()),
                    ))
                    .child(settings_row(
                        cx,
                        "解压后打开目录",
                        "任务完成后直接定位到输出位置。",
                        button("settings-toggle-open-after")
                            .label(if settings.open_after_extract {
                                "已开启"
                            } else {
                                "已关闭"
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
            ThemePreference::System => "跟随系统",
            ThemePreference::Light => "浅色",
            ThemePreference::Dark => "深色",
        };
        settings_card(cx)
            .p_4()
            .gap_3()
            .child(settings_section(
                cx,
                AppIcon::Tool,
                "主题与强调色",
                Some("界面会随系统外观自动切换".to_string()),
            ))
            .child(settings_row(
                cx,
                "主题",
                "浅色、深色或跟随 Windows。",
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
                "主题色",
                "强调色默认蓝色，点击色块可选择任意颜色。",
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        ColorPicker::new(&self.accent_picker)
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
    }

    /// 渲染“关于”选项卡：版本、设置文件和密码库位置。
    pub(super) fn render_about_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        settings_card(cx)
            .p_4()
            .gap_3()
            .child(settings_section(cx, AppIcon::Shield, "关于 Lumark", None))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("本地优先的压缩管理器 · Rust + GPUI · v0.1.0"),
            )
            .child(settings_divider(cx))
            .child(info_row(
                cx,
                "设置文件",
                &self.settings_path.display().to_string(),
            ))
            .child(info_row(
                cx,
                "密码库文件",
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
