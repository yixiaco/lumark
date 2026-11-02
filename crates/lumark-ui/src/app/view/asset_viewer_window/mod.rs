//! 内置模型/骨骼查看器窗口。
//!
//! 使用 CPU 光栅器在拖拽时重绘；左键拖拽旋转，工具栏缩放/重置。
//! 该窗口只读，不修改原包或游戏目录?
use std::sync::Arc;

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::RenderImage;
use gpui_kit::component::TitleBar;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use lumark_render::ShadingMode;
use lumark_unreal::{MeshLod, Skeleton};

mod animation;
mod app;
mod async_render;
mod body;
mod input;
mod inspector;
mod render;
mod status;
mod toolbar;

/// 查看器展示的资产类型
#[derive(Clone)]
pub(crate) enum ViewerScene {
    /// StaticMesh 几何（可能包含多?LOD）
    Mesh {
        /// 按顺序排列的 LOD；至少一个
        lods: Vec<MeshLod>,
    },
    /// Skeleton 参考骨骼
    Skeleton(Skeleton),
    /// SkeletalMesh rest pose 与参考骨骼叠加
    SkeletalMesh {
        /// 按顺序排列的 LOD；至少一个
        lods: Vec<MeshLod>,
        /// 参考骨骼
        skeleton: Skeleton,
        /// 每个 LOD 的蒙皮权重；?`lods` 等长
        skins: Vec<Option<lumark_unreal::SkinWeights>>,
        /// 可播放的压缩动画
        animation: Option<Box<lumark_unreal::AnimSequenceData>>,
    },
    /// 交互式近似 PBR 材质球
    Material {
        /// 材质查看器数据
        viewer: lumark_unreal::MaterialViewerData,
    },
}
/// 当前拖动状态
struct DragState {
    last: (f32, f32),
}
/// 模型查看器窗口
pub(crate) struct AssetViewerWindowView {
    name: String,
    scene: ViewerScene,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    /// 渲染结果（直接交给 GPUI，不再写临时 PNG）
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
    drag: Option<DragState>,
    /// 被隐藏的 section（材质槽），用于材质切换预览
    hidden_sections: Vec<bool>,
    /// 是否显示蒙皮演示姿态
    skin_pose: bool,
    /// 当前 StaticMesh LOD
    lod_index: usize,
    /// 动画是否正在播放
    animation_playing: bool,
    /// 当前动画时间（秒）
    animation_time: f32,
    /// 是否显示右侧检查器
    inspector_open: bool,
    /// 着色模式
    shading: ShadingMode,
    /// 是否显示地面网格
    show_grid: bool,
    /// 是否显示骨骼线
    show_bones: bool,
    /// 最近一次渲染使用的后端标签
    last_backend: Option<String>,
    /// 最近一次渲染的场景统计
    last_stats: Option<lumark_render::SceneStats>,
    /// 键盘焦点（仅正式窗口创建；测试/内部构造可为空）
    focus: Option<gpui_kit::FocusHandle>,
    /// 动画时间轴 Slider 状态
    timeline_slider: Option<gpui_kit::Entity<SliderState>>,
    /// Slider 事件订阅
    slider_subscription: Option<gpui_kit::Subscription>,
    /// 异步渲染代号；旧结果到达时丢弃
    render_generation: u64,
    /// 是否有后台渲染进行中
    rendering: bool,
    /// 骨骼搜索输入（正式窗口创建）
    bone_filter: Option<gpui_kit::Entity<InputState>>,
    /// 基础色/自发光贴图按 sRGB 采样
    srgb_textures: bool,
    /// 线框模式
    wireframe: bool,
    /// 材质槽名称（顺序与 section.material_index 对应）
    material_names: Vec<String>,
}
impl AssetViewerWindowView {
    /// 创建窗口并渲染默认视角（无键盘焦点，供测试/内部使用）。
    #[cfg(test)]
    pub(crate) fn new(name: String, scene: ViewerScene) -> Self {
        Self::create(name, scene, None)
    }

