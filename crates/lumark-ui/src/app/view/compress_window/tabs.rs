//! 压缩窗口经典模式各选项卡。

use super::*;
use crate::i18n::tr;

impl CompressWindowView {
    /// 渲染“常规”选项卡。
    pub(super) fn render_general_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_field(
                tr("压缩文件名:", "Archive name:"),
                92.,
                Input::new(&self.output)
                    .id("compress-window-output-classic")
                    .w_full(),
            ))
            .child(classic_field(
                tr("压缩文件格式:", "Archive format:"),
                92.,
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(choice_select(
                        "compress-format-select",
                        &self.format_select,
                        200.,
                        tr("压缩格式", "Archive format"),
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
                        tr("压缩后测试压缩文件", "Test archive after compression"),
                        self.test_after,
                        |this, value, _| this.test_after = value,
                    ))
                    .child(check_button(
                        cx,
                        "classic-delete-source",
                        tr("压缩后删除源文件", "Delete source files after compression"),
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
                                    .label(tr("自解压 EXE", "Self-extracting EXE"))
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
                                .label(
                                    self.sfx_icon
                                        .as_ref()
                                        .map_or(tr("选择图标", "Choose icon"), |_| {
                                            tr("更换图标", "Change icon")
                                        }),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.choose_sfx_icon(window, cx);
                                })),
                        )
                    })
                    .when(self.format == ArchiveFormat::SevenZip, |this| {
                        this.child(check_button(
                            cx,
                            "classic-solid",
                            tr("创建固实压缩文件", "Create solid archive"),
                            self.solid,
                            |this, value, _| this.solid = value,
                        ))
                        .child(check_button(
                            cx,
                            "classic-encrypt-header",
                            tr("加密文件名", "Encrypt file names"),
                            self.encrypt_header,
                            |this, value, _| this.encrypt_header = value,
                        ))
                    })
                    .child(check_button(
                        cx,
                        "classic-shutdown",
                        tr("完成后关闭计算机", "Shut down after completion"),
                        self.shutdown_after,
                        |this, value, _| this.shutdown_after = value,
                    ))
                    .child(check_button(
                        cx,
                        "classic-low-priority",
                        tr("降低压缩优先级", "Lower compression priority"),
                        self.low_priority,
                        |this, value, _| this.low_priority = value,
                    )),
            )
            .child(classic_field(
                tr("分卷大小:", "Volume size:"),
                92.,
                self.split_size_field(cx),
            ))
            .child(hint_text(cx, self.split_hint(cx)))
            .into_any_element()
    }

    /// 渲染“密码”选项卡。
    pub(super) fn render_password_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_field(
                tr("压缩密码:", "Archive password:"),
                92.,
                Input::new(&self.password)
                    .id("compress-window-password-classic")
                    .w_full(),
            ))
            .child(hint_text(
                cx,
                tr("ZIP 使用 AES-256；7Z 设置密码后同时加密文件内容；ZPAQ 固定使用 AES-256（唯一加密算法）。", "ZIP uses AES-256; 7Z encrypts file contents when a password is set; ZPAQ always uses AES-256, its only algorithm."),
            ))
            .when(matches!(self.format, ArchiveFormat::Zip | ArchiveFormat::SevenZip), |this| {
                this.child(check_button(
                    cx,
                    "classic-zipcrypto",
                    tr("使用 ZipCrypto 兼容加密", "Use ZipCrypto-compatible encryption"),
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
                tr("文件筛选", "File filter"),
                v_flex()
                    .gap_3()
                    .child(classic_field(
                        tr("排除:", "Exclude:"),
                        92.,
                        Input::new(&self.exclude).id("classic-exclude").w_full(),
                    ))
                    .child(classic_field(
                        tr("仅存储:", "Store only:"),
                        92.,
                        Input::new(&self.store).id("classic-store").w_full(),
                    ))
                    .child(hint_text(
                        cx,
                        tr(
                            "多个通配符用 ; 分隔，例如 *.tmp; node_modules/*",
                            "Separate wildcards with ;, e.g. *.tmp; node_modules/*",
                        ),
                    )),
            ))
            .child(classic_group(
                cx,
                tr("路径模式", "Path mode"),
                h_flex()
                    .w_full()
                    .gap_4()
                    .child(radio_button(
                        cx,
                        "classic-path-relative",
                        tr("保留相对路径", "Keep relative paths"),
                        self.path_mode == PathMode::Relative,
                        |this, _, _, _| this.path_mode = PathMode::Relative,
                    ))
                    .child(radio_button(
                        cx,
                        "classic-path-noroot",
                        tr("去掉顶层目录", "Remove top-level folder"),
                        self.path_mode == PathMode::NoRoot,
                        |this, _, _, _| this.path_mode = PathMode::NoRoot,
                    ))
                    .child(radio_button(
                        cx,
                        "classic-path-flatten",
                        tr("全部平铺", "Flatten all"),
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
                    tr("压缩算法", "Compression algorithm"),
                    v_flex()
                        .gap_3()
                        .child(classic_field(
                            tr("压缩方法:", "Method:"),
                            92.,
                            if zip {
                                choice_select(
                                    "compress-zip-method-select",
                                    &self.zip_method_select,
                                    220.,
                                    tr("ZIP 压缩方法", "ZIP method"),
                                )
                            } else {
                                choice_select(
                                    "compress-seven-method-select",
                                    &self.seven_method_select,
                                    220.,
                                    tr("7Z 压缩方法", "7Z method"),
                                )
                            },
                        ))
                        .when(seven, |this| {
                            this.child(
                                h_flex()
                                    .w_full()
                                    .gap_4()
                                    .child(classic_field(
                                        tr("字典大小:", "Dictionary size:"),
                                        92.,
                                        choice_select(
                                            "compress-dictionary-select",
                                            &self.dictionary_select,
                                            160.,
                                            tr("字典大小", "Dictionary size"),
                                        ),
                                    ))
                                    .child(classic_field(
                                        tr("单词大小:", "Word size:"),
                                        92.,
                                        choice_select(
                                            "compress-word-select",
                                            &self.word_select,
                                            120.,
                                            tr("单词大小", "Word size"),
                                        ),
                                    )),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_4()
                                    .child(classic_field(
                                        tr("固实块大小:", "Solid block size:"),
                                        92.,
                                        choice_select(
                                            "compress-solid-select",
                                            &self.solid_select,
                                            160.,
                                            tr("固实块大小", "Solid block size"),
                                        ),
                                    ))
                                    .child(classic_field(
                                        tr("线程个数:", "Threads:"),
                                        92.,
                                        choice_select(
                                            "compress-thread-select",
                                            &self.thread_select,
                                            120.,
                                            tr("线程个数", "Threads"),
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
                    tr("ZPAQ 压缩", "ZPAQ compression"),
                    v_flex()
                        .gap_3()
                        .child(classic_field(
                            tr("压缩级别:", "Level:"),
                            92.,
                            choice_select(
                                "compress-zpaq-method-select",
                                &self.zpaq_method_select,
                                220.,
                                tr("ZPAQ 压缩级别", "ZPAQ level"),
                            ),
                        ))
                        .child(hint_text(
                            cx,
                            tr("级别越高压缩率越高、耗时越长；轻巧模式的“体积小”对应 5 级，4/5 级明显更慢。密码固定使用 AES-256 加密（ZPAQ 唯一算法）。“当前/最近”归档时间会复制全部源文件以改写时间，默认保留原始时间。", "Higher levels compress better but take longer; Smaller maps to level 5 and levels 4/5 are much slower. Passwords always use AES-256, ZPAQ's only algorithm. Current/Latest archive time copies all sources to rewrite timestamps; original time is kept by default."),
                        )),
                ))
            })
            .when(!zip && !seven && !zpaq, |this| {
                this.child(classic_group(
                    cx,
                    tr("压缩算法", "Compression algorithm"),
                    hint_text(cx, tr("该格式使用固定算法；可在“常规”页调整压缩预设。", "This format uses a fixed algorithm; adjust the compression preset on the General tab.")),
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
                tr("文件处理", "File handling"),
                v_flex()
                    .gap_3()
                    .child(classic_field(
                        tr("包含文件:", "Include files:"),
                        92.,
                        choice_select(
                            "compress-filter-select",
                            &self.filter_select,
                            200.,
                            tr("文件时间筛选", "File time filter"),
                        ),
                    ))
                    .when(
                        matches!(
                            self.file_time_filter,
                            FileTimeFilter::After | FileTimeFilter::Range
                        ),
                        |this| {
                            this.child(classic_field(
                                tr("晚于:", "After:"),
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
                                tr("早于:", "Before:"),
                                92.,
                                Input::new(&self.before).id("classic-before").w_full(),
                            ))
                        },
                    )
                    .child(hint_text(cx, tr("日期格式：YYYY-MM-DD。", "Date format: YYYY-MM-DD."))),
            ))
            .child(classic_group(
                cx,
                tr("压缩文件时间", "Archive timestamp"),
                v_flex()
                    .gap_3()
                    .child(classic_field(
                        tr("设置时间:", "Set time:"),
                        92.,
                        choice_select(
                            "compress-archive-time-select",
                            &self.archive_time_select,
                            200.,
                            tr("压缩文件时间", "Archive timestamp"),
                        ),
                    ))
                    .child(hint_text(
                        cx,
                        tr("当前系统时间会统一写入新时间；原始时间保留源文件时间。", "Current system time writes a new timestamp; original time keeps source timestamps."),
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
                tr("压缩包注释", "Archive comment"),
                v_flex()
                    .gap_2()
                    .child(
                        Textarea::new(&self.comment)
                            .h(px(260.))
                            .bordered(true)
                            .aria_label(tr("压缩包注释", "Archive comment")),
                    )
                    .child(hint_text(
                        cx,
                        tr("ZIP 注释在压缩包属性中显示；7Z 支持多行注释。", "ZIP comments appear in archive properties; 7Z supports multi-line comments."),
                    )),
            ))
            .into_any_element()
    }
}
