//! 路径、大小、日期、列宽和格式化辅助。

use super::*;

/// 创建带可选密码掩码的主题文本输入框。
pub(crate) fn new_input(
    window: &mut Window,
    cx: &mut Context<LumarkApp>,
    placeholder: &str,
    masked: bool,
) -> gpui_kit::Entity<InputState> {
    cx.new(|cx| {
        let state = InputState::new(window, cx).placeholder(placeholder.to_string());
        if masked { state.masked(true) } else { state }
    })
}

/// 读取输入实体中的当前文本。
pub(crate) fn input_value(input: &gpui_kit::Entity<InputState>, cx: &gpui_kit::App) -> String {
    input.read(cx).value().to_string()
}

/// 替换输入框值，同时保持 GPUI 输入选择状态约束。
pub(crate) fn set_input(
    input: &gpui_kit::Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    input.update(cx, |state, cx| state.set_value(value, window, cx));
}

/// 构建聚焦的模态卡片；应用外壳负责遮罩背景，并在当前窗口居中显示该卡片。
///
/// 标题行使用浅色衬底和分隔线，与参考图片的窗体标题区保持同一层级感，
/// 同时让确认类对话框不必各自复制标题样式。
pub(crate) fn centered_modal(cx: &mut Context<LumarkApp>, title: &str) -> gpui_kit::Div {
    v_flex()
        .w(px(560.))
        .max_h(px(680.))
        .overflow_hidden()
        .gap_4()
        .p_5()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(10.))
        .shadow_xl()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .pb_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().size(px(8.)).rounded_full().bg(cx.theme().primary))
                .child(div().text_base().font_semibold().child(title.to_string())),
        )
}

/// 收集现有用户文件夹和收藏，供侧栏常用位置列表使用。
///
/// 磁盘根目录由下方可展开的目录树负责，避免同一位置在侧栏出现两次。
pub(crate) fn quick_locations(settings: &Settings) -> Vec<(String, PathBuf)> {
    let mut locations = Vec::new();
    let mut add = |label: &str, path: Option<PathBuf>| {
        if let Some(path) = path.filter(|path| path.is_dir())
            && !locations
                .iter()
                .any(|(_, existing): &(String, PathBuf)| existing == &path)
        {
            locations.push((label.to_string(), path));
        }
    };
    add("桌面", dirs::desktop_dir());
    add("下载", dirs::download_dir());
    add("文档", dirs::document_dir());
    add("图片", dirs::picture_dir());
    add("音乐", dirs::audio_dir());
    add("视频", dirs::video_dir());
    for path in &settings.favorites {
        add(
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("收藏"),
            Some(path.clone()),
        );
    }
    locations
}

/// 按 Windows 保留字符和尾随点规则校验文件名。
pub(crate) fn valid_file_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.chars().any(|character| {
            matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        })
        && !name.ends_with('.')
        && !name.ends_with(' ')
}

/// 把归档中选中的条目解压到本窗口专属临时目录，并返回可直接拖出/复制的顶层路径。
#[cfg(test)]
pub(crate) fn extract_archive_members(
    archive: &Path,
    password: Option<&str>,
    selected: &[(String, bool)],
    output: &Path,
) -> Result<Vec<(PathBuf, bool)>, String> {
    extract_archive_members_with_progress(archive, password, selected, output, |_| {})
}