    /// 创建带键盘焦点的正式查看器窗口。
    pub(crate) fn new_with_focus(
        name: String,
        scene: ViewerScene,
        focus: gpui_kit::FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::create(name, scene, Some(focus));
        let slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.001));
        let subscription = cx.subscribe(&slider, |view, _slider, event: &SliderEvent, cx| {
            let SliderEvent::Change(value) = event else {
                return;
            };
            let Some(length) = view.animation_length() else {
                return;
            };
            view.animation_playing = false;
            view.animation_time = value.end().clamp(0.0, 1.0) * length;
            view.render_preview();
            cx.notify();
        });
        let bone_filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(tr("搜索骨骼…", "Search bones…").to_string())
        });
        view.timeline_slider = Some(slider);
        view.slider_subscription = Some(subscription);
        view.bone_filter = Some(bone_filter);
        view
    }

    fn create(name: String, scene: ViewerScene, focus: Option<gpui_kit::FocusHandle>) -> Self {
        let mut view = Self {
            name,
            scene,
            yaw: 35.0,
            pitch: 25.0,
            zoom: 1.0,
            image: None,
            error: None,
            drag: None,
            hidden_sections: Vec::new(),
            skin_pose: false,
            lod_index: 0,
            animation_playing: false,
            animation_time: 0.0,
            inspector_open: true,
            shading: ShadingMode::Lit,
            show_grid: false,
            show_bones: false,
            last_backend: None,
            last_stats: None,
            focus,
            timeline_slider: None,
            slider_subscription: None,
            render_generation: 0,
            rendering: false,
            bone_filter: None,
            srgb_textures: true,
            wireframe: false,
            material_names: Vec::new(),
        };
        view.hidden_sections = vec![false; view.scene_mesh().map_or(0, |mesh| mesh.sections.len())];
        view.render_preview();
        view
    }
    /// 附带材质槽名称（由详情解析结果提供）。
    pub(crate) fn with_material_names(mut self, names: Vec<String>) -> Self {
        self.material_names = names;
        self
    }

    fn kind_label(&self) -> &'static str {
        match self.scene {
            ViewerScene::Mesh { .. } => tr("模型查看器", "Model viewer"),
            ViewerScene::Skeleton(_) => tr("骨骼查看器", "Skeleton viewer"),
            ViewerScene::SkeletalMesh { .. } => tr("骨骼网格查看器", "Skeletal mesh viewer"),
            ViewerScene::Material { .. } => tr("材质查看器", "Material viewer"),
        }
    }
    fn scene_mesh(&self) -> Option<&MeshLod> {
        match &self.scene {
            ViewerScene::Mesh { lods } => lods.get(self.lod_index).or_else(|| lods.first()),
            ViewerScene::Skeleton(_) => None,
            ViewerScene::SkeletalMesh { lods, .. } => {
                lods.get(self.lod_index).or_else(|| lods.first())
            }
            ViewerScene::Material { .. } => None,
        }
    }

    /// 返回当前场景?LOD 列表
    fn static_lods(&self) -> Option<&[MeshLod]> {
        match &self.scene {
            ViewerScene::Mesh { lods } | ViewerScene::SkeletalMesh { lods, .. } => Some(lods),
            _ => None,
        }
    }

    /// 当前场景是否包含蒙皮权重（用于显示蒙皮演示按钮）。
    fn has_skin(&self) -> bool {
        matches!(
            &self.scene,
            ViewerScene::SkeletalMesh { skins, .. } if skins.iter().any(Option::is_some)
        )
    }

    /// 是否具备可播放动画
    fn has_animation(&self) -> bool {
        let has_animation = matches!(
            self.scene,
            ViewerScene::SkeletalMesh {
                animation: Some(_),
                ..
            }
        );
        has_animation && self.has_skin()
    }

    /// 当前动画时长
    fn animation_length(&self) -> Option<f32> {
        match &self.scene {
            ViewerScene::SkeletalMesh {
                animation: Some(animation),
                ..
            } => Some(animation.length),
            _ => None,
        }
    }

    /// 当前动画的时长和总帧数
    fn animation_info(&self) -> Option<(f32, u32)> {
        match &self.scene {
            ViewerScene::SkeletalMesh {
                animation: Some(animation),
                ..
            } => Some((animation.length, animation.frames.max(1))),
            _ => None,
        }
    }
}

impl Render for AssetViewerWindowView {
    /// 组合标题、工具栏、画布和状态栏
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_timeline_slider(window, cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .when_some(self.focus.as_ref(), |this, focus| {
                this.track_focus(focus).on_key_down(cx.listener(
                    |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        let modifiers = &event.keystroke.modifiers;
                        if modifiers.control || modifiers.alt {
                            return;
                        }
                        this.handle_shortcut(&event.keystroke.key, cx);
                    },
                ))
            })
            .child(self.render_title_bar(cx))
            .child(self.render_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.render_body(cx))
                    .children(self.inspector_open.then(|| self.render_inspector(cx))),
            )
            .child(self.render_status_bar(cx))
    }
}
#[cfg(test)]
mod tests;
