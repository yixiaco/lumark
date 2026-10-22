use super::super::*;
use super::components::{
    AppIcon, choice_select, classic_field, classic_group, dialog_banner, hint_text, segmented_tab,
    segmented_tab_bar,
};
use gpui_kit::Entity;
use gpui_kit::Subscription;
use gpui_kit::component::IndexPath;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use std::path::PathBuf;

/// 轻巧模式默认窗口尺寸，同时也是整个压缩窗口的最小尺寸。
const LIGHT_WINDOW_SIZE: (f32, f32) = (600., 330.);

/// 经典模式默认窗口尺寸；切换布局时恢复到对应模式的尺寸。
const CLASSIC_WINDOW_SIZE: (f32, f32) = (600., 390.);

/// 独立压缩窗口的初始参数快照。
///
/// 主应用在打开窗口时复制这些值，窗口内的所有修改都只作用于自己的状态，
/// 直到用户点击“立即压缩/确定”才提交给主应用。
#[derive(Clone)]
pub(crate) struct CompressInit {
    pub format: ArchiveFormat,
    pub preset: SettingsPreset,
    pub solid: bool,
    pub encrypt_header: bool,
    pub zipcrypto: bool,
    pub test_after: bool,
    pub delete_source: bool,
    /// 生成自解压 EXE；只在 ZIP 格式下生效。
    pub sfx: bool,
    pub path_mode: PathMode,
    pub threads: u32,
    pub zip_compression: ZipCompression,
    pub sevenz_compression: SevenZCompression,
    pub dictionary_mb: u32,
    pub word_size: u32,
    pub solid_block_mb: u32,
    pub file_time_filter: FileTimeFilter,
    pub archive_time: ArchiveTimeSource,
    pub classic: bool,
}

/// 独立压缩窗口的视图状态。
pub(crate) struct CompressWindowView {
    app: Entity<LumarkApp>,
    sources: Vec<PathBuf>,
    output: Entity<InputState>,
    password: Entity<InputState>,
    comment: Entity<TextareaState>,
    exclude: Entity<InputState>,
    store: Entity<InputState>,
    split: Entity<InputState>,
    after: Entity<InputState>,
    before: Entity<InputState>,
    format: ArchiveFormat,
    preset: SettingsPreset,
    solid: bool,
    encrypt_header: bool,
    zipcrypto: bool,
    test_after: bool,
    delete_source: bool,
    sfx: bool,
    /// 压缩成功后安排系统关机；每次打开窗口都从关闭状态开始。
    shutdown_after: bool,
    /// 压缩期间降低进程优先级，减少界面卡顿。
    low_priority: bool,
    path_mode: PathMode,
    classic: bool,
    tab: u8,
    password_visible: bool,
    error: Option<String>,
    zip_compression: ZipCompression,
    sevenz_compression: SevenZCompression,
    dictionary_mb: u32,
    word_size: u32,
    solid_block_mb: u32,
    threads: u32,
    file_time_filter: FileTimeFilter,
    archive_time: ArchiveTimeSource,
    format_select: ChoiceState,
    zip_method_select: ChoiceState,
    seven_method_select: ChoiceState,
    dictionary_select: ChoiceState,
    word_select: ChoiceState,
    solid_select: ChoiceState,
    thread_select: ChoiceState,
    filter_select: ChoiceState,
    archive_time_select: ChoiceState,
    _select_subscriptions: Vec<Subscription>,
}

type ChoiceState = Entity<SelectState<SearchableVec<&'static str>>>;

