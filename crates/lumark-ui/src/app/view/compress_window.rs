use super::super::*;
use super::components::{
    AppIcon, choice_select, classic_field, classic_group, dialog_banner, hint_text, segmented_tab,
    segmented_tab_bar,
};
use gpui_kit::Entity;
use gpui_kit::Subscription;
use gpui_kit::component::IndexPath;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{
    InputGroup, InputGroupAddon, InputGroupAddonAlignment, InputGroupButton, InputGroupInput,
};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
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
    pub zpaq_compression: ZpaqCompression,
    pub dictionary_mb: u32,
    pub word_size: u32,
    pub solid_block_mb: u32,
    /// 分卷大小输入框的初始单位。
    pub split_unit: SizeUnit,
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
    /// 分卷大小输入框右侧下拉框选中的单位。
    split_unit: SizeUnit,
    after: Entity<InputState>,
    before: Entity<InputState>,
    format: ArchiveFormat,
    /// 当前源可选的格式；多文件或文件夹源会隐藏单文件流格式。
    format_options: Vec<(ArchiveFormat, &'static str)>,
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
    zpaq_compression: ZpaqCompression,
    dictionary_mb: u32,
    word_size: u32,
    solid_block_mb: u32,
    threads: u32,
    file_time_filter: FileTimeFilter,
    archive_time: ArchiveTimeSource,
    format_select: ChoiceState,
    zip_method_select: ChoiceState,
    seven_method_select: ChoiceState,
    zpaq_method_select: ChoiceState,
    dictionary_select: ChoiceState,
    word_select: ChoiceState,
    solid_select: ChoiceState,
    thread_select: ChoiceState,
    filter_select: ChoiceState,
    archive_time_select: ChoiceState,
    split_unit_select: ChoiceState,
    _select_subscriptions: Vec<Subscription>,
}

type ChoiceState = Entity<SelectState<SearchableVec<&'static str>>>;

const ZIP_METHOD_LABELS: [&str; 3] = ["Deflate（推荐）", "BZip2", "LZMA"];
const SEVEN_METHOD_LABELS: [&str; 4] = ["LZMA2（推荐）", "LZMA", "PPMd", "BZip2"];
const ZPAQ_METHOD_LABELS: [&str; 5] = [
    "1 - 快速",
    "2 - 平衡（推荐）",
    "3 - 较高",
    "4 - 更高（较慢）",
    "5 - 极限（最慢）",
];
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

/// 常用分卷大小预设。
///
/// 数值和单位分开保存，选中后分别写入输入框与单位下拉；标签沿用参考
/// 产品的“大小 - 用途”写法，方便按论坛附件、刻录介质等场景快速选择。
#[derive(Clone, Copy)]
struct SplitPreset {
    amount: &'static str,
    unit: SizeUnit,
    label: &'static str,
}

impl SplitPreset {
    /// 返回预设对应的字节数，用于在菜单里标记当前选中项。
    fn bytes(self) -> u64 {
        parse_size_in_unit(self.amount, self.unit).unwrap_or_default()
    }
}

static SPLIT_PRESETS: [SplitPreset; 7] = [
    SplitPreset {
        amount: "512",
        unit: SizeUnit::Kilobyte,
        label: "512KB - 论坛附件 小",
    },
    SplitPreset {
        amount: "1",
        unit: SizeUnit::Megabyte,
        label: "1MB - 论坛附件 中",
    },
    SplitPreset {
        amount: "2",
        unit: SizeUnit::Megabyte,
        label: "2MB - 论坛附件 大",
    },
    SplitPreset {
        amount: "100",
        unit: SizeUnit::Megabyte,
        label: "100MB - 网盘",
    },
    SplitPreset {
        amount: "700",
        unit: SizeUnit::Megabyte,
        label: "700MB - CD刻录",
    },
    SplitPreset {
        amount: "4.38",
        unit: SizeUnit::Gigabyte,
        label: "4.38GB - DVD刻录",
    },
    SplitPreset {
        amount: "7.96",
        unit: SizeUnit::Gigabyte,
        label: "7.96GB - 双层DVD刻录",
    },
];

