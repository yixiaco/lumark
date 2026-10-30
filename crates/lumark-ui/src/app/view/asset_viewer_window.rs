//! 内置模型/骨骼查看器窗口。
//!
//! 使用 CPU 光栅器在拖拽时重绘；左键拖拽旋转，工具栏缩放/重置。
//! 该窗口只读，不修改原包或游戏目录。

use std::path::PathBuf;

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::component::TitleBar;
use lumark_unreal::{MeshLod, Skeleton};

/// 查看器展示的资产类型。
#[derive(Clone)]
pub(crate) enum ViewerScene {
    /// StaticMesh 几何。
    Mesh(MeshLod),
    /// Skeleton 参考骨骼。
    Skeleton(Skeleton),
}

/// 当前拖动状态。
struct DragState {
    last: (f32, f32),
}

/// 模型查看器窗口。
pub(crate) struct AssetViewerWindowView {
    name: String,
    scene: ViewerScene,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    image_path: PathBuf,
    error: Option<String>,
    drag: Option<DragState>,
}

impl AssetViewerWindowView {
    /// 创建窗口并渲染默认视角。
    pub(crate) fn new(name: String, scene: ViewerScene) -> Self {
        let image_path =
            std::env::temp_dir().join(format!("lumark-model-{}.png", uuid::Uuid::new_v4()));
        let mut view = Self {
            name,
            scene,
            yaw: 35.0,
            pitch: 25.0,
            zoom: 1.0,
            image_path,
            error: None,
            drag: None,
        };
        view.render_preview();
        view
    }

    /// 按当前相机参数重新渲染并替换图片文件。
    fn render_preview(&mut self) {
        let rgba = match &self.scene {
            ViewerScene::Mesh(mesh) => lumark_unreal::render_static_mesh_orbit(
                mesh, 900, 600, self.yaw, self.pitch, self.zoom,
            ),
            ViewerScene::Skeleton(skeleton) => lumark_unreal::render_skeleton_orbit(
                skeleton, 900, 600, self.yaw, self.pitch, self.zoom,
            ),
        };
        let Some(image) = image::RgbaImage::from_raw(900, 600, rgba) else {
            self.error = Some(tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string());
            return;
        };
        let next = std::env::temp_dir().join(format!("lumark-model-{}.png", uuid::Uuid::new_v4()));
        if let Err(error) = image.save(&next) {
            self.error = Some(error.to_string());
            return;
        }
        if self.image_path != next {
            let _ = std::fs::remove_file(&self.image_path);
        }
        self.image_path = next;
        self.error = None;
    }

    /// 查看器类型名称。
    fn kind_label(&self) -> &'static str {
        match self.scene {
            ViewerScene::Mesh(_) => tr("模型查看器", "Model viewer"),
            ViewerScene::Skeleton(_) => tr("骨骼查看器", "Skeleton viewer"),
        }
    }

    /// 渲染标题栏。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(38.))
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Game.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_sm()
                            .font_semibold()
                            .child(format!("{} · {}", self.name, self.kind_label())),
                    ),
            )
    }

    /// 渲染工具栏。
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                button("model-reset")
                    .ghost()
                    .icon(AppIcon::Refresh.element(13.))
                    .label(tr("重置视角", "Reset view"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.yaw = 35.0;
                        this.pitch = 25.0;
                        this.zoom = 1.0;
                        this.render_preview();
                        cx.notify();
                    })),
            )
            .child(
                button("model-zoom-out")
                    .ghost()
                    .icon(AppIcon::Close.element(12.))
                    .tooltip(tr("缩小", "Zoom out"))
                    .accessibility_label(tr("缩小", "Zoom out"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = (this.zoom / 1.2).clamp(0.2, 6.0);
                        this.render_preview();
                        cx.notify();
                    })),
            )
            .child(
                button("model-zoom-in")
                    .ghost()
                    .icon(AppIcon::Add.element(12.))
                    .tooltip(tr("放大", "Zoom in"))
                    .accessibility_label(tr("放大", "Zoom in"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = (this.zoom * 1.2).clamp(0.2, 6.0);
                        this.render_preview();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{}%", (this_zoom_percent(self.zoom)))),
            )
    }

    /// 渲染画布。
    fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if let Some(error) = &self.error {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .text_sm()
                .text_color(StatusTone::Danger.color(cx))
                .child(error.clone())
                .into_any_element();
        }
        div()
            .id("model-viewer-canvas")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(
                gpui_kit::img(self.image_path.clone())
                    .size_full()
                    .object_fit(gpui_kit::ObjectFit::Contain),
            )
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, _| {
                    this.drag = Some(DragState {
                        last: (event.position.x.as_f32(), event.position.y.as_f32()),
                    });
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                        this.drag = None;
                        return;
                    }
                    let Some(drag) = this.drag.as_mut() else {
                        return;
                    };
                    let position = (event.position.x.as_f32(), event.position.y.as_f32());
                    let dx = position.0 - drag.last.0;
                    let dy = position.1 - drag.last.1;
                    drag.last = position;
                    this.yaw = (this.yaw + dx * 0.4).rem_euclid(360.0);
                    this.pitch = (this.pitch + dy * 0.4).clamp(-89.0, 89.0);
                    this.render_preview();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// 渲染底部状态栏。
    fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(26.))
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(match &self.scene {
                ViewerScene::Mesh(mesh) => format!(
                    "{} {} · {} {}",
                    mesh.positions.len(),
                    tr("顶点", "vertices"),
                    mesh.triangle_count(),
                    tr("三角形", "triangles")
                ),
                ViewerScene::Skeleton(skeleton) => {
                    format!("{} {}", skeleton.bones.len(), tr("根骨骼", "bones"))
                }
            })
            .child(tr("左键拖拽旋转", "Drag to orbit").to_string())
            .child(format!("{}%", this_zoom_percent(self.zoom)))
    }
}

