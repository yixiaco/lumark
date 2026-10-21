use super::super::*;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::radio::Radio;

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
    Down,
    Download,
    Edit,
    FileLock,
    FileZip,
    Folder,
    FolderOpen,
    FolderPlus,
    Grid,
    Home,
    Key,
    List,
    Lock,
    MenuFold,
    MenuUnfold,
    Music,
    Picture,
    Refresh,
    Right,
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
            Self::Down => include_bytes!("../../../assets/icons/iconpark/down.svg"),
            Self::Download => include_bytes!("../../../assets/icons/iconpark/download.svg"),
            Self::Edit => include_bytes!("../../../assets/icons/iconpark/edit.svg"),
            Self::FileLock => include_bytes!("../../../assets/icons/iconpark/file-lock.svg"),
            Self::FileZip => include_bytes!("../../../assets/icons/iconpark/file-zip.svg"),
            Self::Folder => include_bytes!("../../../assets/icons/iconpark/folder.svg"),
            Self::FolderOpen => include_bytes!("../../../assets/icons/iconpark/folder-open.svg"),
            Self::FolderPlus => include_bytes!("../../../assets/icons/iconpark/folder-plus.svg"),
            Self::Grid => include_bytes!("../../../assets/icons/iconpark/grid-four.svg"),
            Self::Home => include_bytes!("../../../assets/icons/iconpark/home.svg"),
            Self::Key => include_bytes!("../../../assets/icons/iconpark/key.svg"),
            Self::List => include_bytes!("../../../assets/icons/iconpark/list.svg"),
            Self::Lock => include_bytes!("../../../assets/icons/iconpark/lock.svg"),
            Self::MenuFold => include_bytes!("../../../assets/icons/iconpark/menu-fold.svg"),
            Self::MenuUnfold => include_bytes!("../../../assets/icons/iconpark/menu-unfold.svg"),
            Self::Music => include_bytes!("../../../assets/icons/iconpark/music.svg"),
            Self::Picture => include_bytes!("../../../assets/icons/iconpark/picture.svg"),
            Self::Refresh => include_bytes!("../../../assets/icons/iconpark/refresh.svg"),
            Self::Right => include_bytes!("../../../assets/icons/iconpark/right.svg"),
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
    button(id)
        .ghost()
        .small()
        .rounded(px(6.))
        .cursor_pointer()
        .icon(icon.element(17.))
        .tooltip(tooltip)
        .accessibility_label(tooltip)
}

/// 页面分组使用的标准卡片表面。
///
/// 页面底色保持浅灰，卡片使用浮层白面、细边框和轻阴影，让设置、密码库和
/// 任务中心的分组从背景中分离出来，而不是整页糊成同一层灰色。
pub(super) fn card(cx: &Context<LumarkApp>) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(10.))
        .shadow_sm()
        .overflow_hidden()
}

/// 顶部动作带按钮的语义色调。
///
/// 所有动作统一使用官方默认 Button 表面，只让图标按动作类别着色，
/// 避免五种彩色填充在横幅上互相争夺注意力。
#[derive(Clone, Copy, Debug)]
pub(super) enum BannerTone {
    Primary,
    Success,
    Warning,
    Danger,
    Neutral,
}

impl BannerTone {
    /// 返回动作图标在当前主题下的颜色。
    fn icon_color(self, cx: &gpui_kit::App) -> gpui_kit::Hsla {
        match self {
            Self::Primary => cx.theme().primary,
            Self::Success => cx.theme().success_foreground,
            Self::Warning => cx.theme().warning_foreground,
            Self::Danger => cx.theme().danger_foreground,
            Self::Neutral => cx.theme().muted_foreground,
        }
    }
}

/// 渲染顶部动作带中的纵向官方 Button。
///
/// 图标和文字由同一个 v_flex 提供，Button 仍负责圆角、边框、悬停、
/// 焦点环和键盘激活；固定尺寸让五个动作在宽窄窗口下保持同一基线。
pub(super) fn banner_action(
    cx: &Context<LumarkApp>,
    id: &'static str,
    icon: AppIcon,
    label: &'static str,
    tone: BannerTone,
    handler: impl Fn(&mut LumarkApp, &mut Window, &mut Context<LumarkApp>) + 'static,
) -> gpui_kit::AnyElement {
    let icon_color = tone.icon_color(cx);
    let label_color = cx.theme().foreground;
    button(id)
        .rounded(px(8.))
        .w(px(68.))
        .h(px(56.))
        .px_0()
        .tooltip(label)
        .accessibility_label(label)
        .child(
            v_flex()
                .items_center()
                .justify_center()
                .gap_1()
                .child(div().text_color(icon_color).child(icon.element(22.)))
                .child(div().text_xs().text_color(label_color).child(label)),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            handler(this, window, cx);
            cx.notify();
        }))
        .into_any_element()
}

