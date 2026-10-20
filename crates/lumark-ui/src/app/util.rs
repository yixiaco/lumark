use super::*;

/// Creates a themed text input with optional password masking.
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

/// Reads the current text from an input entity.
pub(crate) fn input_value(input: &gpui_kit::Entity<InputState>, cx: &gpui_kit::App) -> String {
    input.read(cx).value().to_string()
}

/// Replaces an input value while preserving the GPUI input selection invariants.
pub(crate) fn set_input(
    input: &gpui_kit::Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<LumarkApp>,
) {
    input.update(cx, |state, cx| state.set_value(value, window, cx));
}

/// Builds a focused modal card; the application shell supplies the dimming
/// backdrop and centers this card in the active window.
pub(crate) fn centered_modal(cx: &mut Context<LumarkApp>, title: &str) -> gpui_kit::Div {
    v_flex()
        .w(px(680.))
        .max_h(px(720.))
        .overflow_hidden()
        .gap_4()
        .p_6()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(16.))
        .shadow_xl()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(div().size(px(8.)).rounded_full().bg(cx.theme().primary))
                .child(div().text_lg().font_semibold().child(title.to_string())),
        )
}

/// Collects existing user folders, favorites, and mounted drive roots for the sidebar.
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
    for letter in b'A'..=b'Z' {
        let path = PathBuf::from(format!("{}:\\", letter as char));
        if path.is_dir() {
            add(&format!("{}:", letter as char), Some(path));
        }
    }
    locations
}

/// Validates a filename against Windows reserved characters and trailing-dot rules.
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

/// Removes Lumark extraction directories that are older than the startup retention window.
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

/// Checks whether a path has an extension understood by the archive service.
pub(crate) fn is_archive_path(path: &Path) -> bool {
    ArchiveService::detect(path) != ArchiveFormat::Unknown
}

/// Recognizes executable and script extensions that require an explicit open confirmation.
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

/// Returns the column widths for compact, standard, or relaxed file-list layouts.
pub(crate) fn column_layout_widths(layout: u8) -> (f32, f32, f32) {
    match layout {
        0 => (64.0, 108.0, 56.0),
        2 => (108.0, 168.0, 88.0),
        _ => (80.0, 132.0, 70.0),
    }
}

/// Maps the persisted writable format preference to an archive format.
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

/// Maps a user-facing speed or size preset to its compression level.
pub(crate) fn level_for_preset(preset: SettingsPreset) -> i64 {
    match preset {
        SettingsPreset::Speed => 1,
        SettingsPreset::Balanced => 6,
        SettingsPreset::Size => 9,
    }
}

/// Maps a settings preset to the archive backend's detailed compression preset.
pub(crate) fn archive_preset(preset: SettingsPreset) -> lumark_archive::CompressionPreset {
    match preset {
        SettingsPreset::Speed => lumark_archive::CompressionPreset::Speed,
        SettingsPreset::Balanced => lumark_archive::CompressionPreset::Balanced,
        SettingsPreset::Size => lumark_archive::CompressionPreset::Size,
    }
}

/// Advances through the compression presets in display order.
pub(crate) fn next_preset(preset: SettingsPreset) -> SettingsPreset {
    match preset {
        SettingsPreset::Speed => SettingsPreset::Balanced,
        SettingsPreset::Balanced => SettingsPreset::Size,
        SettingsPreset::Size => SettingsPreset::Speed,
    }
}

/// Advances through formats that Lumark can create.
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

/// Replaces an output file extension when the user changes the archive format.
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

/// Parses a semicolon-separated pattern field into normalized glob patterns.
pub(crate) fn parse_patterns(value: &str) -> Vec<String> {
    value
        .split([';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Parses human-readable byte sizes such as 64M or 1.5G.
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

/// Parses an optional UTC date filter from a compact date field.
pub(crate) fn parse_date(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let date = chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let time = date.and_hms_opt(0, 0, 0)?;
    Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        time,
        chrono::Utc,
    ))
}

/// Adds a numeric suffix until a new archive output path is unused.
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

/// Formats a byte count with binary units for compact UI labels.
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

/// Applies click, Ctrl-toggle, and Shift-range selection semantics to a file row.
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