/// 带进度回调的临时解压；供拖出和 Ctrl+C 进度弹窗使用。
pub(crate) fn extract_archive_members_with_progress(
    archive: &Path,
    password: Option<&str>,
    selected: &[(String, bool)],
    output: &Path,
    on_progress: impl FnMut(lumark_archive::ExtractProgress),
) -> Result<Vec<(PathBuf, bool)>, String> {
    if selected.is_empty() {
        return Err("没有选中的归档条目".to_string());
    }
    std::fs::create_dir_all(output).map_err(|error| error.to_string())?;
    let top = top_level_archive_paths(selected);
    let options = ExtractionOptions {
        output_dir: output.to_path_buf(),
        selected_paths: top.iter().map(|(path, _)| path.clone()).collect(),
        overwrite: true,
        overwrite_policy: ArchiveOverwritePolicy::Overwrite,
        password: password.map(|value| zeroize::Zeroizing::new(value.to_string())),
        ..Default::default()
    };
    ArchiveService::extract(archive, &options, on_progress).map_err(|error| error.to_string())?;
    let paths = top
        .into_iter()
        .filter_map(|(relative, is_dir)| {
            lumark_archive::safe_join(output, &relative)
                .ok()
                .filter(|path| path.exists())
                .map(|path| (path, is_dir))
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        Err("解压临时文件失败".to_string())
    } else {
        Ok(paths)
    }
}

/// 去掉被父目录覆盖的重复选择，只保留最外层条目。
pub(crate) fn top_level_archive_paths(selected: &[(String, bool)]) -> Vec<(String, bool)> {
    let mut top: Vec<(String, bool)> = Vec::new();
    for (path, is_dir) in selected {
        let covered = top
            .iter()
            .any(|(parent, _)| path == parent || path.starts_with(&format!("{parent}/")));
        if covered {
            continue;
        }
        top.retain(|(parent, _)| !parent.starts_with(&format!("{path}/")));
        top.push((path.clone(), *is_dir));
    }
    top
}

/// 删除超过启动保留期限的 Lumark 解压临时目录。
pub(crate) fn cleanup_stale_temp_dirs() {
    let root = std::env::temp_dir().join("Lumark").join("open");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(24 * 60 * 60))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map(|modified| modified < cutoff)
            .unwrap_or(false);
        if stale && path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// 检查路径扩展名是否由归档服务支持。
pub(crate) fn is_archive_path(path: &Path) -> bool {
    ArchiveService::detect(path) != ArchiveFormat::Unknown
}

/// 判断路径是否为 ISO 光盘映像。
pub(crate) fn is_iso_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"))
}