const WRITABLE_FORMATS: &[(ArchiveFormat, &str)] = &[
    (ArchiveFormat::Zip, "ZIP"),
    (ArchiveFormat::SevenZip, "7Z"),
    (ArchiveFormat::Zpaq, "ZPAQ"),
    (ArchiveFormat::Tar, "TAR"),
    (ArchiveFormat::TarGz, "TAR.GZ"),
    (ArchiveFormat::TarBz2, "TAR.BZ2"),
    (ArchiveFormat::TarXz, "TAR.XZ"),
    (ArchiveFormat::TarZst, "TAR.ZST"),
    (ArchiveFormat::TarBr, "TAR.BR"),
    (ArchiveFormat::TarLz4, "TAR.LZ4"),
    (ArchiveFormat::TarLz, "TAR.LZ"),
    (ArchiveFormat::TarLzma, "TAR.LZMA"),
    (ArchiveFormat::Gzip, "GZ"),
    (ArchiveFormat::Bzip2, "BZ2"),
    (ArchiveFormat::Xz, "XZ"),
    (ArchiveFormat::Zstd, "ZST"),
    (ArchiveFormat::Brotli, "BROTLI"),
    (ArchiveFormat::Lz4, "LZ4"),
];

/// 根据源类型过滤压缩格式。
///
/// 单文件流格式没有归档目录，只能压缩一个普通文件；多选文件或文件夹时
/// 从下拉框隐藏这些格式，避免用户选到必然失败的选项。
fn writable_format_options(allow_single_stream: bool) -> Vec<(ArchiveFormat, &'static str)> {
    WRITABLE_FORMATS
        .iter()
        .copied()
        .filter(|(format, _)| allow_single_stream || !format.is_single_stream())
        .collect()
}

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
        let allow_single_stream = single_stream_allowed(&sources);
        let format_options = writable_format_options(allow_single_stream);
        // 如果设置默认格式是 GZ/Brotli 等单流格式，但源有多个文件或目录，
        // 优先落到对应的 TAR 变体，而不是静默回落到 ZIP。
        let preferred = init
            .format
            .single_stream_tar_variant()
            .unwrap_or(ArchiveFormat::Zip);
        let format_index = format_options
            .iter()
            .position(|(format, _)| *format == init.format)
            .or_else(|| {
                format_options
                    .iter()
                    .position(|(format, _)| *format == preferred)
            })
            .unwrap_or(0);
        let initial_format = format_options
            .get(format_index)
            .map(|(format, _)| *format)
            .unwrap_or(ArchiveFormat::Zip);
        if initial_format != init.format {
            let current = PathBuf::from(input_value(&output, cx));
            if !current.as_os_str().is_empty() {
                let replacement = replace_archive_extension(&current, initial_format);
                set_input(&output, replacement.to_string_lossy().as_ref(), window, cx);
            }
        }
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
        let zpaq_method_index = zpaq_method_index(init.zpaq_compression);
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
            format_options.iter().map(|(_, label)| *label).collect(),
            format_index,
        );
        let zip_method_select =
            new_select(window, cx, ZIP_METHOD_LABELS.to_vec(), zip_method_index);
        let seven_method_select =
            new_select(window, cx, SEVEN_METHOD_LABELS.to_vec(), seven_method_index);
        let zpaq_method_select =
            new_select(window, cx, ZPAQ_METHOD_LABELS.to_vec(), zpaq_method_index);
        let dictionary_select =
            new_select(window, cx, DICTIONARY_LABELS.to_vec(), dictionary_index);
        let word_select = new_select(window, cx, WORD_LABELS.to_vec(), word_index);
        let solid_select = new_select(window, cx, SOLID_LABELS.to_vec(), solid_index);
        let thread_select = new_select(window, cx, THREAD_LABELS.to_vec(), thread_index);
        let filter_select = new_select(window, cx, FILTER_LABELS.to_vec(), filter_index);
        let archive_time_select =
            new_select(window, cx, ARCHIVE_TIME_LABELS.to_vec(), archive_time_index);
        let split_unit_select = new_select(
            window,
            cx,
            SizeUnit::LABELS.to_vec(),
            init.split_unit.index(),
        );
        let mut select_subscriptions = Vec::new();
        select_subscriptions.push(cx.subscribe_in(
            &format_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some((format, _)) = this
                        .format_options
                        .iter()
                        .find(|(_, label)| *label == *value)
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
            &zpaq_method_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(index) =
                        ZPAQ_METHOD_LABELS.iter().position(|label| *label == *value)
                {
                    this.zpaq_compression = zpaq_compression_for_index(index);
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
        select_subscriptions.push(cx.subscribe(
            &split_unit_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    let unit = SizeUnit::from_label(value);
                    this.split_unit = unit;
                    this.app.update(cx, |app, _| app.compress_split_unit = unit);
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
            split_unit: init.split_unit,
            after,
            before,
            format: initial_format,
            format_options,
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
            zpaq_compression: init.zpaq_compression,
            dictionary_mb: init.dictionary_mb,
            word_size: init.word_size,
            solid_block_mb: init.solid_block_mb,
            threads: init.threads,
            file_time_filter: init.file_time_filter,
            archive_time: init.archive_time,
            format_select,
            zip_method_select,
            seven_method_select,
            zpaq_method_select,
            dictionary_select,
            word_select,
            solid_select,
            thread_select,
            filter_select,
            archive_time_select,
            split_unit_select,
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

    /// 格式选择旁的解释文本。
    ///
    /// 单文件源只显示扩展名；多文件或文件夹源会提示已限制为可容纳多条目的
    /// 格式，避免用户误以为 GZ/Brotli 等单流格式仍可用。
    fn format_hint(&self) -> String {
        let extension = self.format.extension();
        if !single_stream_allowed(&self.sources) && extension.starts_with("tar") {
            format!("扩展名 .{extension}（多文件源用 TAR 变体）")
        } else {
            format!("扩展名 .{extension}")
        }
    }

    /// 切换压缩预设，并同步 ZPAQ 的当前级别。
    ///
    /// 轻巧模式只有这三个预设、没有算法页；如果只改标签而不同步级别，
    /// “体积小”仍会沿用窗口打开时的 ZPAQ 方法，压缩效果自然和平衡接近。
    fn set_preset(&mut self, preset: SettingsPreset, window: &mut Window, cx: &mut Context<Self>) {
        self.preset = preset;
        self.zpaq_compression = ZpaqCompression::from_preset(archive_preset(preset));
        let index = zpaq_method_index(self.zpaq_compression);
        self.zpaq_method_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(index)), window, cx);
        });
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
        // 正常流程在打开窗口时已隐藏单流格式；这里防御直接调用或文件状态
        // 变化（例如唯一源在窗口打开后被换成目录）导致的无效组合。
        if self.format.is_single_stream() && !single_stream_allowed(&self.sources) {
            let suggestion = self
                .format
                .single_stream_tar_variant()
                .map(|format| format.label())
                .unwrap_or("TAR 归档");
            self.error = Some(format!(
                "{} 只能压缩一个普通文件；压缩多个文件或文件夹请改用 {}。",
                self.format.label(),
                suggestion
            ));
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
        let split_value = input_value(&self.split, cx);
        let split_size = match split_size_or_error(&split_value, self.split_unit) {
            Ok(size) => size,
            Err(message) if self.classic => {
                self.error = Some(message.to_string());
                cx.notify();
                return;
            }
            // 轻巧模式没有分卷字段，隐藏的旧值按原有规则静默忽略。
            Err(_) => None,
        };
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
        let zpaq_compression =
            effective_zpaq_compression(self.classic, self.preset, self.zpaq_compression);
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
            zpaq_compression,
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

    /// 渲染分卷大小组合控件：数值输入、常用大小下拉和单位下拉。
    ///
    /// 数值框保留手动输入能力，可以直接写 `100M`、`4.38G`；下拉箭头只负责
    /// 填入常用数值并同步单位，不会锁住输入框。显式后缀在解析时优先于单位
    /// 下拉，两种输入方式不会互相覆盖。
    fn split_size_field(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        let current = parse_size_in_unit(&input_value(&self.split, cx), self.split_unit);
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                InputGroup::new("classic-split-group")
                    .flex_1()
                    .min_w_0()
                    .input(
                        InputGroupInput::new(&self.split)
                            .id("classic-split")
                            .aria_label("分卷大小数值"),
                    )
                    .addon(
                        InputGroupAddon::new("classic-split-preset-addon")
                            .align(InputGroupAddonAlignment::InlineEnd)
                            .child(
                                InputGroupButton::new("classic-split-presets")
                                    .cursor_pointer()
                                    .accessibility_label("常用分卷大小")
                                    .tooltip("从常用大小中选择，也可以直接输入")
                                    .dropdown_caret(true)
                                    .dropdown_menu_with_anchor(
                                        gpui_kit::Anchor::TopRight,
                                        move |menu, _, _| {
                                            SPLIT_PRESETS.iter().fold(menu, |menu, preset| {
                                                let view = view.clone();
                                                let amount = preset.amount;
                                                let unit = preset.unit;
                                                menu.item(
                                                    PopupMenuItem::new(preset.label)
                                                        .checked(current == Some(preset.bytes()))
                                                        .on_click(move |_, window, cx| {
                                                            view.update(cx, |this, cx| {
                                                                this.apply_split_preset(
                                                                    amount, unit, window, cx,
                                                                );
                                                            })
                                                            .ok();
                                                        }),
                                                )
                                            })
                                        },
                                    ),
                            ),
                    ),
            )
            .child(choice_select(
                "compress-split-unit",
                &self.split_unit_select,
                92.,
                "分卷大小单位",
            ))
    }

    /// 把常用分卷预设写入数值输入框，并同步单位下拉。
    fn apply_split_preset(
        &mut self,
        amount: &str,
        unit: SizeUnit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        set_input(&self.split, amount, window, cx);
        self.split_unit = unit;
        self.app.update(cx, |app, _| app.compress_split_unit = unit);
        self.split_unit_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(unit.index())), window, cx);
        });
        cx.notify();
    }

    /// 生成分卷提示：单位换算结果实时可见，并说明留空和手动输入的语义。
    fn split_hint(&self, cx: &Context<Self>) -> String {
        let value = input_value(&self.split, cx);
        match parse_size_in_unit(&value, self.split_unit) {
            Some(bytes) => {
                format!(
                    "当前分卷大小：{}；留空表示不分卷。",
                    format_size_precise(bytes)
                )
            }
            None if value.trim().is_empty() => {
                "留空表示不分卷；最小 64KB，可从下拉选择或输入 100M、2G。".to_string()
            }
            None => "无法识别分卷大小；请输入数字或 100M、2G 等写法。".to_string(),
        }
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
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .w_full()
                        .px_4()
                        .pb_1()
                        .text_xs()
                        .text_color(cx.theme().danger_foreground)
                        .child(error),
                )
            })
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
            .child(classic_field("分卷大小:", 92., self.split_size_field(cx)))
            .child(hint_text(cx, self.split_hint(cx)))
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
                "ZIP 使用 AES-256；7Z 设置密码后同时加密文件内容；ZPAQ 固定使用 AES-256（唯一加密算法）。",
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
    fn render_algorithm_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
            sfx: self.compress_sfx && self.compress_format == ArchiveFormat::Zip,
            path_mode: self.compress_path_mode,
            threads: std::thread::available_parallelism()
                .map(|count| count.get() as u32)
                .unwrap_or(1)
                .clamp(1, 8),
            zip_compression: ZipCompression::Deflate,
            sevenz_compression: SevenZCompression::Lzma2,
            zpaq_compression: ZpaqCompression::from_preset(archive_preset(self.compress_preset)),
            dictionary_mb: 16,
            word_size: 32,
            solid_block_mb: 2048,
            split_unit: self.compress_split_unit,
            file_time_filter: FileTimeFilter::Any,
            archive_time: default_archive_time(self.compress_format),
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
    handler: impl Fn(&mut CompressWindowView, bool, &mut Window, &mut Context<CompressWindowView>)
    + 'static,
) -> Radio {
    Radio::new(id)
        .label(label)
        .checked(checked)
        .on_click(cx.listener(move |this, value: &bool, window, cx| {
            handler(this, *value, window, cx);
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

/// 解析并校验分卷大小。
///
/// 留空表示不分卷；无法识别或低于归档层 64KB 下限时返回可直接展示的
/// 错误文本，避免用户以为已经分卷、实际却只生成了单个文件。
fn split_size_or_error(value: &str, unit: SizeUnit) -> Result<Option<u64>, &'static str> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let size =
        parse_size_in_unit(value, unit).ok_or("分卷大小无法识别，请输入数字或 100M、2G 等写法")?;
    if size < 64 * 1024 {
        return Err("分卷大小至少为 64KB");
    }
    Ok(Some(size))
}

/// 按格式选择压缩窗口的默认归档时间。
///
/// ZPAQ 的“当前/最近”时间需要复制全部源文件，代码目录动辄数百 MB；
/// 默认保留原始时间，既符合 zpaq 惯例，也避免只读/被占用文件导致失败。
fn default_archive_time(format: ArchiveFormat) -> ArchiveTimeSource {
    if format == ArchiveFormat::Zpaq {
        ArchiveTimeSource::Original
    } else {
        ArchiveTimeSource::Current
    }
}

/// 计算本次任务实际使用的 ZPAQ 级别。
///
/// 轻巧模式只有三个预设，没有算法页，因此提交时必须以预设为准；经典
/// 模式允许在算法页单独选择 1–5 级，使用显式选择值。
fn effective_zpaq_compression(
    classic: bool,
    preset: SettingsPreset,
    selected: ZpaqCompression,
) -> ZpaqCompression {
    if classic {
        selected
    } else {
        ZpaqCompression::from_preset(archive_preset(preset))
    }
}

/// 把 ZPAQ 压缩级别映射为下拉索引。
fn zpaq_method_index(compression: ZpaqCompression) -> usize {
    match compression {
        ZpaqCompression::Fast => 0,
        ZpaqCompression::Balanced => 1,
        ZpaqCompression::Better => 2,
        ZpaqCompression::Maximum => 3,
        ZpaqCompression::Ultra => 4,
    }
}

/// 把 ZPAQ 下拉索引映射为压缩级别。
fn zpaq_compression_for_index(index: usize) -> ZpaqCompression {
    match index {
        0 => ZpaqCompression::Fast,
        2 => ZpaqCompression::Better,
        3 => ZpaqCompression::Maximum,
        4 => ZpaqCompression::Ultra,
        _ => ZpaqCompression::Balanced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 压缩窗口的可写格式必须与归档层的 `is_writable()` 双向一致，
    /// 避免再出现核心支持但界面漏项（例如 Brotli/LZ4）的情况。
    #[test]
    fn writable_formats_cover_archive_service() {
        let expected: Vec<_> = ArchiveFormat::ALL
            .iter()
            .copied()
            .filter(|format| format.is_writable())
            .collect();
        let actual: Vec<_> = WRITABLE_FORMATS.iter().map(|(format, _)| *format).collect();
        for format in &expected {
            assert!(actual.contains(format), "压缩窗口缺少可写格式：{format:?}");
        }
        for format in &actual {
            assert!(format.is_writable(), "格式未声明可写：{format:?}");
        }
        assert_eq!(actual.len(), expected.len(), "可写格式数量与界面不一致");
        assert!(
            actual.contains(&ArchiveFormat::Zpaq),
            "压缩窗口缺少 ZPAQ 选项"
        );
    }

    /// 多文件或文件夹源必须隐藏单文件流格式，只保留可容纳多条目的格式。
    #[test]
    fn multiple_sources_hide_single_stream_formats() {
        let single_file = writable_format_options(true);
        assert_eq!(single_file, WRITABLE_FORMATS.to_vec());

        let multiple = writable_format_options(false);
        for (format, _) in &multiple {
            assert!(
                !format.is_single_stream(),
                "多文件源仍显示单流格式：{format:?}"
            );
        }
        for format in [
            ArchiveFormat::Gzip,
            ArchiveFormat::Bzip2,
            ArchiveFormat::Xz,
            ArchiveFormat::Zstd,
            ArchiveFormat::Brotli,
            ArchiveFormat::Lz4,
        ] {
            assert!(
                single_file.iter().any(|(item, _)| *item == format),
                "单文件源缺少 {format:?}"
            );
            assert!(
                !multiple.iter().any(|(item, _)| *item == format),
                "多文件源不应包含 {format:?}"
            );
        }
        assert!(
            multiple
                .iter()
                .any(|(format, _)| *format == ArchiveFormat::TarBr),
            "多文件源缺少 TAR.BR"
        );
        assert_eq!(multiple.len(), single_file.len() - 6);
    }

    /// ZPAQ 级别下拉索引与压缩级别必须双向一致。
    #[test]
    fn zpaq_levels_round_trip_through_select_index() {
        let levels = [
            ZpaqCompression::Fast,
            ZpaqCompression::Balanced,
            ZpaqCompression::Better,
            ZpaqCompression::Maximum,
            ZpaqCompression::Ultra,
        ];
        assert_eq!(ZPAQ_METHOD_LABELS.len(), levels.len());
        for (index, level) in levels.into_iter().enumerate() {
            assert_eq!(zpaq_method_index(level), index);
            assert_eq!(zpaq_compression_for_index(index), level);
        }
    }

    /// ZPAQ 默认保留原始时间，其他格式继续默认当前时间。
    #[test]
    fn zpaq_defaults_to_original_archive_time() {
        assert_eq!(
            default_archive_time(ArchiveFormat::Zip),
            ArchiveTimeSource::Current
        );
        assert_eq!(
            default_archive_time(ArchiveFormat::SevenZip),
            ArchiveTimeSource::Current
        );
        assert_eq!(
            default_archive_time(ArchiveFormat::Zpaq),
            ArchiveTimeSource::Original
        );
    }

    /// 轻巧模式提交时以预设为准，经典模式保留显式级别。
    #[test]
    fn light_mode_preset_overrides_zpaq_level() {
        assert_eq!(
            effective_zpaq_compression(false, SettingsPreset::Size, ZpaqCompression::Fast),
            ZpaqCompression::Ultra
        );
        assert_eq!(
            effective_zpaq_compression(false, SettingsPreset::Balanced, ZpaqCompression::Ultra),
            ZpaqCompression::Balanced
        );
        assert_eq!(
            effective_zpaq_compression(true, SettingsPreset::Size, ZpaqCompression::Fast),
            ZpaqCompression::Fast
        );
    }

    /// 常用分卷预设的标签和字节数必须与参考列表一致。
    #[test]
    fn split_presets_match_reference_sizes() {
        let labels: Vec<&str> = SPLIT_PRESETS.iter().map(|preset| preset.label).collect();
        assert_eq!(
            labels,
            [
                "512KB - 论坛附件 小",
                "1MB - 论坛附件 中",
                "2MB - 论坛附件 大",
                "100MB - 网盘",
                "700MB - CD刻录",
                "4.38GB - DVD刻录",
                "7.96GB - 双层DVD刻录",
            ]
        );
        assert_eq!(SPLIT_PRESETS[0].bytes(), 512 * 1024);
        assert_eq!(SPLIT_PRESETS[1].bytes(), 1024 * 1024);
        assert_eq!(SPLIT_PRESETS[2].bytes(), 2 * 1024 * 1024);
        assert_eq!(SPLIT_PRESETS[3].bytes(), 100 * 1024 * 1024);
        assert_eq!(SPLIT_PRESETS[4].bytes(), 700 * 1024 * 1024);
        assert_eq!(
            SPLIT_PRESETS[5].bytes(),
            (4.38 * 1024.0 * 1024.0 * 1024.0) as u64
        );
        assert_eq!(
            SPLIT_PRESETS[6].bytes(),
            (7.96 * 1024.0 * 1024.0 * 1024.0) as u64
        );
    }

    /// 纯数字按单位换算，显式后缀和空值按既有规则处理。
    #[test]
    fn split_size_follows_selected_unit() {
        assert_eq!(parse_size_in_unit("", SizeUnit::Megabyte), None);
        assert_eq!(parse_size_in_unit("  ", SizeUnit::Megabyte), None);
        assert_eq!(parse_size_in_unit("0", SizeUnit::Megabyte), None);
        assert_eq!(parse_size_in_unit("abc", SizeUnit::Megabyte), None);
        assert_eq!(parse_size_in_unit("512", SizeUnit::Byte), Some(512));
        assert_eq!(
            parse_size_in_unit("512", SizeUnit::Kilobyte),
            Some(512 * 1024)
        );
        assert_eq!(
            parse_size_in_unit("100", SizeUnit::Megabyte),
            Some(100 * 1024 * 1024)
        );
        assert_eq!(
            parse_size_in_unit("1.5", SizeUnit::Gigabyte),
            Some(1_610_612_736)
        );
        assert_eq!(
            parse_size_in_unit("100M", SizeUnit::Gigabyte),
            Some(100 * 1024 * 1024)
        );
        assert_eq!(
            parse_size_in_unit("2G", SizeUnit::Byte),
            Some(2 * 1024 * 1024 * 1024)
        );
    }

    /// 单位标签与索引必须双向一致，未知标签回退到 MB。
    #[test]
    fn size_unit_labels_round_trip() {
        let units = [
            SizeUnit::Byte,
            SizeUnit::Kilobyte,
            SizeUnit::Megabyte,
            SizeUnit::Gigabyte,
        ];
        for unit in units {
            assert_eq!(SizeUnit::from_label(SizeUnit::LABELS[unit.index()]), unit);
        }
        assert_eq!(SizeUnit::from_label("TB"), SizeUnit::Megabyte);
        assert_eq!(SizeUnit::Byte.multiplier(), 1);
        assert_eq!(SizeUnit::Kilobyte.multiplier(), 1024);
        assert_eq!(SizeUnit::Megabyte.multiplier(), 1024 * 1024);
        assert_eq!(SizeUnit::Gigabyte.multiplier(), 1024 * 1024 * 1024);
    }

    /// 分卷大小留空表示不分卷，过小或无法识别时返回提示。
    #[test]
    fn split_size_validation_matches_archive_floor() {
        assert_eq!(split_size_or_error("", SizeUnit::Megabyte), Ok(None));
        assert_eq!(
            split_size_or_error("100", SizeUnit::Megabyte),
            Ok(Some(100 * 1024 * 1024))
        );
        assert_eq!(
            split_size_or_error("64", SizeUnit::Kilobyte),
            Ok(Some(64 * 1024))
        );
        assert_eq!(
            split_size_or_error("10", SizeUnit::Byte),
            Err("分卷大小至少为 64KB")
        );
        assert_eq!(
            split_size_or_error("abc", SizeUnit::Megabyte),
            Err("分卷大小无法识别，请输入数字或 100M、2G 等写法")
        );
    }
}
