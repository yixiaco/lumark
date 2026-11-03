//! 跨页面复用的图标、按钮、进度和视图组件。

use super::super::*;
use crate::i18n::tr;
use gpui_kit::Entity;
use gpui_kit::base::{Tab as BaseTab, Tabs as BaseTabs};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::select::{SearchableVec, Select, SelectState};

pub(super) use lumark_ui_shared::assets::{AppIcon, brand_mark};
pub(super) use lumark_ui_shared::chrome::{banner_gradient, brand_lockup, dialog_banner};
pub(super) use lumark_ui_shared::widgets::{classic_field, form_label, hint_text};

/// 主程序对话框横幅使用的统一标语。
pub(super) fn dialog_tagline() -> &'static str {
    tr(
        "本地归档工作台 · 高效专业，值得信赖",
        "Local archive workspace · efficient and trustworthy",
    )
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
            Self::Success => StatusTone::Success.color(cx),
            Self::Warning => StatusTone::Warning.color(cx),
            Self::Danger => StatusTone::Danger.color(cx),
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
        return StatusTone::Danger.color(cx);
    }
    if entry.is_dir {
        return StatusTone::Warning.color(cx);
    }
    match entry_icon(entry) {
        AppIcon::Zip => cx.theme().accent_foreground,
        AppIcon::Picture => StatusTone::Success.color(cx),
        AppIcon::Music | AppIcon::Video => cx.theme().primary,
        AppIcon::FileLock => StatusTone::Danger.color(cx),
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