/// 识别需要显式打开确认的可执行文件和脚本扩展名。
pub(crate) fn is_executable_like(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            [
                "exe", "com", "bat", "cmd", "ps1", "vbs", "js", "jse", "wsf", "wsh", "msi", "scr",
                "cpl", "hta", "reg",
            ]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

/// 把持久化的可写格式偏好映射为归档格式。
pub(crate) fn format_from_settings(settings: &Settings) -> ArchiveFormat {
    match settings.default_format.to_ascii_lowercase().as_str() {
        "7z" | "sevenzip" => ArchiveFormat::SevenZip,
        "zpaq" => ArchiveFormat::Zpaq,
        "lzh" | "lha" => ArchiveFormat::Lzh,
        "tar" => ArchiveFormat::Tar,
        "tar.gz" | "tgz" => ArchiveFormat::TarGz,
        "tar.bz2" | "tbz2" => ArchiveFormat::TarBz2,
        "tar.xz" | "txz" => ArchiveFormat::TarXz,
        "tar.zst" | "tzst" => ArchiveFormat::TarZst,
        "tar.br" => ArchiveFormat::TarBr,
        "tar.lz4" => ArchiveFormat::TarLz4,
        "tar.lz" => ArchiveFormat::TarLz,
        "tar.lzma" => ArchiveFormat::TarLzma,
        "gz" | "gzip" => ArchiveFormat::Gzip,
        "bz2" => ArchiveFormat::Bzip2,
        "xz" => ArchiveFormat::Xz,
        "zst" => ArchiveFormat::Zstd,
        "br" | "brotli" => ArchiveFormat::Brotli,
        "lz4" => ArchiveFormat::Lz4,
        _ => ArchiveFormat::Zip,
    }
}

/// 判断当前源是否允许单文件流格式：必须恰好是一个普通文件。
pub(crate) fn single_stream_allowed(sources: &[PathBuf]) -> bool {
    sources.len() == 1 && sources.first().is_some_and(|path| path.is_file())
}

/// 多文件或文件夹源下把单文件流格式切换为对应的 TAR 变体。
///
/// 压缩窗口会用 `single_stream_allowed` 直接隐藏这些格式；此函数处理
/// 设置里的默认格式或已有会话格式，保证输出扩展名和实际写入格式一致。
pub(crate) fn resolve_compress_format(format: ArchiveFormat, sources: &[PathBuf]) -> ArchiveFormat {
    if format.is_single_stream() && !single_stream_allowed(sources) {
        format.single_stream_tar_variant().unwrap_or(format)
    } else {
        format
    }
}

/// 把用户可见的速度或体积预设映射为压缩级别。
pub(crate) fn level_for_preset(preset: SettingsPreset) -> i64 {
    match preset {
        SettingsPreset::Speed => 1,
        SettingsPreset::Balanced => 6,
        SettingsPreset::Size => 9,
    }
}

/// 把设置预设映射为归档后端的具体压缩预设。
pub(crate) fn archive_preset(preset: SettingsPreset) -> lumark_archive::CompressionPreset {
    match preset {
        SettingsPreset::Speed => lumark_archive::CompressionPreset::Speed,
        SettingsPreset::Balanced => lumark_archive::CompressionPreset::Balanced,
        SettingsPreset::Size => lumark_archive::CompressionPreset::Size,
    }
}

/// 从前面截断标题，保留末尾的路径信息。
///
/// 标题栏只显示一行，路径末尾（压缩包或文件名）比盘符更重要；超过
/// `limit` 个字符时保留末尾并在最前面加省略号，避免超长路径把右侧
/// 的工作区入口和系统按钮挤出窗口。
pub(crate) fn truncate_title(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    if total <= limit || limit == 0 {
        return text.to_string();
    }
    let keep = limit.saturating_sub(1);
    let tail = text.chars().skip(total - keep).collect::<String>();
    format!("…{tail}")
}

/// 按顺序切换到 Lumark 可创建的格式。
#[allow(dead_code)]
pub(crate) fn next_writable_format(format: ArchiveFormat) -> ArchiveFormat {
    match format {
        ArchiveFormat::Zip => ArchiveFormat::SevenZip,
        ArchiveFormat::SevenZip => ArchiveFormat::Zpaq,
        ArchiveFormat::Zpaq => ArchiveFormat::Lzh,
        ArchiveFormat::Lzh => ArchiveFormat::Tar,
        ArchiveFormat::Tar => ArchiveFormat::TarGz,
        ArchiveFormat::TarGz => ArchiveFormat::TarBz2,
        ArchiveFormat::TarBz2 => ArchiveFormat::TarXz,
        ArchiveFormat::TarXz => ArchiveFormat::TarZst,
        ArchiveFormat::TarZst => ArchiveFormat::TarBr,
        ArchiveFormat::TarBr => ArchiveFormat::TarLz4,
        ArchiveFormat::TarLz4 => ArchiveFormat::TarLz,
        ArchiveFormat::TarLz => ArchiveFormat::TarLzma,
        ArchiveFormat::TarLzma => ArchiveFormat::Gzip,
        ArchiveFormat::Gzip => ArchiveFormat::Bzip2,
        ArchiveFormat::Bzip2 => ArchiveFormat::Xz,
        ArchiveFormat::Xz => ArchiveFormat::Zstd,
        ArchiveFormat::Zstd => ArchiveFormat::Brotli,
        ArchiveFormat::Brotli => ArchiveFormat::Lz4,
        _ => ArchiveFormat::Zip,
    }
}

/// 去掉文件名末尾的归档、面具或自解压扩展名，返回主名。
///
/// 反复剥离已知后缀可以同时处理复合扩展名（`.tar.gz`）和用户多次切换
/// 格式/面具留下的历史后缀（例如旧版本产生的 `photos.mp4.mp4`），避免
/// 每次替换都再追加一层。只剥离已知后缀，因此 `my.photos` 这类带点的
/// 主名不会被误伤。
fn strip_output_extension(name: &str) -> &str {
    /// 路径剥壳时需要识别的归档、面具和自解压后缀。
    const SUFFIXES: [&str; 46] = [
        ".tar.lzma",
        ".tar.lz4",
        ".tar.zst",
        ".tar.br",
        ".tar.lz",
        ".tar.bz2",
        ".tar.gz",
        ".tar.xz",
        ".tar.z",
        ".taz",
        ".tgz",
        ".tbz2",
        ".txz",
        ".tzst",
        ".zip",
        ".7z",
        ".zpaq",
        ".zpq",
        ".lzh",
        ".lha",
        ".tar",
        ".gz",
        ".bz2",
        ".xz",
        ".zst",
        ".br",
        ".lz4",
        ".exe",
        ".mp4",
        ".m4v",
        ".mov",
        ".mkv",
        ".avi",
        ".webm",
        ".mp3",
        ".flac",
        ".jpg",
        ".jpeg",
        ".png",
        ".gif",
        ".bmp",
        ".webp",
        ".aac",
        ".wav",
        ".ogg",
        ".pdf",
    ];
    let mut stem = name;
    loop {
        let lower = stem.to_ascii_lowercase();
        let Some(suffix) = SUFFIXES.iter().find(|suffix| lower.ends_with(*suffix)) else {
            break;
        };
        let next = &stem[..stem.len() - suffix.len()];
        if next.is_empty() || next == "." || next == ".." {
            break;
        }
        stem = next;
    }
    stem
}

/// 判断文件名是否已经以指定扩展名结尾（大小写不敏感）。
fn ends_with_extension(name: &str, extension: &str) -> bool {
    name.to_ascii_lowercase()
        .ends_with(&format!(".{}", extension.to_ascii_lowercase()))
}

/// 用户切换归档格式时替换输出文件扩展名。
///
/// 会先剥掉归档、面具或 `.exe` 后缀，因此从伪装/自解压状态关闭时不会
/// 产生 `photos.mp4.zip` 这样的叠加扩展名。
pub(crate) fn replace_archive_extension(path: &Path, format: ArchiveFormat) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    let stripped = strip_output_extension(name);
    path.with_file_name(format!("{stripped}.{}", format.extension()))
}