/// 返回从主题强调色浅色过渡到页面底色的横幅背景。
///
/// 渐变只取当前主题的主色和中性表面色，浅色和深色模式各自保持可读对比，
/// 用户修改主题色后动作带和对话框横幅会同步换色。
pub(super) fn banner_gradient(cx: &gpui_kit::App) -> gpui_kit::Div {
    let dark = cx.theme().mode.is_dark();
    let primary = cx.theme().primary;
    let (from, to) = if dark {
        (
            hsla(primary.h, 0.34, 0.17, 1.),
            hsla(217. / 360., 0.28, 0.10, 1.),
        )
    } else {
        (
            hsla(primary.h, 0.58, 0.94, 1.),
            hsla(214. / 360., 0.55, 0.99, 1.),
        )
    };
    div().bg(gpui_kit::linear_gradient(
        110.,
        gpui_kit::linear_color_stop(from, 0.),
        gpui_kit::linear_color_stop(to, 1.),
    ))
}

/// 渲染标题栏、动作带和对话框共用的品牌标识。
pub(super) fn brand_lockup(cx: &gpui_kit::App, slogan: bool) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(34.))
                .rounded(px(9.))
                .bg(cx.theme().button_primary)
                .text_color(cx.theme().button_primary_foreground)
                .flex()
                .items_center()
                .justify_center()
                .shadow_sm()
                .child(AppIcon::Compression.element(20.)),
        )
        .child(
            v_flex()
                .gap(px(0.))
                .child(
                    div()
                        .text_base()
                        .font_semibold()
                        .whitespace_nowrap()
                        .child("Lumark"),
                )
                .when(slogan, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .whitespace_nowrap()
                            .text_color(cx.theme().muted_foreground)
                            .child("高效专业 · 值得信赖"),
                    )
                }),
        )
}

/// 渲染压缩、解压对话框顶部的品牌横幅。
pub(super) fn dialog_banner(cx: &gpui_kit::App, height: f32) -> impl IntoElement {
    banner_gradient(cx)
        .w_full()
        .h(px(height))
        .px_5()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(brand_lockup(cx, false))
        .child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child("本地归档工作台 · 高效专业，值得信赖"),
        )
}

/// 渲染经典压缩选项卡中的分组框。
///
/// 使用组件库 GroupBox 的 outline 变体，标题、内边距、边框和圆角
/// 由主题统一提供，避免再手写一套 Win32 边框。
pub(super) fn classic_group(
    _cx: &gpui_kit::App,
    title: &str,
    content: impl IntoElement,
) -> impl IntoElement {
    GroupBox::new()
        .outline()
        .title(title.to_string())
        .mt_3()
        .child(content)
}

/// 渲染左标签右控件的经典表单行。
pub(super) fn classic_field(
    label: &str,
    width: f32,
    control: impl IntoElement,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_3()
        .items_center()
        .child(
            div()
                .w(px(width))
                .flex_shrink_0()
                .text_sm()
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// 渲染一行小号辅助说明文字。
pub(super) fn hint_text(cx: &gpui_kit::App, text: impl Into<String>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// 渲染官方单选按钮。
///
/// 组状态仍由压缩预设字段维护；组件库负责圆点、焦点环、悬停和键盘激活。
#[allow(dead_code)]
pub(super) fn option_radio(
    cx: &Context<LumarkApp>,
    id: &'static str,
    label: String,
    checked: bool,
    handler: impl Fn(&mut LumarkApp, &mut Window, &mut Context<LumarkApp>) + 'static,
) -> gpui_kit::AnyElement {
    Radio::new(id)
        .label(label)
        .checked(checked)
        .on_click(cx.listener(move |this, _, window, cx| {
            handler(this, window, cx);
            cx.notify();
        }))
        .into_any_element()
}

/// 渲染带图标、标题和说明的分区标题。
///
/// 标题与说明改为上下排列，并在右侧延伸到卡片边缘的细线，形成比单行
/// 小字更清晰的分组层级。
pub(super) fn section_heading(
    cx: &Context<LumarkApp>,
    icon: AppIcon,
    title: &str,
    caption: Option<String>,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .items_center()
        .gap_3()
        .child(
            div()
                .size(px(34.))
                .flex_shrink_0()
                .rounded(px(9.))
                .bg(cx.theme().primary.opacity(0.12))
                .text_color(cx.theme().primary)
                .flex()
                .items_center()
                .justify_center()
                .child(icon.element(17.)),
        )
        .child(
            v_flex()
                .min_w_0()
                .gap(px(1.))
                .child(div().text_base().font_semibold().child(title.to_string()))
                .when(caption.is_some(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(caption.unwrap_or_default()),
                    )
                }),
        )
        .child(div().flex_1().h(px(1.)).bg(cx.theme().border))
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

/// 按文件类别返回参考图片中彩色文件图标的主题色。
///
/// 颜色只取自主题语义色，因此深浅色切换时仍保持足够对比。
pub(super) fn entry_icon_color(cx: &Context<LumarkApp>, entry: &BrowserEntry) -> gpui_kit::Hsla {
    if entry.encrypted {
        return cx.theme().danger_foreground;
    }
    if entry.is_dir {
        return cx.theme().warning_foreground;
    }
    match entry_icon(entry) {
        AppIcon::Zip => cx.theme().accent_foreground,
        AppIcon::Picture => cx.theme().success_foreground,
        AppIcon::Music | AppIcon::Video => cx.theme().primary,
        AppIcon::FileLock => cx.theme().danger_foreground,
        _ => cx.theme().muted_foreground,
    }
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
