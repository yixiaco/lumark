//! 独立压缩窗口状态、构造、Render 和打开入口。

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
    /// 初始压缩格式。
    pub format: ArchiveFormat,
    /// 初始压缩预设。
    pub preset: SettingsPreset,
    /// 是否默认固实。
    pub solid: bool,
    /// 是否默认加密 7Z 头。
    pub encrypt_header: bool,
    /// 是否默认使用 ZipCrypto。
    pub zipcrypto: bool,
    /// 是否默认创建后测试。
    pub test_after: bool,
    /// 是否默认删除源文件。
    pub delete_source: bool,
    /// 生成自解压 EXE；在 ZIP 或 7Z 格式下生效。
    pub sfx: bool,
    /// 是否默认启用伪装输出。
    pub disguise: bool,
    /// 伪装面具类型。
    pub disguise_mask: DisguiseMaskPreference,
    /// 自定义面具文件路径。
    pub disguise_custom: Option<PathBuf>,
    /// 初始路径根模式。
    pub path_mode: PathMode,
    /// 初始压缩线程数。
    pub threads: u32,
    /// 初始 ZIP 压缩方法。
    pub zip_compression: ZipCompression,
    /// 初始 7Z 压缩方法。
    pub sevenz_compression: SevenZCompression,
    /// 初始 ZPAQ 级别。
    pub zpaq_compression: ZpaqCompression,
    /// 初始 LZMA 字典大小。
    pub dictionary_mb: u32,
    /// 初始 LZMA 字长。
    pub word_size: u32,
    /// 初始固实块大小。
    pub solid_block_mb: u32,
    /// 分卷大小输入框的初始单位。
    pub split_unit: SizeUnit,
    /// 初始文件时间过滤。
    pub file_time_filter: FileTimeFilter,
    /// 初始归档时间来源。
    pub archive_time: ArchiveTimeSource,
    /// 是否以经典排版打开。
    pub classic: bool,
}