/// 把输出扩展名替换为面具扩展名，用于开启伪装时同步文件名。
///
/// 输出名已经是“主名 + 目标扩展名”时原样返回，避免“压缩按钮按一次多一层
/// `.mp4`”；历史版本留下的 `photos.mp4.mp4` 会先剥离重复后缀再归一化。
pub(crate) fn replace_disguise_extension(path: &Path, extension: &str) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    let stripped = strip_output_extension(name);
    let suffix = format!(".{extension}");
    if name.len() == stripped.len() + suffix.len()
        && name[stripped.len()..].eq_ignore_ascii_case(&suffix)
    {
        return path.to_path_buf();
    }
    path.with_file_name(format!("{stripped}.{extension}"))
}

/// 替换面具扩展名，优先剥离调用方已知的上一次面具后缀。
///
/// 预设面具和常见媒体扩展名可以由 [`strip_output_extension`] 处理；自定义
/// 面具可能是任意扩展名，因此这里显式传入旧扩展名，避免切换自定义面具时
/// 变成 `photos.xyz.abc`。
pub(crate) fn replace_mask_extension(
    path: &Path,
    previous_extension: Option<&str>,
    extension: &str,
) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    if ends_with_extension(name, extension) {
        return replace_disguise_extension(path, extension);
    }
    if let Some(previous) = previous_extension {
        let suffix = format!(".{}", previous.to_ascii_lowercase());
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(&suffix) {
            let stem = &name[..name.len() - suffix.len()];
            return path.with_file_name(format!("{stem}.{extension}"));
        }
    }
    replace_disguise_extension(path, extension)
}

/// 把设置中的面具偏好解析为具体面具。
///
/// 自定义面具缺失或无效时返回可直接展示给用户的错误；预设面具始终可用。
pub(crate) fn resolve_disguise_mask(
    preference: DisguiseMaskPreference,
    custom: Option<&Path>,
) -> Result<DisguiseMask, String> {
    match preference {
        DisguiseMaskPreference::Video => Ok(DisguiseMask::PresetVideo),
        DisguiseMaskPreference::Image => Ok(DisguiseMask::PresetImage),
        DisguiseMaskPreference::Custom => {
            let path = custom.ok_or_else(|| "请选择自定义面具文件".to_string())?;
            let mask = DisguiseMask::File(path.to_path_buf());
            mask.validate().map_err(|error| error.to_string())?;
            Ok(mask)
        }
    }
}

