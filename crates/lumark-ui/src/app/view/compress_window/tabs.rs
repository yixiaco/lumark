//! 压缩窗口经典模式各选项卡。

use super::*;

impl CompressWindowView {
    /// 渲染“常规”选项卡。
    pub(super) fn render_general_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_field(
                "压缩文件名:",
                92.,
                Input::new(&self.output)
                    .id("compress-window-output-classic")
                    .w_full(),
            ))
            .child(classic_field(
                "压缩文件格式:",
                92.,
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
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .child(check_button(
                        cx,
                        "classic-test-after",
                        "压缩后测试压缩文件",
                        self.test_after,
                        |this, value, _| this.test_after = value,
                    ))
                    .child(check_button(
                        cx,
                        "classic-delete-source",
                        "压缩后删除源文件",
                        self.delete_source,
                        |this, value, _| this.delete_source = value,
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .when(
                        matches!(self.format, ArchiveFormat::Zip | ArchiveFormat::SevenZip),
                        |this| {
                            this.child(
                                Checkbox::new("classic-sfx")
                                    .label("自解压 EXE")
                                    .checked(self.sfx)
                                    .on_click(cx.listener(|this, value: &bool, window, cx| {
                                        this.set_sfx(*value, window, cx);
                                    })),
                            )
                        },
                    )
                    .when(self.sfx, |this| {
                        this.child(
                            button("classic-sfx-icon")
                                .xsmall()
                                .label(self.sfx_icon.as_ref().map_or("选择图标", |_| "更换图标"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.choose_sfx_icon(window, cx);
                                })),
                        )
                    })
                    .when(self.format == ArchiveFormat::SevenZip, |this| {
                        this.child(check_button(
                            cx,
                            "classic-solid",
                            "创建固实压缩文件",
                            self.solid,
                            |this, value, _| this.solid = value,
                        ))
                        .child(check_button(
                            cx,
                            "classic-encrypt-header",
                            "加密文件名",
                            self.encrypt_header,
                            |this, value, _| this.encrypt_header = value,
                        ))
                    })
                    .child(check_button(
                        cx,
                        "classic-shutdown",
                        "完成后关闭计算机",
                        self.shutdown_after,
                        |this, value, _| this.shutdown_after = value,
                    ))
                    .child(check_button(
                        cx,
                        "classic-low-priority",
                        "降低压缩优先级",
                        self.low_priority,
                        |this, value, _| this.low_priority = value,
                    )),
            )
            .child(classic_field("分卷大小:", 92., self.split_size_field(cx)))
            .child(hint_text(cx, self.split_hint(cx)))
            .into_any_element()
    }

    /// 渲染“密码”选项卡。
    pub(super) fn render_password_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_field(
                "压缩密码:",
                92.,
                Input::new(&self.password)
                    .id("compress-window-password-classic")
                    .w_full(),
            ))
            .child(hint_text(
                cx,
                "ZIP 使用 AES-256；7Z 设置密码后同时加密文件内容；ZPAQ 固定使用 AES-256（唯一加密算法）。",
            ))
            .when(matches!(self.format, ArchiveFormat::Zip | ArchiveFormat::SevenZip), |this| {
                this.child(check_button(
                    cx,
                    "classic-zipcrypto",
                    "使用 ZipCrypto 兼容加密",
                    self.zipcrypto,
                    |this, value, _| this.zipcrypto = value,
                ))
            })
            .into_any_element()
    }

    /// 渲染“文件”选项卡。
    pub(super) fn render_files_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_group(
                cx,
                "文件筛选",
                v_flex()
                    .gap_3()
                    .child(classic_field(
                        "排除:",
                        92.,
                        Input::new(&self.exclude).id("classic-exclude").w_full(),
                    ))
                    .child(classic_field(
                        "仅存储:",
                        92.,
                        Input::new(&self.store).id("classic-store").w_full(),
                    ))
                    .child(hint_text(
                        cx,
                        "多个通配符用 ; 分隔，例如 *.tmp; node_modules/*",
                    )),
            ))
            .child(classic_group(
                cx,
                "路径模式",
                h_flex()
                    .w_full()
                    .gap_4()
                    .child(radio_button(
                        cx,
                        "classic-path-relative",
                        "保留相对路径",
                        self.path_mode == PathMode::Relative,
                        |this, _, _, _| this.path_mode = PathMode::Relative,
                    ))
                    .child(radio_button(
                        cx,
                        "classic-path-noroot",
                        "去掉顶层目录",
                        self.path_mode == PathMode::NoRoot,
                        |this, _, _, _| this.path_mode = PathMode::NoRoot,
                    ))
                    .child(radio_button(
                        cx,
                        "classic-path-flatten",
                        "全部平铺",
                        self.path_mode == PathMode::Flatten,
                        |this, _, _, _| this.path_mode = PathMode::Flatten,
                    )),
            ))
            .into_any_element()
    }

    /// 渲染“算法”选项卡：格式方法、字典、单词、固实块和线程数。
    pub(super) fn render_algorithm_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let zip = self.format == ArchiveFormat::Zip;
        let seven = self.format == ArchiveFormat::SevenZip;
        let zpaq = self.format == ArchiveFormat::Zpaq;
        v_flex()
            .w_full()
            .gap_3()
            .when(zip || seven, |this| {
                this.child(classic_group(
                    cx,
                    "压缩算法",
                    v_flex()
                        .gap_3()
                        .child(classic_field(
                            "压缩方法:",
                            92.,
                            if zip {
                                choice_select(
                                    "compress-zip-method-select",
                                    &self.zip_method_select,
                                    220.,
                                    "ZIP 压缩方法",
                                )
                            } else {
                                choice_select(
                                    "compress-seven-method-select",
                                    &self.seven_method_select,
                                    220.,
                                    "7Z 压缩方法",
                                )
                            },
                        ))
                        .when(seven, |this| {
                            this.child(
                                h_flex()
                                    .w_full()
                                    .gap_4()
                                    .child(classic_field(
                                        "字典大小:",
                                        92.,
                                        choice_select(
                                            "compress-dictionary-select",
                                            &self.dictionary_select,
                                            160.,
                                            "字典大小",
                                        ),
                                    ))
                                    .child(classic_field(
                                        "单词大小:",
                                        92.,
                                        choice_select(
                                            "compress-word-select",
                                            &self.word_select,
                                            120.,
                                            "单词大小",
                                        ),
                                    )),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_4()
                                    .child(classic_field(
                                        "固实块大小:",
                                        92.,
                                        choice_select(
                                            "compress-solid-select",
                                            &self.solid_select,
                                            160.,
                                            "固实块大小",
                                        ),
                                    ))
                                    .child(classic_field(
                                        "线程个数:",
                                        92.,
                                        choice_select(
                                            "compress-thread-select",
                                            &self.thread_select,
                                            120.,
                                            "线程个数",
                                        ),
                                    )),
                            )
                        })
                        .child(hint_text(cx, self.memory_hint())),
                ))
            })
            .when(zpaq, |this| {
                this.child(classic_group(
                    cx,
                    "ZPAQ 压缩",
                    v_flex()
                        .gap_3()
                        .child(classic_field(
                            "压缩级别:",
                            92.,
                            choice_select(
                                "compress-zpaq-method-select",
                                &self.zpaq_method_select,
                                220.,
                                "ZPAQ 压缩级别",
                            ),
                        ))
                        .child(hint_text(
                            cx,
                            "级别越高压缩率越高、耗时越长；轻巧模式的“体积小”对应 5 级，4/5 级明显更慢。密码固定使用 AES-256 加密（ZPAQ 唯一算法）。“当前/最近”归档时间会复制全部源文件以改写时间，默认保留原始时间。",
                        )),
                ))
            })
            .when(!zip && !seven && !zpaq, |this| {
                this.child(classic_group(
                    cx,
                    "压缩算法",
                    hint_text(cx, "该格式使用固定算法；可在“常规”页调整压缩预设。"),
                ))
            })
            .into_any_element()
    }

    /// 渲染“时间”选项卡。
    pub(super) fn render_time_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_group(
                cx,
                "文件处理",
                v_flex()
                    .gap_3()
                    .child(classic_field(
                        "包含文件:",
                        92.,
                        choice_select(
                            "compress-filter-select",
                            &self.filter_select,
                            200.,
                            "文件时间筛选",
                        ),
                    ))
                    .when(
                        matches!(
                            self.file_time_filter,
                            FileTimeFilter::After | FileTimeFilter::Range
                        ),
                        |this| {
                            this.child(classic_field(
                                "晚于:",
                                92.,
                                Input::new(&self.after).id("classic-after").w_full(),
                            ))
                        },
                    )
                    .when(
                        matches!(
                            self.file_time_filter,
                            FileTimeFilter::Before | FileTimeFilter::Range
                        ),
                        |this| {
                            this.child(classic_field(
                                "早于:",
                                92.,
                                Input::new(&self.before).id("classic-before").w_full(),
                            ))
                        },
                    )
                    .child(hint_text(cx, "日期格式：YYYY-MM-DD。")),
            ))
            .child(classic_group(
                cx,
                "压缩文件时间",
                v_flex()
                    .gap_3()
                    .child(classic_field(
                        "设置时间:",
                        92.,
                        choice_select(
                            "compress-archive-time-select",
                            &self.archive_time_select,
                            200.,
                            "压缩文件时间",
                        ),
                    ))
                    .child(hint_text(
                        cx,
                        "当前系统时间会统一写入新时间；原始时间保留源文件时间。",
                    )),
            ))
            .into_any_element()
    }

    /// 渲染“注释”选项卡，使用官方多行 Textarea。
    pub(super) fn render_comment_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_group(
                cx,
                "压缩包注释",
                v_flex()
                    .gap_2()
                    .child(
                        Textarea::new(&self.comment)
                            .h(px(260.))
                            .bordered(true)
                            .aria_label("压缩包注释"),
                    )
                    .child(hint_text(
                        cx,
                        "ZIP 注释在压缩包属性中显示；7Z 支持多行注释。",
                    )),
            ))
            .into_any_element()
    }
}
