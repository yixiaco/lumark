//! 压缩窗口轻巧排版。

use super::*;

impl CompressWindowView {
    /// 渲染轻巧模式的完整表单。
    pub(super) fn render_light(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let mut content = v_flex()
            .flex_1()
            .min_h_0()
            .px_5()
            .py_4()
            .gap_3()
            .child(classic_field(
                "压缩到:",
                66.,
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Input::new(&self.output)
                            .id("compress-window-output")
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        button("compress-window-browse")
                            .ghost()
                            .cursor_pointer()
                            .label("浏览…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_directory(window, cx);
                            })),
                    ),
            ))
            .child(classic_field(
                "格式:",
                66.,
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(choice_select(
                        "compress-format-select",
                        &self.format_select,
                        200.,
                        "压缩格式",
                    ))
                    .child(hint_text(cx, self.format_hint())),
            ))
            .child(classic_field(
                "压缩配置:",
                66.,
                h_flex()
                    .w_full()
                    .gap_4()
                    .child(radio_button(
                        cx,
                        "compress-window-preset-speed",
                        "速度快",
                        self.preset == SettingsPreset::Speed,
                        |this, _, window, cx| this.set_preset(SettingsPreset::Speed, window, cx),
                    ))
                    .child(radio_button(
                        cx,
                        "compress-window-preset-balanced",
                        "平衡",
                        self.preset == SettingsPreset::Balanced,
                        |this, _, window, cx| this.set_preset(SettingsPreset::Balanced, window, cx),
                    ))
                    .child(radio_button(
                        cx,
                        "compress-window-preset-size",
                        "体积小",
                        self.preset == SettingsPreset::Size,
                        |this, _, window, cx| this.set_preset(SettingsPreset::Size, window, cx),
                    )),
            ))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_center()
                    .child(check_button(
                        cx,
                        "compress-window-test-after",
                        "压缩后测试",
                        self.test_after,
                        |this, value, _| this.test_after = value,
                    ))
                    .child(check_button(
                        cx,
                        "compress-window-delete-source",
                        "成功后删除源文件",
                        self.delete_source,
                        |this, value, _| this.delete_source = value,
                    ))
                    .when(self.format == ArchiveFormat::SevenZip, |this| {
                        this.child(check_button(
                            cx,
                            "compress-window-solid",
                            "创建固实压缩文件",
                            self.solid,
                            |this, value, _| this.solid = value,
                        ))
                        .child(check_button(
                            cx,
                            "compress-window-encrypt-header",
                            "加密文件名",
                            self.encrypt_header,
                            |this, value, _| this.encrypt_header = value,
                        ))
                    }),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_center()
                    .when(
                        matches!(self.format, ArchiveFormat::Zip | ArchiveFormat::SevenZip),
                        |this| {
                            this.child(
                                Checkbox::new("compress-window-sfx")
                                    .label("自解压 EXE")
                                    .checked(self.sfx)
                                    .on_click(cx.listener(|this, value: &bool, window, cx| {
                                        this.set_sfx(*value, window, cx);
                                    })),
                            )
                        },
                    )
                    .child(
                        Checkbox::new("compress-window-disguise")
                            .label("伪装为媒体文件")
                            .checked(self.disguise)
                            .on_click(cx.listener(|this, value: &bool, window, cx| {
                                this.set_disguise(*value, window, cx);
                            })),
                    )
                    .child(check_button(
                        cx,
                        "compress-window-shutdown",
                        "完成后关闭计算机",
                        self.shutdown_after,
                        |this, value, _| this.shutdown_after = value,
                    ))
                    .child(check_button(
                        cx,
                        "compress-window-low-priority",
                        "降低压缩优先级",
                        self.low_priority,
                        |this, value, _| this.low_priority = value,
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        button("compress-window-password-toggle")
                            .link()
                            .xsmall()
                            .cursor_pointer()
                            .icon(AppIcon::Key.element(13.))
                            .label(if self.password_visible {
                                "取消密码"
                            } else {
                                "设置密码"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.password_visible = !this.password_visible;
                                if !this.password_visible {
                                    set_input(&this.password, "", window, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .when(self.password_visible, |this| {
                        this.child(
                            Input::new(&self.password)
                                .id("compress-window-password")
                                .flex_1()
                                .min_w_0(),
                        )
                    }),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger_foreground)
                        .child(error),
                )
            })
            .child(div().flex_1());
        content = content.child(self.render_footer(cx, false));
        content.into_any_element()
    }
}