/// 返回当前面具偏好对应的输出扩展名；无法解析时返回 `None`。
pub(crate) fn disguise_extension(
    preference: DisguiseMaskPreference,
    custom: Option<&Path>,
) -> Option<String> {
    resolve_disguise_mask(preference, custom)
        .ok()
        .and_then(|mask| mask.extension().ok())
}

/// 面具偏好在界面中的标签。
pub(crate) fn disguise_mask_label(preference: DisguiseMaskPreference) -> &'static str {
    match preference {
        DisguiseMaskPreference::Video => "视频（MP4）",
        DisguiseMaskPreference::Image => "图片（JPG）",
        DisguiseMaskPreference::Custom => "自定义文件",
    }
}

/// 把界面标签还原为面具偏好；未知标签回退到内置视频面具。
pub(crate) fn disguise_mask_from_label(label: &str) -> DisguiseMaskPreference {
    match label {
        "图片（JPG）" => DisguiseMaskPreference::Image,
        "自定义文件" => DisguiseMaskPreference::Custom,
        _ => DisguiseMaskPreference::Video,
    }
}

/// 面具下拉框的固定选项顺序。
pub(crate) const DISGUISE_MASK_LABELS: [&str; 3] = ["视频（MP4）", "图片（JPG）", "自定义文件"];

/// 返回面具偏好在下拉框中的索引。
pub(crate) fn disguise_mask_index(preference: DisguiseMaskPreference) -> usize {
    match preference {
        DisguiseMaskPreference::Video => 0,
        DisguiseMaskPreference::Image => 1,
        DisguiseMaskPreference::Custom => 2,
    }
}

/// 把分号分隔的匹配模式字段解析为规范化通配模式。
pub(crate) fn parse_patterns(value: &str) -> Vec<String> {
    value
        .split([';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// 解析 64M、1.5G 等人类可读字节大小。
pub(crate) fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    let (number, multiplier) = if let Some(value) = value.strip_suffix("gb") {
        (value, 1024u64 * 1024 * 1024)
    } else if let Some(value) = value.strip_suffix("mb") {
        (value, 1024u64 * 1024)
    } else if let Some(value) = value.strip_suffix("kb") {
        (value, 1024u64)
    } else if let Some(value) = value.strip_suffix('g') {
        (value, 1024u64 * 1024 * 1024)
    } else if let Some(value) = value.strip_suffix('m') {
        (value, 1024u64 * 1024)
    } else if let Some(value) = value.strip_suffix('k') {
        (value, 1024u64)
    } else {
        (value.as_str(), 1u64)
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| *number > 0.0)
        .map(|number| (number * multiplier as f64) as u64)
}

/// 分卷大小输入框可选的计量单位。
///
/// 数值输入框只保存数字，单位由右侧下拉框决定；用户也可以直接写出
/// `100M`、`2G` 等带后缀的写法，此时后缀优先于下拉单位。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SizeUnit {
    /// 字节。
    Byte,
    /// 千字节。
    Kilobyte,
    #[default]
    Megabyte,
    /// 吉字节。
    Gigabyte,
}

impl SizeUnit {
    /// 单位下拉框的固定顺序，Byte 到 GB 依次放大。
    pub(crate) const LABELS: [&'static str; 4] = ["Byte", "KB", "MB", "GB"];

    /// 返回该单位对应的字节倍数；沿用归档层 1024 进制的约定。
    pub(crate) fn multiplier(self) -> u64 {
        match self {
            Self::Byte => 1,
            Self::Kilobyte => 1024,
            Self::Megabyte => 1024 * 1024,
            Self::Gigabyte => 1024 * 1024 * 1024,
        }
    }

    /// 返回单位在 [`Self::LABELS`] 中的索引。
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Byte => 0,
            Self::Kilobyte => 1,
            Self::Megabyte => 2,
            Self::Gigabyte => 3,
        }
    }

    /// 按下拉标签还原单位；未知标签回退到 MB，避免界面出现空单位。
    pub(crate) fn from_label(label: &str) -> Self {
        match label {
            "Byte" => Self::Byte,
            "KB" => Self::Kilobyte,
            "GB" => Self::Gigabyte,
            _ => Self::Megabyte,
        }
    }
}