const ZIP_METHOD_LABELS: [&str; 3] = ["Deflate（推荐）", "BZip2", "LZMA"];
const SEVEN_METHOD_LABELS: [&str; 4] = ["LZMA2（推荐）", "LZMA", "PPMd", "BZip2"];
const DICTIONARY_LABELS: [&str; 17] = [
    "1 MB", "2 MB", "4 MB", "8 MB", "16 MB", "24 MB", "32 MB", "48 MB", "64 MB", "96 MB", "128 MB",
    "192 MB", "256 MB", "384 MB", "512 MB", "768 MB", "1024 MB",
];
const WORD_LABELS: [&str; 12] = [
    "8", "12", "16", "24", "32", "48", "64", "96", "128", "192", "256", "273",
];
const SOLID_LABELS: [&str; 17] = [
    "1 MB", "2 MB", "4 MB", "8 MB", "16 MB", "32 MB", "64 MB", "128 MB", "256 MB", "512 MB",
    "1 GB", "2 GB", "4 GB", "8 GB", "16 GB", "32 GB", "64 GB",
];
const THREAD_LABELS: [&str; 32] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
];
const FILTER_LABELS: [&str; 4] = ["任意时间", "早于", "晚于", "日期范围"];
const ARCHIVE_TIME_LABELS: [&str; 3] = ["当前系统时间", "原始压缩时间", "最近文件时间"];

const WRITABLE_FORMATS: &[(ArchiveFormat, &str)] = &[
    (ArchiveFormat::Zip, "ZIP"),
    (ArchiveFormat::SevenZip, "7Z"),
    (ArchiveFormat::Tar, "TAR"),
    (ArchiveFormat::TarGz, "TAR.GZ"),
    (ArchiveFormat::TarBz2, "TAR.BZ2"),
    (ArchiveFormat::TarXz, "TAR.XZ"),
    (ArchiveFormat::TarZst, "TAR.ZST"),
    (ArchiveFormat::Gzip, "GZ"),
    (ArchiveFormat::Bzip2, "BZ2"),
    (ArchiveFormat::Xz, "XZ"),
    (ArchiveFormat::Zstd, "ZST"),
];