/// 独立压缩窗口的视图状态。
pub(crate) struct CompressWindowView {
    /// 主应用句柄。
    pub(super) app: Entity<LumarkApp>,
    /// 本次要压缩的源路径。
    pub(super) sources: Vec<PathBuf>,
    /// 输出路径输入框。
    pub(super) output: Entity<InputState>,
    /// 密码输入框。
    pub(super) password: Entity<InputState>,
    /// 注释输入框。
    pub(super) comment: Entity<TextareaState>,
    /// 排除规则输入框。
    pub(super) exclude: Entity<InputState>,
    /// 强制 Store 规则输入框。
    pub(super) store: Entity<InputState>,
    /// 分卷大小输入框。
    pub(super) split: Entity<InputState>,
    /// 分卷大小输入框右侧下拉框选中的单位。
    pub(super) split_unit: SizeUnit,
    /// 压缩后命令输入框。
    pub(super) after: Entity<InputState>,
    /// 压缩前命令输入框。
    pub(super) before: Entity<InputState>,
    /// 当前压缩格式。
    pub(super) format: ArchiveFormat,
    /// 当前源可选的格式；多文件或文件夹源会隐藏单文件流格式。
    pub(super) format_options: Vec<(ArchiveFormat, &'static str)>,
    /// 当前压缩预设。
    pub(super) preset: SettingsPreset,
    /// 是否创建固实归档。
    pub(super) solid: bool,
    /// 是否加密 7Z 头。
    pub(super) encrypt_header: bool,
    /// 是否使用 ZipCrypto。
    pub(super) zipcrypto: bool,
    /// 创建后是否测试。
    pub(super) test_after: bool,
    /// 成功后是否删除源文件。
    pub(super) delete_source: bool,
    /// 是否生成自解压 EXE。
    pub(super) sfx: bool,
    /// 是否把输出包装成媒体外观。
    pub(super) disguise: bool,
    /// 当前面具类型和自定义面具路径。
    pub(super) disguise_mask: DisguiseMaskPreference,
    /// 自定义面具路径输入框。
    pub(super) disguise_custom: Option<PathBuf>,
    /// 压缩成功后安排系统关机；每次打开窗口都从关闭状态开始。
    pub(super) shutdown_after: bool,
    /// 压缩期间降低进程优先级，减少界面卡顿。
    pub(super) low_priority: bool,
    /// 归档内路径根模式。
    pub(super) path_mode: PathMode,
    /// 是否使用经典排版。
    pub(super) classic: bool,
    /// 当前选项卡。
    pub(super) tab: u8,
    /// 密码是否明文显示。
    pub(super) password_visible: bool,
    /// 当前表单错误。
    pub(super) error: Option<String>,
    /// ZIP 压缩方法。
    pub(super) zip_compression: ZipCompression,
    /// 7Z 压缩方法。
    pub(super) sevenz_compression: SevenZCompression,
    /// ZPAQ 压缩级别。
    pub(super) zpaq_compression: ZpaqCompression,
    /// LZMA 字典大小。
    pub(super) dictionary_mb: u32,
    /// LZMA 字长。
    pub(super) word_size: u32,
    /// 固实块大小。
    pub(super) solid_block_mb: u32,
    /// 压缩线程数。
    pub(super) threads: u32,
    /// 文件时间过滤。
    pub(super) file_time_filter: FileTimeFilter,
    /// 归档时间来源。
    pub(super) archive_time: ArchiveTimeSource,
    /// 格式下拉框。
    pub(super) format_select: ChoiceState,
    /// ZIP 方法下拉框。
    pub(super) zip_method_select: ChoiceState,
    /// 7Z 方法下拉框。
    pub(super) seven_method_select: ChoiceState,
    /// ZPAQ 级别下拉框。
    pub(super) zpaq_method_select: ChoiceState,
    /// 字典大小下拉框。
    pub(super) dictionary_select: ChoiceState,
    /// 字长下拉框。
    pub(super) word_select: ChoiceState,
    /// 固实块下拉框。
    pub(super) solid_select: ChoiceState,
    /// 线程数下拉框。
    pub(super) thread_select: ChoiceState,
    /// 时间过滤下拉框。
    pub(super) filter_select: ChoiceState,
    /// 归档时间下拉框。
    pub(super) archive_time_select: ChoiceState,
    /// 分卷单位下拉框。
    pub(super) split_unit_select: ChoiceState,
    /// 面具下拉框。
    pub(super) disguise_select: ChoiceState,
    /// 保留下拉框订阅，窗口存活期间不被释放。
    pub(super) _select_subscriptions: Vec<Subscription>,
}

/// 压缩窗口下拉选择框的实体类型。
type ChoiceState = Entity<SelectState<SearchableVec<&'static str>>>;

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

/// 分卷大小预设。
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

/// 压缩窗口可选的格式顺序。
const WRITABLE_FORMATS: &[(ArchiveFormat, &str)] = &[
    (ArchiveFormat::Zip, "ZIP"),
    (ArchiveFormat::SevenZip, "7Z"),
    (ArchiveFormat::Zpaq, "ZPAQ"),
    (ArchiveFormat::Lzh, "LZH"),
    (ArchiveFormat::Wim, "WIM"),
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

mod labels;
pub(crate) use labels::*;

mod chrome;
mod classic;
mod disguise;
mod form;
mod light;
mod split;
mod tabs;
mod util;
use util::*;

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
        let disguise_select = new_select(
            window,
            cx,
            DISGUISE_MASK_LABELS.to_vec(),
            disguise_mask_index(init.disguise_mask),
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
        select_subscriptions.push(cx.subscribe_in(
            &disguise_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.set_disguise_mask(disguise_mask_from_label(value), window, cx);
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
            disguise: init.disguise,
            disguise_mask: init.disguise_mask,
            disguise_custom: init.disguise_custom,
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
            disguise_select,
            _select_subscriptions: select_subscriptions,
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
            sfx: self.compress_sfx
                && matches!(
                    self.compress_format,
                    ArchiveFormat::Zip | ArchiveFormat::SevenZip
                ),
            disguise: self.compress_disguise,
            disguise_mask: self.compress_disguise_mask,
            disguise_custom: self.compress_disguise_custom.clone(),
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