/// 解析“数字 + 下拉单位”形式的分卷大小。
///
/// 空值表示不分卷；带字母后缀的写法沿用 [`parse_size`] 的规则，因此
/// `100M`、`1.5G` 仍然有效，并且显式后缀优先于下拉单位。纯数字按所选
/// 单位换算，零值、负数和无法识别的文本返回 `None`。
pub(crate) fn parse_size_in_unit(value: &str, unit: SizeUnit) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value
        .chars()
        .last()
        .is_some_and(|value| value.is_ascii_alphabetic())
    {
        return parse_size(value);
    }
    let number = value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number > 0.0)?;
    let bytes = number * unit.multiplier() as f64;
    (bytes < u64::MAX as f64).then_some(bytes as u64)
}

/// 从紧凑日期字段解析可选的 UTC 日期过滤条件。
pub(crate) fn parse_date(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let date = chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let time = date.and_hms_opt(0, 0, 0)?;
    Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        time,
        chrono::Utc,
    ))
}

/// 使用二进制单位格式化字节数，供紧凑界面标签显示。
pub(crate) fn format_size(bytes: u64) -> String {
    /// 字节格式化使用的二进制单位。
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 使用最多两位二进制单位格式化分卷大小。
///
/// 与 [`format_size`] 的固定一位小数不同，这里去掉末尾的零，让
/// `4.38 GB`、`700 MB` 等常用预设回显时与用户选择的标签一致。
pub(crate) fn format_size_precise(bytes: u64) -> String {
    /// 字节格式化使用的二进制单位。
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text} {}", UNITS[unit])
}

