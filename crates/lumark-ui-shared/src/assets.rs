//! 主程序与自解压运行时共用的 IconPark 图标和品牌标志。

use gpui_kit::{IntoElement, ParentElement, Styled, div, px};

/// Lumark 标题栏和文件行使用的 IconPark 图标。
///
/// SVG 在编译时打包，因此界面运行时无需访问网络。
/// 每个图标使用 `currentColor`，便于主题为浅色和深色模式着色，
/// 无需维护多套图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppIcon {
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
    /// Game 图标。
    Game,
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
    /// ZoomIn 图标。
    ZoomIn,
    /// ZoomOut 图标。
    ZoomOut,
}

impl AppIcon {
    /// 返回一个 IconPark 图标的嵌入 SVG 字节。
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Add => include_bytes!("../assets/icons/iconpark/add.svg"),
            Self::ArrowLeft => include_bytes!("../assets/icons/iconpark/arrow-left.svg"),
            Self::ArrowRight => include_bytes!("../assets/icons/iconpark/arrow-right.svg"),
            Self::ArrowUp => include_bytes!("../assets/icons/iconpark/arrow-up.svg"),
            Self::Check => include_bytes!("../assets/icons/iconpark/check.svg"),
            Self::Close => include_bytes!("../assets/icons/iconpark/close.svg"),
            Self::Compression => include_bytes!("../assets/icons/iconpark/compression.svg"),
            Self::Copy => include_bytes!("../assets/icons/iconpark/copy.svg"),
            Self::Delete => include_bytes!("../assets/icons/iconpark/delete.svg"),
            Self::Doc => include_bytes!("../assets/icons/iconpark/doc-detail.svg"),
            Self::Down => include_bytes!("../assets/icons/iconpark/down.svg"),
            Self::Download => include_bytes!("../assets/icons/iconpark/download.svg"),
            Self::Edit => include_bytes!("../assets/icons/iconpark/edit.svg"),
            Self::FileLock => include_bytes!("../assets/icons/iconpark/file-lock.svg"),
            Self::FileZip => include_bytes!("../assets/icons/iconpark/file-zip.svg"),
            Self::Folder => include_bytes!("../assets/icons/iconpark/folder.svg"),
            Self::FolderOpen => include_bytes!("../assets/icons/iconpark/folder-open.svg"),
            Self::FolderPlus => include_bytes!("../assets/icons/iconpark/folder-plus.svg"),
            Self::Game => include_bytes!("../assets/icons/iconpark/game.svg"),
            Self::Grid => include_bytes!("../assets/icons/iconpark/grid-four.svg"),
            Self::Home => include_bytes!("../assets/icons/iconpark/home.svg"),
            Self::Key => include_bytes!("../assets/icons/iconpark/key.svg"),
            Self::List => include_bytes!("../assets/icons/iconpark/list.svg"),
            Self::Lock => include_bytes!("../assets/icons/iconpark/lock.svg"),
            Self::MenuFold => include_bytes!("../assets/icons/iconpark/menu-fold.svg"),
            Self::MenuUnfold => include_bytes!("../assets/icons/iconpark/menu-unfold.svg"),
            Self::Music => include_bytes!("../assets/icons/iconpark/music.svg"),
            Self::Picture => include_bytes!("../assets/icons/iconpark/picture.svg"),
            Self::Refresh => include_bytes!("../assets/icons/iconpark/refresh.svg"),
            Self::Right => include_bytes!("../assets/icons/iconpark/right.svg"),
            Self::Save => include_bytes!("../assets/icons/iconpark/save.svg"),
            Self::Search => include_bytes!("../assets/icons/iconpark/search.svg"),
            Self::Setting => include_bytes!("../assets/icons/iconpark/setting.svg"),
            Self::Shield => include_bytes!("../assets/icons/iconpark/shield.svg"),
            Self::Time => include_bytes!("../assets/icons/iconpark/time.svg"),
            Self::Tool => include_bytes!("../assets/icons/iconpark/tool.svg"),
            Self::Unlock => include_bytes!("../assets/icons/iconpark/unlock.svg"),
            Self::Video => include_bytes!("../assets/icons/iconpark/video.svg"),
            Self::Zip => include_bytes!("../assets/icons/iconpark/zip.svg"),
            Self::ZoomIn => include_bytes!("../assets/icons/iconpark/zoom-in.svg"),
            Self::ZoomOut => include_bytes!("../assets/icons/iconpark/zoom-out.svg"),
        }
    }

    /// 构建继承周围文字颜色的可缩放图标。
    pub fn element(self, size: f32) -> gpui_kit::component::Icon {
        gpui_kit::component::Icon::default()
            .data(self.bytes())
            .size(px(size))
    }
}

/// 渲染标题栏、动作带和对话框共用的品牌标志。
///
/// GPUI 的 SVG 元素只把图形栅格化成 alpha 蒙版，再用元素前景色填充；用
/// `Icon` 加载彩色品牌标志会得到一块纯色方块。这里改走图片解码路径
/// （`img` + `ImageFormat::Svg`），保留标志自带的渐变与配色。
///
/// 小尺寸下使用去掉底部字标的 mark 版本，完整字标只保留在图标源文件中，
/// 避免 24–34 px 的标志出现无法辨认的文字残影。
pub fn brand_mark(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex_shrink_0()
        .child(gpui_kit::img(brand_mark_image()).size(px(size)))
}

/// 品牌标志的 SVG 图片源，只解码一次并在所有窗口间共享。
fn brand_mark_image() -> std::sync::Arc<gpui_kit::Image> {
    /// 品牌标志的共享缓存。
    static IMAGE: std::sync::OnceLock<std::sync::Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    IMAGE
        .get_or_init(|| {
            std::sync::Arc::new(gpui_kit::Image::from_bytes(
                gpui_kit::ImageFormat::Svg,
                include_bytes!("../assets/brand/lumark-mark.svg").to_vec(),
            ))
        })
        .clone()
}