impl Render for AssetViewerWindowView {
    /// 组合标题、工具栏、画布和状态栏。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(self.render_toolbar(cx))
            .child(self.render_body(cx))
            .child(self.render_status_bar(cx))
    }
}

impl Drop for AssetViewerWindowView {
    /// 删除窗口独占的临时预览图片。
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.image_path);
    }
}

/// 缩放百分比。
fn this_zoom_percent(zoom: f32) -> u32 {
    (zoom * 100.0).round() as u32
}

impl LumarkApp {
    /// 在新的只读窗口中打开模型或骨骼查看器。
    pub(crate) fn open_asset_viewer(
        &mut self,
        scene: ViewerScene,
        name: String,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let title = match scene {
            ViewerScene::Mesh(_) => "模型查看器 - Lumark",
            ViewerScene::Skeleton(_) => "骨骼查看器 - Lumark",
        };
        let display_kind = match scene {
            ViewerScene::Mesh(_) => "模型",
            ViewerScene::Skeleton(_) => "骨骼",
        };
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
        let result = gpui_kit::open_window(options, cx, move |_window, cx| {
            cx.new(|_cx| AssetViewerWindowView::new(display.clone(), scene_for_window.clone()))
        });
        match result {
            Ok(_) => self.status = format!("已在内置{display_kind}查看器中打开 {name}"),
            Err(error) => self.status = format!("无法打开{display_kind}查看器：{error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_unreal::MeshSection;

    /// 窗口初始化时生成可读取的预览图片。
    #[test]
    fn viewer_renders_initial_preview_file() {
        let mesh = MeshLod {
            positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
            sections: vec![MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 1,
            }],
        };
        let view = AssetViewerWindowView::new("Mesh".to_string(), ViewerScene::Mesh(mesh));
        assert!(view.error.is_none());
        assert!(view.image_path.is_file());
        assert!(
            std::fs::metadata(&view.image_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                > 0
        );
    }

    /// 骨骼查看器同样生成预览图片。
    #[test]
    fn skeleton_viewer_renders_preview() {
        use lumark_unreal::Bone;

        let skeleton = Skeleton {
            name: "SKEL_Test".to_string(),
            bones: vec![
                Bone {
                    name: "Root".to_string(),
                    parent_index: -1,
                    translation: [0.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                Bone {
                    name: "Joint".to_string(),
                    parent_index: 0,
                    translation: [0.0, 0.0, 5.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ],
        };
        let view = AssetViewerWindowView::new("SKEL".to_string(), ViewerScene::Skeleton(skeleton));
        assert!(view.error.is_none());
        assert!(view.image_path.is_file());
    }
}
