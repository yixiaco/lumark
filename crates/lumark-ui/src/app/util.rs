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
pub(crate) fn extract_archive_members(
    archive: &Path,
    password: Option<&str>,
    selected: &[(String, bool)],
    output: &Path,
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
    ArchiveService::extract(archive, &options, |_| {}).map_err(|error| error.to_string())?;
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

/// 返回紧凑、标准或宽松文件列表布局的列宽。
pub(crate) fn column_layout_widths(layout: u8) -> (f32, f32, f32) {
    match layout {
        0 => (72.0, 126.0, 56.0),
        2 => (110.0, 160.0, 92.0),
        _ => (88.0, 142.0, 72.0),
    }
}

/// 把持久化的可写格式偏好映射为归档格式。
pub(crate) fn format_from_settings(settings: &Settings) -> ArchiveFormat {
    match settings.default_format.to_ascii_lowercase().as_str() {
        "7z" | "sevenzip" => ArchiveFormat::SevenZip,
        "tar" => ArchiveFormat::Tar,
        "tar.gz" | "tgz" => ArchiveFormat::TarGz,
        "tar.bz2" | "tbz2" => ArchiveFormat::TarBz2,
        "tar.xz" | "txz" => ArchiveFormat::TarXz,
        "tar.zst" | "tzst" => ArchiveFormat::TarZst,
        "gz" | "gzip" => ArchiveFormat::Gzip,
        "bz2" => ArchiveFormat::Bzip2,
        "xz" => ArchiveFormat::Xz,
        "zst" => ArchiveFormat::Zstd,
        "br" | "brotli" => ArchiveFormat::Brotli,
        "lz4" => ArchiveFormat::Lz4,
        _ => ArchiveFormat::Zip,
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
        ArchiveFormat::SevenZip => ArchiveFormat::Tar,
        ArchiveFormat::Tar => ArchiveFormat::TarGz,
        ArchiveFormat::TarGz => ArchiveFormat::TarBz2,
        ArchiveFormat::TarBz2 => ArchiveFormat::TarXz,
        ArchiveFormat::TarXz => ArchiveFormat::TarZst,
        ArchiveFormat::TarZst => ArchiveFormat::Gzip,
        ArchiveFormat::Gzip => ArchiveFormat::Bzip2,
        ArchiveFormat::Bzip2 => ArchiveFormat::Xz,
        ArchiveFormat::Xz => ArchiveFormat::Zstd,
        ArchiveFormat::Zstd => ArchiveFormat::Brotli,
        ArchiveFormat::Brotli => ArchiveFormat::Lz4,
        _ => ArchiveFormat::Zip,
    }
}

/// 用户切换归档格式时替换输出文件扩展名。
pub(crate) fn replace_archive_extension(path: &Path, format: ArchiveFormat) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    let lower = name.to_ascii_lowercase();
    let stripped = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".txz", ".tzst", ".zip",
        ".7z", ".tar", ".gz", ".bz2", ".xz", ".zst", ".br", ".lz4",
    ]
    .iter()
    .find_map(|suffix| {
        lower
            .strip_suffix(suffix)
            .map(|_| &name[..name.len() - suffix.len()])
    })
    .unwrap_or(name);
    path.with_file_name(format!("{stripped}.{}", format.extension()))
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

/// 从紧凑日期字段解析可选的 UTC 日期过滤条件。
pub(crate) fn parse_date(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let date = chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let time = date.and_hms_opt(0, 0, 0)?;
    Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        time,
        chrono::Utc,
    ))
}

/// 追加数字后缀，直到得到未占用的归档输出路径。
pub(crate) fn unique_output_path(path: PathBuf, format: ArchiveFormat) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive");
    let suffix = format!(".{}", format.extension());
    let stem = name.strip_suffix(&suffix).unwrap_or("archive");
    for index in 1..1000 {
        let candidate = parent.join(format!("{stem} ({index}){suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

/// 使用二进制单位格式化字节数，供紧凑界面标签显示。
pub(crate) fn format_size(bytes: u64) -> String {
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
}