impl CompressWindowView {
    /// 创建窗口状态，并初始化注释多行文本框。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        app: Entity<LumarkApp>,
        sources: Vec<PathBuf>,
        output: Entity<InputState>,
        password: Entity<InputState>,
        exclude: Entity<InputState>,
        store: Entity<InputState>,
        split: Entity<InputState>,
        after: Entity<InputState>,
        before: Entity<InputState>,
        init: CompressInit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let comment = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(5)
                .placeholder("压缩包注释（可选，支持多行）")
        });
        let format_index = WRITABLE_FORMATS
            .iter()
            .position(|(format, _)| *format == init.format)
            .unwrap_or(0);
        let zip_method_index = match init.zip_compression {
            ZipCompression::Deflate => 0,
            ZipCompression::Bzip2 => 1,
            ZipCompression::Lzma => 2,
        };
        let seven_method_index = match init.sevenz_compression {
            SevenZCompression::Lzma2 => 0,
            SevenZCompression::Lzma => 1,
            SevenZCompression::Ppmd => 2,
            SevenZCompression::Bzip2 => 3,
        };
        let dictionary_label = format!("{} MB", init.dictionary_mb);
        let dictionary_index = DICTIONARY_LABELS
            .iter()
            .position(|label| *label == dictionary_label)
            .unwrap_or(4);
        let word_index = WORD_LABELS
            .iter()
            .position(|label| *label == init.word_size.to_string())
            .unwrap_or(4);
        let solid_label = solid_label_for(init.solid_block_mb);
        let solid_index = SOLID_LABELS
            .iter()
            .position(|label| *label == solid_label)
            .unwrap_or(11);
        let thread_index = init.threads.saturating_sub(1).min(31) as usize;
        let filter_index = match init.file_time_filter {
            FileTimeFilter::Any => 0,
            FileTimeFilter::Before => 1,
            FileTimeFilter::After => 2,
            FileTimeFilter::Range => 3,
        };
        let archive_time_index = match init.archive_time {
            ArchiveTimeSource::Current => 0,
            ArchiveTimeSource::Original => 1,
            ArchiveTimeSource::Latest => 2,
        };
        let format_select = new_select(
            window,
            cx,
            WRITABLE_FORMATS.iter().map(|(_, label)| *label).collect(),
            format_index,
        );
        let zip_method_select =
            new_select(window, cx, ZIP_METHOD_LABELS.to_vec(), zip_method_index);
        let seven_method_select =
            new_select(window, cx, SEVEN_METHOD_LABELS.to_vec(), seven_method_index);
        let dictionary_select =
            new_select(window, cx, DICTIONARY_LABELS.to_vec(), dictionary_index);
        let word_select = new_select(window, cx, WORD_LABELS.to_vec(), word_index);
        let solid_select = new_select(window, cx, SOLID_LABELS.to_vec(), solid_index);
        let thread_select = new_select(window, cx, THREAD_LABELS.to_vec(), thread_index);
        let filter_select = new_select(window, cx, FILTER_LABELS.to_vec(), filter_index);
        let archive_time_select =
            new_select(window, cx, ARCHIVE_TIME_LABELS.to_vec(), archive_time_index);
        let mut select_subscriptions = Vec::new();
        select_subscriptions.push(cx.subscribe_in(
            &format_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some((format, _)) =
                        WRITABLE_FORMATS.iter().find(|(_, label)| *label == *value)
                {
                    this.set_format(*format, window, cx);
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &zip_method_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.zip_compression = match *value {
                        "BZip2" => ZipCompression::Bzip2,
                        "LZMA" => ZipCompression::Lzma,
                        _ => ZipCompression::Deflate,
                    };
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &seven_method_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.sevenz_compression = match *value {
                        "LZMA" => SevenZCompression::Lzma,
                        "PPMd" => SevenZCompression::Ppmd,
                        "BZip2" => SevenZCompression::Bzip2,
                        _ => SevenZCompression::Lzma2,
                    };
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &dictionary_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(number) = value.split_whitespace().next()
                    && let Ok(mb) = number.parse::<u32>()
                {
                    this.dictionary_mb = mb;
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &word_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Ok(word) = value.parse::<u32>()
                {
                    this.word_size = word;
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &solid_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(number) = value.split_whitespace().next()
                    && let Ok(amount) = number.parse::<u32>()
                {
                    this.solid_block_mb = if value.ends_with("GB") {
                        amount.saturating_mul(1024)
                    } else {
                        amount
                    };
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &thread_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Ok(threads) = value.parse::<u32>()
                {
                    this.threads = threads;
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &filter_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.file_time_filter = match *value {
                        "早于" => FileTimeFilter::Before,
                        "晚于" => FileTimeFilter::After,
                        "日期范围" => FileTimeFilter::Range,
                        _ => FileTimeFilter::Any,
                    };
                    cx.notify();
                }
            },
        ));
        select_subscriptions.push(cx.subscribe(
            &archive_time_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.archive_time = match *value {
                        "原始压缩时间" => ArchiveTimeSource::Original,
                        "最近文件时间" => ArchiveTimeSource::Latest,
                        _ => ArchiveTimeSource::Current,
                    };
                    cx.notify();
                }
            },
        ));
        let output_for_focus = output.clone();
        window.on_next_frame(move |window, cx| {
            output_for_focus.update(cx, |state, cx| {
                state.focus(window, cx);
            });
        });
        Self {
            app,
            sources,
            output,
            password,
            comment,
            exclude,
            store,
            split,
            after,
            before,
            format: init.format,
            preset: init.preset,
            solid: init.solid,
            encrypt_header: init.encrypt_header,
            zipcrypto: init.zipcrypto,
            test_after: init.test_after,
            delete_source: init.delete_source,
            sfx: init.sfx,
            shutdown_after: false,
            low_priority: false,
            path_mode: init.path_mode,
            classic: init.classic,
            tab: 0,
            password_visible: false,
            error: None,
            zip_compression: init.zip_compression,
            sevenz_compression: init.sevenz_compression,
            dictionary_mb: init.dictionary_mb,
            word_size: init.word_size,
            solid_block_mb: init.solid_block_mb,
            threads: init.threads,
            file_time_filter: init.file_time_filter,
            archive_time: init.archive_time,
            format_select,
            zip_method_select,
            seven_method_select,
            dictionary_select,
            word_select,
            solid_select,
            thread_select,
            filter_select,
            archive_time_select,
            _select_subscriptions: select_subscriptions,
        }
    }

    /// 刷新格式并把输出输入的扩展名同步过去。
    fn set_format(&mut self, format: ArchiveFormat, window: &mut Window, cx: &mut Context<Self>) {
        self.format = format;
        if format != ArchiveFormat::Zip {
            // 自解压运行时只实现 ZIP 解压，切换格式时自动取消勾选。
            self.sfx = false;
        }
        let current = PathBuf::from(input_value(&self.output, cx));
        if !current.as_os_str().is_empty() {
            let replacement = replace_archive_extension(&current, format);
            set_input(
                &self.output,
                replacement.to_string_lossy().as_ref(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 切换自解压输出，并把输出扩展名在 `.exe` 与当前格式扩展名之间同步。
    fn set_sfx(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.sfx = enabled;
        let current = PathBuf::from(input_value(&self.output, cx));
        if !current.as_os_str().is_empty() {
            let replacement = if enabled {
                current.with_extension("exe")
            } else {
                replace_archive_extension(&current, self.format)
            };
            set_input(
                &self.output,
                replacement.to_string_lossy().as_ref(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 打开系统目录选择器，只替换输出路径的父目录。
    fn choose_directory(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let current = PathBuf::from(input_value(&self.output, cx));
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择压缩包输出目录".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(directory) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |view, window, cx| {
                    let name = current
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("archive.zip");
                    let target = directory.join(name);
                    set_input(&view.output, target.to_string_lossy().as_ref(), window, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 关闭压缩窗口，不改动主应用已经提交的任务。
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| {
            app.status = "已取消压缩".to_string();
            cx.notify();
        });
        window.remove_window();
    }

    /// 在轻巧和经典布局之间切换，并调整窗口尺寸。
    fn switch_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.classic = !self.classic;
        let (width, height) = if self.classic {
            CLASSIC_WINDOW_SIZE
        } else {
            LIGHT_WINDOW_SIZE
        };
        window.resize(gpui_kit::size(px(width), px(height)));
        self.output.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// 校验表单并提交压缩任务，然后关闭独立窗口。
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut output = PathBuf::from(input_value(&self.output, cx));
        if output.as_os_str().is_empty() {
            self.error = Some("请输入输出路径".to_string());
            cx.notify();
            return;
        }
        if self.sources.is_empty() {
            self.error = Some("当前没有可压缩的项目".to_string());
            cx.notify();
            return;
        }
        let password = input_value(&self.password, cx);
        // 自解压只在 ZIP 下有效；运行时模块不处理密码和分卷。
        let sfx = self.sfx && self.format == ArchiveFormat::Zip;
        if sfx && !password.is_empty() {
            self.error = Some("自解压暂不支持密码，请先关闭密码".to_string());
            cx.notify();
            return;
        }
        let split_size = parse_size(&input_value(&self.split, cx));
        if sfx && split_size.is_some_and(|size| size >= 64 * 1024) {
            self.error = Some("自解压不支持分卷".to_string());
            cx.notify();
            return;
        }
        if sfx {
            output.set_extension("exe");
        }
        let comment = self.comment.read(cx).value().to_string();
        let (modified_after, modified_before) = match self.file_time_filter {
            FileTimeFilter::Any => (None, None),
            FileTimeFilter::Before => (None, parse_date(&input_value(&self.before, cx))),
            FileTimeFilter::After => (parse_date(&input_value(&self.after, cx)), None),
            FileTimeFilter::Range => (
                parse_date(&input_value(&self.after, cx)),
                parse_date(&input_value(&self.before, cx)),
            ),
        };
        let options = CompressionOptions {
            format: self.format,
            level: Some(level_for_preset(self.preset)),
            preset: archive_preset(self.preset),
            solid: self.solid && self.format == ArchiveFormat::SevenZip,
            encrypt_header: self.encrypt_header,
            dictionary_size: (self.format == ArchiveFormat::SevenZip)
                .then_some(self.dictionary_mb.saturating_mul(1024 * 1024)),
            threads: self.threads.clamp(1, 64),
            zip_compression: self.zip_compression,
            sevenz_compression: self.sevenz_compression,
            word_size: (self.format == ArchiveFormat::SevenZip).then_some(self.word_size),
            solid_block_size: (self.format == ArchiveFormat::SevenZip)
                .then_some(self.solid_block_mb as u64 * 1024 * 1024),
            archive_time: self.archive_time,
            file_time_filter: self.file_time_filter,
            modified_after,
            modified_before,
            path_mode: self.path_mode,
            password: (!password.is_empty()).then(|| zeroize::Zeroizing::new(password)),
            zip_encryption: if self.zipcrypto {
                ZipEncryption::ZipCrypto
            } else {
                ZipEncryption::Aes256
            },
            comment: (!comment.trim().is_empty()).then_some(comment),
            split_size,
            test_after: self.test_after,
            delete_source: self.delete_source,
            sfx,
            exclude_patterns: parse_patterns(&input_value(&self.exclude, cx)),
            store_only_patterns: parse_patterns(&input_value(&self.store, cx)),
            ..Default::default()
        };
        let sources = self.sources.clone();
        let task_options = CompressTaskOptions {
            shutdown_after: self.shutdown_after,
            low_priority: self.low_priority,
        };
        let app = self.app.clone();
        app.update(cx, |app, cx| {
            app.start_compress_from_options(output, sources, options, task_options, window, cx);
        });
        window.remove_window();
    }

    /// 渲染窗口标题栏。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(38.))
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(px(7.))
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Compression.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_semibold()
                            .child("压缩文件 - Lumark"),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 个项目", self.sources.len())),
                    ),
            )
    }

    /// 渲染所有布局共用的底部操作栏。
    fn render_footer(&self, cx: &Context<Self>, classic: bool) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .p_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                button("compress-window-switch")
                    .ghost()
                    .cursor_pointer()
                    .icon(AppIcon::Tool.element(15.))
                    .label(if classic {
                        "切换至轻巧模式"
                    } else {
                        "切换至经典模式"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.switch_layout(window, cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                button("compress-window-cancel")
                    .label("取消")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close(window, cx);
                    })),
            )
            .child(
                button("compress-window-start")
                    .primary()
                    .icon(AppIcon::Compression.element(15.))
                    .label(if classic { "确定" } else { "立即压缩" })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start(window, cx);
                    })),
            )
    }

    /// 渲染轻巧模式的完整表单。
    fn render_light(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
                    .child(hint_text(
                        cx,
                        format!("扩展名 .{}", self.format.extension()),
                    )),
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
                        |this, _, _| this.preset = SettingsPreset::Speed,
                    ))
                    .child(radio_button(
                        cx,
                        "compress-window-preset-balanced",
                        "平衡",
                        self.preset == SettingsPreset::Balanced,
                        |this, _, _| this.preset = SettingsPreset::Balanced,
                    ))
                    .child(radio_button(
                        cx,
                        "compress-window-preset-size",
                        "体积小",
                        self.preset == SettingsPreset::Size,
                        |this, _, _| this.preset = SettingsPreset::Size,
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
                    .when(self.format == ArchiveFormat::Zip, |this| {
                        this.child(
                            Checkbox::new("compress-window-sfx")
                                .label("自解压 EXE")
                                .checked(self.sfx)
                                .on_click(cx.listener(|this, value: &bool, window, cx| {
                                    this.set_sfx(*value, window, cx);
                                })),
                        )
                    })
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

    /// 渲染经典模式的六个分段选项卡。
    fn render_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected = usize::from(self.tab);
        let view = cx.entity().downgrade();
        let tabs = [
            (AppIcon::Setting, "常规"),
            (AppIcon::Key, "密码"),
            (AppIcon::Doc, "文件"),
            (AppIcon::Tool, "算法"),
            (AppIcon::Time, "时间"),
            (AppIcon::Edit, "注释"),
        ];
        let items = tabs
            .iter()
            .enumerate()
            .map(|(index, (icon, label))| {
                let view = view.clone();
                segmented_tab(
                    cx,
                    ("compress-window-tab", index),
                    *icon,
                    label,
                    selected == index,
                )
                .set_position(index + 1, tabs.len())
                .on_click(move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.tab = index as u8;
                        cx.notify();
                    })
                    .ok();
                })
                .into_any_element()
            })
            .collect::<Vec<_>>();

        segmented_tab_bar(cx, "compress-window-tabs", items)
    }

    /// 渲染经典模式的六个选项卡。
    fn render_classic(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let tab = self.tab;
        let content = v_flex()
            .id("compress-window-classic-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(match tab {
                0 => self.render_general_tab(cx),
                1 => self.render_password_tab(cx),
                2 => self.render_files_tab(cx),
                3 => self.render_algorithm_tab(cx),
                4 => self.render_time_tab(cx),
                _ => self.render_comment_tab(cx),
            });
        v_flex()
            .flex_1()
            .min_h_0()
            .child(div().w_full().px_4().pt_3().child(self.render_tabs(cx)))
            .child(content)
            .child(self.render_footer(cx, true))
            .into_any_element()
    }

    /// 渲染“常规”选项卡。
    fn render_general_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
                    .child(hint_text(
                        cx,
                        format!("扩展名 .{}", self.format.extension()),
                    )),
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
                    .when(self.format == ArchiveFormat::Zip, |this| {
                        this.child(
                            Checkbox::new("classic-sfx")
                                .label("自解压 EXE")
                                .checked(self.sfx)
                                .on_click(cx.listener(|this, value: &bool, window, cx| {
                                    this.set_sfx(*value, window, cx);
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
            .child(classic_field(
                "分卷大小:",
                92.,
                Input::new(&self.split).id("classic-split").w_full(),
            ))
            .child(hint_text(cx, "支持 100M、2G 等写法；留空表示不分卷。"))
            .into_any_element()
    }

    /// 渲染“密码”选项卡。
    fn render_password_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
                "ZIP 使用 AES-256；7Z 设置密码后同时加密文件内容。",
            ))
            .when(self.format == ArchiveFormat::Zip, |this| {
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
    fn render_files_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
                        |this, _, _| this.path_mode = PathMode::Relative,
                    ))
                    .child(radio_button(
                        cx,
                        "classic-path-noroot",
                        "去掉顶层目录",
                        self.path_mode == PathMode::NoRoot,
                        |this, _, _| this.path_mode = PathMode::NoRoot,
                    ))
                    .child(radio_button(
                        cx,
                        "classic-path-flatten",
                        "全部平铺",
                        self.path_mode == PathMode::Flatten,
                        |this, _, _| this.path_mode = PathMode::Flatten,
                    )),
            ))
            .into_any_element()
    }

    /// 渲染“算法”选项卡：格式方法、字典、单词、固实块和线程数。
    fn render_algorithm_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let zip = self.format == ArchiveFormat::Zip;
        let seven = self.format == ArchiveFormat::SevenZip;
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
            .when(!zip && !seven, |this| {
                this.child(classic_group(
                    cx,
                    "压缩算法",
                    hint_text(cx, "该格式使用固定算法；可在“常规”页调整压缩预设。"),
                ))
            })
            .into_any_element()
    }

    /// 渲染“时间”选项卡。
    fn render_time_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
    fn render_comment_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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

    /// 估算当前 7Z 算法所需的内存。
    fn memory_hint(&self) -> String {
        if self.format == ArchiveFormat::SevenZip {
            let encoder = self.dictionary_mb.saturating_mul(10) + self.word_size;
            let decoder = self.dictionary_mb + 2;
            format!("压缩所需内存：约 {encoder} MB；解压所需内存：约 {decoder} MB")
        } else if self.format == ArchiveFormat::Zip {
            "ZIP Deflate 使用固定内存；LZMA/BZip2 方法会额外占用少量内存。".to_string()
        } else {
            "该格式由流编码器处理，内存占用较小。".to_string()
        }
    }
}