/// 把秒数格式化为 HH:MM:SS，供任务进度窗口显示。
pub(crate) fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// 把密码记录时间转换为本地时区的紧凑文本。
pub(crate) fn format_record_time(value: chrono::DateTime<chrono::Utc>) -> String {
    value
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// 把点击、Ctrl 切换和 Shift 连选语义应用到文件行。
pub(crate) fn app_select_row(
    app: &mut LumarkApp,
    event: &gpui_kit::MouseDownEvent,
    target_id: &str,
) {
    if event.modifiers.shift {
        let ordered = app.displayed_ids();
        app.browser
            .selection
            .select_range(&ordered, target_id, true);
    } else if event.modifiers.control {
        app.browser.selection.toggle(target_id);
    } else {
        app.browser.selection.select_only(target_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证选中父目录时不会再把同一子树里的文件重复加入复制列表。
    #[test]
    fn top_level_archive_paths_drops_nested_selection() {
        let selected = vec![
            ("docs".to_string(), true),
            ("docs/readme.txt".to_string(), false),
            ("photo.png".to_string(), false),
        ];
        assert_eq!(
            top_level_archive_paths(&selected),
            vec![("docs".to_string(), true), ("photo.png".to_string(), false)]
        );
    }

    /// 验证归档目录能解压到临时目录并返回可直接复制/拖出的顶层路径。
    #[test]
    fn archive_members_extract_to_temp_paths() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("docs");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("readme.txt"), b"hello").unwrap();
        let archive = directory.path().join("docs.zip");
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format: ArchiveFormat::Zip,
                path_mode: PathMode::Relative,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let output = directory.path().join("output");
        let members =
            extract_archive_members(&archive, None, &[("docs".to_string(), true)], &output)
                .unwrap();
        assert_eq!(members.len(), 1);
        assert!(members[0].1);
        assert!(output.join("docs/readme.txt").exists());
    }

    /// 验证设置里的 ZPAQ 默认格式和输出扩展名替换。
    #[test]
    fn zpaq_settings_and_extension_replace() {
        let settings = Settings {
            default_format: "zpaq".to_string(),
            ..Settings::default()
        };
        assert_eq!(format_from_settings(&settings), ArchiveFormat::Zpaq);
        assert_eq!(
            replace_archive_extension(Path::new(r"C:\data\backup.zip"), ArchiveFormat::Zpaq),
            PathBuf::from(r"C:\data\backup.zpaq")
        );
        assert_eq!(
            replace_archive_extension(Path::new(r"C:\data\backup.zpaq"), ArchiveFormat::Zip),
            PathBuf::from(r"C:\data\backup.zip")
        );
        assert!(ArchiveFormat::Zpaq.is_writable());
        assert!(ArchiveFormat::Zpaq.supports_password());
    }

    /// 验证新增可写格式的设置映射和扩展名替换。
    #[test]
    fn new_writable_settings_and_extension_replace() {
        for (extension, format) in [
            ("lzh", ArchiveFormat::Lzh),
            ("tar.br", ArchiveFormat::TarBr),
            ("tar.lz4", ArchiveFormat::TarLz4),
            ("tar.lz", ArchiveFormat::TarLz),
            ("tar.lzma", ArchiveFormat::TarLzma),
            ("br", ArchiveFormat::Brotli),
            ("lz4", ArchiveFormat::Lz4),
        ] {
            let settings = Settings {
                default_format: extension.to_string(),
                ..Settings::default()
            };
            assert_eq!(format_from_settings(&settings), format, "{extension}");
            let source = format!(r"C:\data\backup.{extension}");
            assert_eq!(
                replace_archive_extension(Path::new(&source), ArchiveFormat::Zip),
                PathBuf::from(r"C:\data\backup.zip"),
                "{extension}"
            );
        }
        assert_eq!(
            replace_archive_extension(Path::new(r"C:\data\backup.taz"), ArchiveFormat::Zip),
            PathBuf::from(r"C:\data\backup.zip")
        );
        let lha = Settings {
            default_format: "lha".to_string(),
            ..Settings::default()
        };
        assert_eq!(format_from_settings(&lha), ArchiveFormat::Lzh);
        assert_eq!(
            replace_archive_extension(Path::new(r"C:\data\backup.lha"), ArchiveFormat::Zip),
            PathBuf::from(r"C:\data\backup.zip")
        );
    }

    /// 验证单流格式只允许单个普通文件，多文件或文件夹自动改用 TAR 变体。
    #[test]
    fn single_stream_formats_resolve_by_source_shape() {
        let directory = tempdir().unwrap();
        let one = directory.path().join("one.docx");
        let two = directory.path().join("two.docx");
        std::fs::write(&one, b"one").unwrap();
        std::fs::write(&two, b"two").unwrap();
        let folder = directory.path().join("folder");
        std::fs::create_dir(&folder).unwrap();

        assert!(single_stream_allowed(std::slice::from_ref(&one)));
        assert!(!single_stream_allowed(&[one.clone(), two.clone()]));
        assert!(!single_stream_allowed(std::slice::from_ref(&folder)));
        assert!(!single_stream_allowed(&[]));

        assert_eq!(
            resolve_compress_format(ArchiveFormat::Brotli, std::slice::from_ref(&one)),
            ArchiveFormat::Brotli
        );
        assert_eq!(
            resolve_compress_format(ArchiveFormat::Brotli, &[one.clone(), two.clone()]),
            ArchiveFormat::TarBr
        );
        assert_eq!(
            resolve_compress_format(ArchiveFormat::Brotli, &[folder]),
            ArchiveFormat::TarBr
        );
        assert_eq!(
            resolve_compress_format(ArchiveFormat::Zip, &[one, two]),
            ArchiveFormat::Zip
        );
    }

    /// 分卷提示保留两位精度，并去掉多余的小数零。
    #[test]
    fn precise_size_keeps_split_preset_precision() {
        assert_eq!(format_size_precise(512 * 1024), "512 KB");
        assert_eq!(format_size_precise(700 * 1024 * 1024), "700 MB");
        assert_eq!(format_size_precise(1_610_612_736), "1.5 GB");
        assert_eq!(format_size_precise(4_702_989_189), "4.38 GB");
        assert_eq!(format_size_precise(100), "100 B");
    }

    /// 验证面具偏好解析、扩展名替换和自定义面具缺失时的提示。
    #[test]
    fn disguise_mask_resolution_and_extension_replace() {
        assert_eq!(
            resolve_disguise_mask(DisguiseMaskPreference::Video, None).unwrap(),
            DisguiseMask::PresetVideo
        );
        assert_eq!(
            resolve_disguise_mask(DisguiseMaskPreference::Image, None).unwrap(),
            DisguiseMask::PresetImage
        );
        assert_eq!(
            resolve_disguise_mask(DisguiseMaskPreference::Video, None)
                .unwrap()
                .extension()
                .unwrap(),
            "mp4"
        );
        assert!(
            resolve_disguise_mask(DisguiseMaskPreference::Custom, None)
                .unwrap_err()
                .contains("自定义面具")
        );

        let directory = tempdir().unwrap();
        let mask = directory.path().join("cover.jpg");
        std::fs::write(&mask, vec![0x42; 4096]).unwrap();
        assert_eq!(
            resolve_disguise_mask(DisguiseMaskPreference::Custom, Some(&mask))
                .unwrap()
                .extension()
                .unwrap(),
            "jpg"
        );
        assert_eq!(
            disguise_extension(DisguiseMaskPreference::Custom, Some(&mask)).as_deref(),
            Some("jpg")
        );
        assert_eq!(
            disguise_extension(DisguiseMaskPreference::Image, None).as_deref(),
            Some("jpg")
        );
        assert_eq!(
            disguise_mask_label(DisguiseMaskPreference::Video),
            "视频（MP4）"
        );
        assert_eq!(
            disguise_mask_from_label("自定义文件"),
            DisguiseMaskPreference::Custom
        );
        assert_eq!(disguise_mask_index(DisguiseMaskPreference::Image), 1);

        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\backup.tar.gz"), "mp4"),
            PathBuf::from(r"C:\data\backup.mp4")
        );
        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\backup.7z"), "jpg"),
            PathBuf::from(r"C:\data\backup.jpg")
        );
        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\backup"), "mp4"),
            PathBuf::from(r"C:\data\backup.mp4")
        );
    }

    /// 回归：伪装输出不能再重复追加 `.mp4`，关闭伪装也不能留下叠加后缀。
    #[test]
    fn disguise_extension_is_not_appended_repeatedly() {
        // 已经是目标扩展名：保持原样，这是“压缩一次多一个 .mp4”的直接回归点。
        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\photos.mp4"), "mp4"),
            PathBuf::from(r"C:\data\photos.mp4")
        );
        // 历史版本留下的多层后缀：一次归一化。
        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\photos.mp4.mp4"), "mp4"),
            PathBuf::from(r"C:\data\photos.mp4")
        );
        // 切换面具：替换而不是追加。
        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\photos.mp4"), "jpg"),
            PathBuf::from(r"C:\data\photos.jpg")
        );
        // 带点的主名不能被误伤。
        assert_eq!(
            replace_disguise_extension(Path::new(r"C:\data\my.photos"), "mp4"),
            PathBuf::from(r"C:\data\my.photos.mp4")
        );
        // 关闭伪装/切换归档格式：剥掉面具或自解压后缀。
        assert_eq!(
            replace_archive_extension(Path::new(r"C:\data\photos.mp4"), ArchiveFormat::Zip),
            PathBuf::from(r"C:\data\photos.zip")
        );
        assert_eq!(
            replace_archive_extension(Path::new(r"C:\data\photos.exe"), ArchiveFormat::Zip),
            PathBuf::from(r"C:\data\photos.zip")
        );
        // 自定义面具切换：显式剥离旧扩展名。
        assert_eq!(
            replace_mask_extension(Path::new(r"C:\data\photos.xyz"), Some("xyz"), "abc"),
            PathBuf::from(r"C:\data\photos.abc")
        );
        assert_eq!(
            replace_mask_extension(Path::new(r"C:\data\photos.jpg"), Some("jpg"), "mp4"),
            PathBuf::from(r"C:\data\photos.mp4")
        );
    }
}
