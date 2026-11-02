use super::image_ops::viewer_display_name;
use super::*;

impl LumarkApp {
    /// 按文件类型选择文本查看器或图片查看器。
    pub(crate) fn open_builtin_viewer(
        &mut self,
        target: PathBuf,
        display: String,
        can_save: bool,
        archive_target: Option<super::super::text_viewer_window::ViewerArchiveTarget>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        if is_image_name(&display) {
            self.open_image_viewer(target, display, parent, cx);
        } else {
            self.open_text_viewer(target, display, can_save, archive_target, parent, cx);
        }
    }

    /// 在新的只读窗口中打开图片。
    pub(crate) fn open_image_viewer(
        &mut self,
        target: PathBuf,
        display: String,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let size = std::fs::metadata(&target)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        // 生成型预览（纹理/材质球等）的临时 PNG 文件名没有意义，优先使用
        // 调用方传入的资产显示名。
        let name = viewer_display_name(&display, &target);
        let window_size = gpui_kit::size(px(1000.), px(760.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - window_size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - window_size.height.as_f32()) / 2.0),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size: window_size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("图片查看器 - Lumark".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(480.), px(360.))),
            ..TitleBar::window_options()
        };
        let image_path = target.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                ImageViewerWindowView::new(image_path.clone(), name.clone(), size, window, cx)
            })
        });
        match result {
            Ok(_) => self.status = format!("已在内置图片查看器中打开 {display}"),
            Err(error) => self.status = format!("无法打开图片查看器：{error}"),
        }
    }
}
