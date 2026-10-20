use super::super::*;

/// Lumark 标题栏和文件行使用的 IconPark 图标。
///
/// SVG 在编译时打包，因此界面运行时无需访问网络。
/// 每个图标使用 `currentColor`，便于主题为浅色和深色模式着色，
/// 无需维护多套图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppIcon {
    Add,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Check,
    Close,
    Compression,
    Copy,
    Delete,
    Doc,
    Download,
    Edit,
    FileLock,
    FileZip,
    FolderOpen,
    FolderPlus,
    Grid,
    Home,
    Key,
    List,
    Lock,
    More,
    Music,
    Picture,
    Refresh,
    Save,
    Search,
    Setting,
    Shield,
    Time,
    Tool,
    Unlock,
    Video,
    Zip,
}

impl AppIcon {
    /// 返回一个 IconPark 图标的嵌入 SVG 字节。
    pub(super) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Add => include_bytes!("../../../assets/icons/iconpark/add.svg"),
            Self::ArrowLeft => include_bytes!("../../../assets/icons/iconpark/arrow-left.svg"),
            Self::ArrowRight => include_bytes!("../../../assets/icons/iconpark/arrow-right.svg"),
            Self::ArrowUp => include_bytes!("../../../assets/icons/iconpark/arrow-up.svg"),
            Self::Check => include_bytes!("../../../assets/icons/iconpark/check.svg"),
            Self::Close => include_bytes!("../../../assets/icons/iconpark/close.svg"),
            Self::Compression => {
                include_bytes!("../../../assets/icons/iconpark/compression.svg")
            }
            Self::Copy => include_bytes!("../../../assets/icons/iconpark/copy.svg"),
            Self::Delete => include_bytes!("../../../assets/icons/iconpark/delete.svg"),
            Self::Doc => include_bytes!("../../../assets/icons/iconpark/doc-detail.svg"),
            Self::Download => include_bytes!("../../../assets/icons/iconpark/download.svg"),
            Self::Edit => include_bytes!("../../../assets/icons/iconpark/edit.svg"),
            Self::FileLock => include_bytes!("../../../assets/icons/iconpark/file-lock.svg"),
            Self::FileZip => include_bytes!("../../../assets/icons/iconpark/file-zip.svg"),
            Self::FolderOpen => include_bytes!("../../../assets/icons/iconpark/folder-open.svg"),
            Self::FolderPlus => include_bytes!("../../../assets/icons/iconpark/folder-plus.svg"),
            Self::Grid => include_bytes!("../../../assets/icons/iconpark/grid-four.svg"),
            Self::Home => include_bytes!("../../../assets/icons/iconpark/home.svg"),
            Self::Key => include_bytes!("../../../assets/icons/iconpark/key.svg"),
            Self::List => include_bytes!("../../../assets/icons/iconpark/list.svg"),
            Self::Lock => include_bytes!("../../../assets/icons/iconpark/lock.svg"),
            Self::More => include_bytes!("../../../assets/icons/iconpark/more.svg"),
            Self::Music => include_bytes!("../../../assets/icons/iconpark/music.svg"),
            Self::Picture => include_bytes!("../../../assets/icons/iconpark/picture.svg"),
            Self::Refresh => include_bytes!("../../../assets/icons/iconpark/refresh.svg"),
            Self::Save => include_bytes!("../../../assets/icons/iconpark/save.svg"),
            Self::Search => include_bytes!("../../../assets/icons/iconpark/search.svg"),
            Self::Setting => include_bytes!("../../../assets/icons/iconpark/setting.svg"),
            Self::Shield => include_bytes!("../../../assets/icons/iconpark/shield.svg"),
            Self::Time => include_bytes!("../../../assets/icons/iconpark/time.svg"),
            Self::Tool => include_bytes!("../../../assets/icons/iconpark/tool.svg"),
            Self::Unlock => include_bytes!("../../../assets/icons/iconpark/unlock.svg"),
            Self::Video => include_bytes!("../../../assets/icons/iconpark/video.svg"),
            Self::Zip => include_bytes!("../../../assets/icons/iconpark/zip.svg"),
        }
    }

    /// 构建继承周围文字颜色的可缩放图标。
    pub(super) fn element(self, size: f32) -> gpui_kit::component::Icon {
        gpui_kit::component::Icon::default()
            .data(self.bytes())
            .size(px(size))
    }
}

/// 创建紧凑且带提示的命令按钮。
pub(super) fn icon_button(
    id: impl Into<gpui_kit::ElementId>,
    icon: AppIcon,
    tooltip: &'static str,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon.element(17.))
        .tooltip(tooltip)
        .accessibility_label(tooltip)
}

/// 创建带 IconPark 图标的主命令按钮。
pub(super) fn primary_icon_button(
    id: impl Into<gpui_kit::ElementId>,
    icon: AppIcon,
    label: &'static str,
) -> Button {
    Button::new(id)
        .primary()
        .icon(icon.element(16.))
        .label(label)
        .accessibility_label(label)
}

/// 创建带 IconPark 图标的次要命令按钮。
pub(super) fn secondary_icon_button(
    id: impl Into<gpui_kit::ElementId>,
    icon: AppIcon,
    label: &'static str,
) -> Button {
    Button::new(id)
        .secondary()
        .icon(icon.element(16.))
        .label(label)
        .accessibility_label(label)
}

/// 文件列表和设置分组使用的标准卡片表面。
pub(super) fn card(cx: &Context<LumarkApp>) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .rounded_lg()
        .overflow_hidden()
}

/// 带可选尾部说明的分区标题。
pub(super) fn section_heading(
    cx: &Context<LumarkApp>,
    icon: AppIcon,
    title: &str,
    caption: Option<String>,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(28.))
                .rounded_md()
                .bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
                .flex()
                .items_center()
                .justify_center()
                .child(icon.element(16.)),
        )
        .child(div().font_semibold().child(title.to_string()))
        .when(caption.is_some(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(caption.unwrap_or_default()),
            )
        })
}

/// 把文件行映射为易识别的 IconPark 图标。
pub(super) fn entry_icon(entry: &BrowserEntry) -> AppIcon {
    if entry.is_dir {
        return AppIcon::FolderOpen;
    }
    if entry.encrypted {
        return AppIcon::FileLock;
    }
    let lower = entry.name.to_ascii_lowercase();
    if [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".zst", ".cab", ".iso",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
    {
        return AppIcon::Zip;
    }
    if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        return AppIcon::Picture;
    }
    if [".mp3", ".flac", ".wav", ".m4a", ".ogg"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        return AppIcon::Music;
    }
    if [".mp4", ".mkv", ".avi", ".mov", ".webm"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        return AppIcon::Video;
    }
    AppIcon::Doc
}

/// 把快捷位置映射为 IconPark 图标。
pub(super) fn location_icon(label: &str) -> AppIcon {
    match label {
        "桌面" | "主文件夹" => AppIcon::Home,
        "下载" => AppIcon::Download,
        "文档" => AppIcon::Doc,
        "图片" => AppIcon::Picture,
        "音乐" => AppIcon::Music,
        "视频" => AppIcon::Video,
        _ if label.ends_with(':') => AppIcon::Home,
        _ => AppIcon::FolderOpen,
    }
}
