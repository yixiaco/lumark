//! 跨页面复用的图标、按钮、进度和视图组件。

use super::super::*;
use crate::i18n::tr;
use gpui_kit::Entity;
use gpui_kit::base::{Tab as BaseTab, Tabs as BaseTabs};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::select::{SearchableVec, Select, SelectState};

/// Lumark 标题栏和文件行使用的 IconPark 图标。
///
/// SVG 在编译时打包，因此界面运行时无需访问网络。
/// 每个图标使用 `currentColor`，便于主题为浅色和深色模式着色，
/// 无需维护多套图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppIcon {
    /// Add 图标。
    Add,
    /// ArrowLeft 图标。
    ArrowLeft,
    /// ArrowRight 图标。
    ArrowRight,
    /// ArrowUp 图标。
    ArrowUp,
    /// Check 图标。
    Check,
    /// Close 图标。
    Close,
    /// Compression 图标。
    Compression,
    /// Copy 图标。
    Copy,
    /// Delete 图标。
    Delete,
    /// Doc 图标。
    Doc,
    /// Down 图标。
    Down,
    /// Download 图标。
    Download,
    /// Edit 图标。
    Edit,
    /// FileLock 图标。
    FileLock,
    /// FileZip 图标。
    FileZip,
    /// Folder 图标。
    Folder,
    /// FolderOpen 图标。
    FolderOpen,
    /// FolderPlus 图标。
    FolderPlus,
    /// Grid 图标。
    Grid,
    /// Home 图标。
    Home,
    /// Key 图标。
    Key,
    /// List 图标。
    List,
    /// Lock 图标。
    Lock,
    /// MenuFold 图标。
    MenuFold,
    /// MenuUnfold 图标。
    MenuUnfold,
    /// Music 图标。
    Music,
    /// Picture 图标。
    Picture,
    /// Refresh 图标。
    Refresh,
    /// Right 图标。
    Right,
    /// Save 图标。
    Save,
    /// Search 图标。
    Search,
    /// Setting 图标。
    Setting,
    /// Shield 图标。
    Shield,
    /// Time 图标。
    Time,
    /// Tool 图标。
    Tool,
    /// Unlock 图标。
    Unlock,
    /// Video 图标。
    Video,
    /// Zip 图标。
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
        .icon(icon.element(17.))
        .tooltip(tooltip)
        .accessibility_label(tooltip)
}

/// 页面分组使用的标准卡片表面。
///
/// 页面底色保持浅灰，卡片使用浮层白面和细边框区分层次；阴影只保留
/// 极轻的一层，避免多张卡片叠在浅灰背景上显得发灰、发重。
pub(super) fn card(cx: &Context<LumarkApp>) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
        .overflow_hidden()
}

/// 顶部动作带按钮的语义色调。
///
/// 所有动作统一使用官方默认 Button 表面，只让图标按动作类别着色，
/// 避免五种彩色填充在横幅上互相争夺注意力。
#[derive(Clone, Copy, Debug)]
pub(super) enum BannerTone {
    /// 主色提示。
    Primary,
    /// 成功提示。
    Success,
    /// 警告提示。
    Warning,
    /// 错误提示。
    Danger,
    /// 中性提示。
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
    label: String,
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
        .tooltip(label.clone())
        .accessibility_label(label.clone())
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
    let from = cx.theme().primary.opacity(if cx.theme().mode.is_dark() {
        0.18
    } else {
        0.10
    });
    let to = cx.theme().background;
    div().bg(gpui_kit::linear_gradient(
        110.,
        gpui_kit::linear_color_stop(from, 0.),
        gpui_kit::linear_color_stop(to, 1.),
    ))
}

/// 渲染标题栏、动作带和对话框共用的品牌标志。
///
/// GPUI 的 SVG 元素只把图形栅格化成 alpha 蒙版，再用元素前景色填充；用
/// `Icon` 加载彩色品牌标志会得到一块纯色方块。这里改走图片解码路径
/// （`img` + `ImageFormat::Svg`），保留标志自带的渐变与配色。
///
/// 小尺寸下使用去掉底部字标的 mark 版本，完整字标只保留在图标源文件中，
/// 避免 24–34 px 的标志出现无法辨认的文字残影。
pub(super) fn brand_mark(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex_shrink_0()
        .child(gpui_kit::img(brand_mark_image()).size(px(size)))
}