impl Render for CompressWindowView {
    /// 渲染独立压缩窗口。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(dialog_banner(cx, 50.))
            .child(if self.classic {
                self.render_classic(cx)
            } else {
                self.render_light(cx)
            })
    }
}

impl LumarkApp {
    /// 用明确的项目列表打开轻巧压缩窗口。
    pub(crate) fn open_compress_window_for_sources(
        &mut self,
        sources: Vec<PathBuf>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.compress_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        self.compress_window = None;
        if sources.is_empty() {
            self.status = "当前没有可压缩的项目".to_string();
            return;
        }
        let init = CompressInit {
            format: self.compress_format,
            preset: self.compress_preset,
            solid: self.compress_solid,
            encrypt_header: self.compress_encrypt_header,
            zipcrypto: self.compress_zipcrypto,
            test_after: self.compress_test_after,
            delete_source: self.compress_delete_source,
            sfx: self.compress_sfx,
            path_mode: self.compress_path_mode,
            threads: std::thread::available_parallelism()
                .map(|count| count.get() as u32)
                .unwrap_or(1)
                .clamp(1, 8),
            zip_compression: ZipCompression::Deflate,
            sevenz_compression: SevenZCompression::Lzma2,
            dictionary_mb: 16,
            word_size: 32,
            solid_block_mb: 2048,
            file_time_filter: FileTimeFilter::Any,
            archive_time: ArchiveTimeSource::Current,
            classic: false,
        };
        let app = cx.entity();
        let output = self.compress_output.clone();
        let password = self.compress_password.clone();
        let exclude = self.compress_exclude.clone();
        let store = self.compress_store.clone();
        let split = self.compress_split.clone();
        let after = self.compress_after.clone();
        let before = self.compress_before.clone();
        let sources_for_view = sources.clone();
        let (width, height) = LIGHT_WINDOW_SIZE;
        let size = gpui_kit::size(px(width), px(height));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - size.height.as_f32()) / 2.0),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("压缩文件 - Lumark".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(size),
            ..TitleBar::window_options()
        };
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                CompressWindowView::new(
                    app.clone(),
                    sources_for_view.clone(),
                    output.clone(),
                    password.clone(),
                    exclude.clone(),
                    store.clone(),
                    split.clone(),
                    after.clone(),
                    before.clone(),
                    init.clone(),
                    window,
                    cx,
                )
            })
        });
        match result {
            Ok((handle, _)) => self.compress_window = Some(handle),
            Err(error) => self.status = format!("无法打开压缩窗口：{error}"),
        }
    }
}

