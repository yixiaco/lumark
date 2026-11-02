use super::*;

impl LumarkApp {
    /// 根据详情面板缓存选择查看器场景
    pub(crate) fn details_viewer_scene(&self) -> Option<ViewerScene> {
        if let Some(viewer) = self.details_material.clone() {
            return Some(ViewerScene::Material { viewer });
        }
        match (self.details_mesh.clone(), self.details_skeleton.clone()) {
            (Some(mesh), Some(skeleton)) => Some(ViewerScene::SkeletalMesh {
                lods: if self.details_lods.is_empty() {
                    vec![mesh]
                } else {
                    self.details_lods.clone()
                },
                skeleton,
                skins: self.details_skins.clone(),
                animation: self.details_animation.clone().map(Box::new),
            }),
            (Some(mesh), None) => Some(ViewerScene::Mesh {
                lods: if self.details_lods.is_empty() {
                    vec![mesh]
                } else {
                    self.details_lods.clone()
                },
            }),
            (None, Some(skeleton)) => Some(ViewerScene::Skeleton(skeleton)),
            (None, None) => None,
        }
    }

    /// 在新的只读窗口中打开模型或骨骼查看器
    pub(crate) fn open_asset_viewer(
        &mut self,
        scene: ViewerScene,
        name: String,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let title = match scene {
            ViewerScene::Mesh { .. } => "模型查看器 - Lumark",
            ViewerScene::Skeleton(_) => "骨骼查看器 - Lumark",
            ViewerScene::SkeletalMesh { .. } => "骨骼网格查看器 - Lumark",
            ViewerScene::Material { .. } => "材质查看器 - Lumark",
        };
        let display_kind = match scene {
            ViewerScene::Mesh { .. } => "模型",
            ViewerScene::Skeleton(_) => "骨骼",
            ViewerScene::SkeletalMesh { .. } => "骨骼网格",
            ViewerScene::Material { .. } => "材质",
        };
        let material_names = self.details_material_names.clone();
        let window_size = gpui_kit::size(px(960.), px(720.));
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
                title: Some(title.into()),
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
        let display = name.clone();
        let scene_for_window = scene.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            let focus = cx.focus_handle();
            let view = cx.new(|_cx| {
                AssetViewerWindowView::new_with_focus(
                    display.clone(),
                    scene_for_window.clone(),
                    focus.clone(),
                    window,
                    _cx,
                )
                .with_material_names(material_names.clone())
            });
            window.focus(&focus, cx);
            view
        });
        match result {
            Ok(_) => self.status = format!("已在内置{display_kind}查看器中打开 {name}"),
            Err(error) => self.status = format!("无法打开{display_kind}查看器：{error}"),
        }
    }
}