/// 品牌标志的 SVG 图片源，只解码一次并在所有窗口间共享。
fn brand_mark_image() -> std::sync::Arc<gpui_kit::Image> {
    /// 图片查看器的共享缓存状态。
    static IMAGE: std::sync::OnceLock<std::sync::Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    IMAGE
        .get_or_init(|| {
            std::sync::Arc::new(gpui_kit::Image::from_bytes(
                gpui_kit::ImageFormat::Svg,
                include_bytes!("../../../assets/brand/lumark-mark.svg").to_vec(),
            ))
        })
        .clone()
}

/// 渲染标题栏、动作带和对话框共用的品牌标识。
pub(super) fn brand_lockup(cx: &gpui_kit::App, slogan: bool) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_2()
        .child(brand_mark(34.))
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
                            .child(tr(
                                "高效专业 · 值得信赖",
                                "Efficient, professional, trustworthy",
                            )),
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
                .child(tr(
                    "本地归档工作台 · 高效专业，值得信赖",
                    "Local archive workspace · efficient and trustworthy",
                )),
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

/// 渲染固定宽度的下拉框，弹出列表与触发区保持同宽。
///
/// 组件库的 `Select` 根节点会占满父级宽度，而弹出列表默认取根节点宽度；
/// 只给 `Select` 设置宽度时列表仍会按整行展开。这里先用外层容器锁定宽度，
/// 再把触发区和列表宽度设为同一数值，避免菜单横跨整个表单。
pub(super) fn choice_select(
    id: &'static str,
    state: &Entity<SelectState<SearchableVec<&'static str>>>,
    width: f32,
    accessibility_label: &'static str,
) -> impl IntoElement {
    div().w(px(width)).flex_shrink_0().child(
        Select::new(state)
            .id(id)
            .w_full()
            .menu_width(px(width))
            .accessibility_label(accessibility_label),
    )
}

/// 渲染分段选项卡轨道。
///
/// 轨道使用浅灰底、4px 内边距和 2px 间距，圆角取自主题的控件圆角，
/// 避免把矩形轨道做成胶囊形状。
pub(super) fn segmented_tab_bar(
    cx: &gpui_kit::App,
    id: &'static str,
    tabs: Vec<gpui_kit::AnyElement>,
) -> impl IntoElement {
    BaseTabs::new(id)
        .w_full()
        .h(px(40.))
        .p(px(4.))
        .gap(px(2.))
        .flex()
        .items_center()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .children(tabs)
}

/// 渲染一个带图标的分段选项卡。
///
/// 选中项使用白色表面、细边框和浅阴影，未选中项保持透明并在悬停时显示
/// 浅色底；圆角从主题控件圆角向内收 2px，保持直角矩形而不是胶囊形状。
pub(super) fn segmented_tab(
    cx: &gpui_kit::App,
    id: impl Into<gpui_kit::ElementId>,
    icon: AppIcon,
    label: &'static str,
    selected: bool,
) -> BaseTab {
    let theme = cx.theme();
    let border = if selected {
        theme.border
    } else {
        theme.transparent
    };
    let tab_radius = if theme.radius > px(2.) {
        theme.radius - px(2.)
    } else {
        px(0.)
    };
    // 深色模式没有纯白表面，改用比轨道更亮的浮起色，保持选中项层次。
    let selected_bg = if theme.mode.is_dark() {
        theme.secondary_hover
    } else {
        theme.popover
    };

    BaseTab::new(id)
        .selected(selected)
        .accessibility_label(label)
        .h_full()
        .flex_shrink_0()
        .px_4()
        .gap_2()
        .rounded(tab_radius)
        .border_1()
        .border_color(border)
        .whitespace_nowrap()
        .text_sm()
        .text_color(if selected {
            theme.foreground
        } else {
            theme.muted_foreground
        })
        .when(selected, |this| {
            this.bg(selected_bg).shadow_xs().font_medium()
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(theme.secondary_hover))
        })
        .child(icon.element(15.))
        .child(label)
}

/// 渲染一行小号辅助说明文字。
pub(super) fn hint_text(cx: &gpui_kit::App, text: impl Into<String>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
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

/// 渲染输入框上方的小号字段标题。
pub(super) fn form_label(cx: &gpui_kit::App, label: &str) -> gpui_kit::Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(label.to_string())
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
/// 返回侧栏快捷位置的当前语言名称。
pub(super) fn location_label(label: &str) -> &str {
    match label {
        "桌面" => tr("桌面", "Desktop"),
        "下载" => tr("下载", "Downloads"),
        "文档" => tr("文档", "Documents"),
        "图片" => tr("图片", "Pictures"),
        "音乐" => tr("音乐", "Music"),
        "视频" => tr("视频", "Videos"),
        _ => label,
    }
}

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