/// 创建带标签的官方复选框。
fn check_button(
    cx: &Context<CompressWindowView>,
    id: &'static str,
    label: &'static str,
    checked: bool,
    handler: impl Fn(&mut CompressWindowView, bool, &mut Context<CompressWindowView>) + 'static,
) -> Checkbox {
    Checkbox::new(id)
        .label(label)
        .checked(checked)
        .on_click(cx.listener(move |this, value: &bool, _, cx| {
            handler(this, *value, cx);
            cx.notify();
        }))
}

/// 创建带标签的官方单选按钮。
fn radio_button(
    cx: &Context<CompressWindowView>,
    id: &'static str,
    label: &'static str,
    checked: bool,
    handler: impl Fn(&mut CompressWindowView, bool, &mut Context<CompressWindowView>) + 'static,
) -> Radio {
    Radio::new(id)
        .label(label)
        .checked(checked)
        .on_click(cx.listener(move |this, value: &bool, _, cx| {
            handler(this, *value, cx);
            cx.notify();
        }))
}

/// 创建一个官方 Select 状态，选项来自静态标签列表。
fn new_select(
    window: &mut Window,
    cx: &mut Context<CompressWindowView>,
    labels: Vec<&'static str>,
    index: usize,
) -> ChoiceState {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(labels),
            Some(IndexPath::new(index)),
            window,
            cx,
        )
    })
}

/// 把固实块大小格式化为下拉标签。
fn solid_label_for(mb: u32) -> String {
    if mb >= 1024 {
        format!("{} GB", mb / 1024)
    } else {
        format!("{mb} MB")
    }
}
