//! 压缩窗口共享的控件、格式和 ZPAQ 辅助。

use super::*;

/// 创建带标签的官方复选框。
pub(super) fn check_button(
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
pub(super) fn radio_button(
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
pub(super) fn new_select(
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
pub(super) fn solid_label_for(mb: u32) -> String {
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
pub(super) fn split_size_or_error(
    value: &str,
    unit: SizeUnit,
) -> Result<Option<u64>, &'static str> {
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
pub(super) fn default_archive_time(format: ArchiveFormat) -> ArchiveTimeSource {
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
pub(super) fn effective_zpaq_compression(
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
pub(super) fn zpaq_method_index(compression: ZpaqCompression) -> usize {
    match compression {
        ZpaqCompression::Fast => 0,
        ZpaqCompression::Balanced => 1,
        ZpaqCompression::Better => 2,
        ZpaqCompression::Maximum => 3,
        ZpaqCompression::Ultra => 4,
    }
}

/// 把 ZPAQ 下拉索引映射为压缩级别。
pub(super) fn zpaq_compression_for_index(index: usize) -> ZpaqCompression {
    match index {
        0 => ZpaqCompression::Fast,
        2 => ZpaqCompression::Better,
        3 => ZpaqCompression::Maximum,
        4 => ZpaqCompression::Ultra,
        _ => ZpaqCompression::Balanced,
    }
}
