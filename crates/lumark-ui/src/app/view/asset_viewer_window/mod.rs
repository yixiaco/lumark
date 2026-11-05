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
use lumark_scene::Bounds;
use lumark_unreal::{MeshLod, Skeleton};

mod animation;
mod app;
mod async_render;
mod body;
mod frame_pump;
mod input;
mod inspector;
mod render;
mod status;
mod toolbar;

/// 查看器展示的资产类型
///
/// `Material` 变体携带完整的材质查看器数据（含贴图）而明显大于其他变体；
/// 这里按值持有，避免在 UI/预览链路中引入额外 Box 间接层。
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub(crate) enum ViewerScene {
    /// StaticMesh 几何（可能包含多个 LOD）
    Mesh {
        /// 按顺序排列的 LOD；至少一个
        lods: Vec<MeshLod>,
        /// 已解析材质（含贴图），按 section.material_index 对齐
        materials: std::sync::Arc<Vec<lumark_unreal::ResolvedMaterial>>,
    },
    /// Skeleton 参考骨骼
    Skeleton(Skeleton),
    /// SkeletalMesh rest pose 与参考骨骼叠加
    SkeletalMesh {
        /// 按顺序排列的 LOD；至少一个
        lods: Vec<MeshLod>,
        /// 参考骨骼
        skeleton: Skeleton,
        /// 每个 LOD 的蒙皮权重；与 `lods` 等长
        skins: Vec<Option<lumark_unreal::SkinWeights>>,
        /// 已解析材质（含贴图），按 section.material_index 对齐
        materials: std::sync::Arc<Vec<lumark_unreal::ResolvedMaterial>>,
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
    /// 左键轨道旋转、右键屏幕平移。
    mode: DragMode,
}

/// 画布拖拽模式。
#[derive(Clone, Copy, PartialEq, Eq)]
enum DragMode {
    /// 左键轨道旋转。
    Orbit,
    /// 右键上下左右平移。
    Pan,
}

/// 右侧检查器标签页。
#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    /// 场景概要与 LOD。
    Overview,
    /// section 列表与选中材质详情。
    Materials,
    /// 骨骼树。
    Bones,
}

/// 初始检查器标签：网格优先展示材质，骨骼/材质资产展示概要。
fn initial_inspector_tab(scene: &ViewerScene) -> InspectorTab {
    match scene {
        ViewerScene::Mesh { .. } | ViewerScene::SkeletalMesh { .. } => InspectorTab::Materials,
        ViewerScene::Skeleton(_) | ViewerScene::Material { .. } => InspectorTab::Overview,
    }
}

/// 计算每个 LOD 的局部空间包围盒，供概要面板显示尺寸（UE 单位 cm）。
fn compute_lod_bounds(scene: &ViewerScene) -> Vec<Option<([f32; 3], [f32; 3])>> {
    let lods = match scene {
        ViewerScene::Mesh { lods, .. } | ViewerScene::SkeletalMesh { lods, .. } => lods,
        _ => return Vec::new(),
    };
    lods.iter()
        .map(|lod| {
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for position in &lod.positions {
                for axis in 0..3 {
                    min[axis] = min[axis].min(position[axis] as f32);
                    max[axis] = max[axis].max(position[axis] as f32);
                }
            }
            min[0].is_finite().then_some((min, max))
        })
        .collect()
}

/// 模型查看器窗口
/// 所有 LOD 的并集包围盒；质量档/LOD 切换时相机取景保持稳定。
fn fit_bounds_for_lods(lod_bounds: &[Option<([f32; 3], [f32; 3])>]) -> Option<Bounds> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for (lod_min, lod_max) in lod_bounds.iter().flatten() {
        for axis in 0..3 {
            min[axis] = min[axis].min(lod_min[axis]);
            max[axis] = max[axis].max(lod_max[axis]);
        }
    }
    min[0].is_finite().then_some(Bounds { min, max })
}

/// 模型查看器窗口
pub(crate) struct AssetViewerWindowView {
    name: String,
    scene: Arc<ViewerScene>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    /// 右键平移累计屏幕像素：+x 向右、+y 向下。
    pan: [f32; 2],
    /// 渲染结果（直接交给 GPUI，不再写临时 PNG）
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
    drag: Option<DragState>,
    /// 被隐藏的 section（材质槽），用于材质切换预览
    hidden_sections: Vec<bool>,
    /// 默认规则会隐藏全部 section 时，是否已自动回退为全部显示。
    visibility_fallback: bool,
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
    /// 检查器当前标签页。
    inspector_tab: InspectorTab,
    /// 检查器材质的当前 section。
    selected_section: usize,
    /// FModel 属性 JSON（由预览链路生成）；无映射或解析失败时为空。
    properties_json: Option<Arc<str>>,
    /// 可拖拽的检查器分栏状态（正式窗口创建）。
    inspector_state: Option<gpui_kit::Entity<gpui_kit::base::ResizableState>>,
    /// 各 LOD 的局部空间包围盒（创建时一次性计算）。
    lod_bounds: Vec<Option<([f32; 3], [f32; 3])>>,
    /// 固定取景包围盒（全部 LOD 并集），避免质量/LOD 切换时相机跳动。
    fit_bounds: Option<Bounds>,
    /// 着色模式
    shading: ShadingMode,
    /// 是否显示地面网格
    show_grid: bool,
    /// 太阳高度角（度，0 = Snooper 默认地平线）
    sun_elevation_degrees: f32,
    /// 是否启用 Snooper 环境效果（雾/天空/地面/FXAA）
    snooper_effects: bool,
    /// 查看器输出分辨率倍率
    render_resolution: render::ViewerResolution,
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
    /// 帧泵状态：单飞行帧、输入合并与静止高质量帧
    frame_pump: frame_pump::FramePump,
    /// 骨骼搜索输入（正式窗口创建）
    bone_filter: Option<gpui_kit::Entity<InputState>>,
    /// 基础色/自发光贴图按 sRGB 采样
    srgb_textures: bool,
    /// 是否把顶点色乘到基础色（FModel 默认关闭）
    vertex_colors: bool,
    /// 线框模式
    wireframe: bool,
    /// 材质槽名称（顺序与 section.material_index 对应）
    material_names: Vec<String>,
}
impl AssetViewerWindowView {
    /// 创建窗口并渲染默认视角（无键盘焦点，供测试/内部使用）。
    #[cfg(test)]
    pub(crate) fn new(name: String, scene: ViewerScene) -> Self {
        let mut view = Self::create(name, scene, None);
        // 测试没有事件循环：同步渲染首帧，保持断言确定性。
        view.render_preview();
        view
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
        // FModel 默认隐藏无贴图/半透明 section；正式窗口打开即按该规则初始化。
        view.reset_section_visibility();
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
            view.request_preview(cx);
            cx.notify();
        });
        let bone_filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(tr("搜索骨骼…", "Search bones…").to_string())
        });
        view.timeline_slider = Some(slider);
        view.slider_subscription = Some(subscription);
        view.bone_filter = Some(bone_filter);
        view.inspector_state = Some(cx.new(|_| gpui_kit::base::ResizableState::default()));
        // 首帧也在后台渲染：窗口打开不阻塞界面线程。
        view.request_preview(cx);
        view
    }

    /// 创建查看器状态（不渲染；首帧由对话框/测试显式触发）。
    fn create(name: String, scene: ViewerScene, focus: Option<gpui_kit::FocusHandle>) -> Self {
        let inspector_tab = initial_inspector_tab(&scene);
        let lod_bounds = compute_lod_bounds(&scene);
        let fit_bounds = fit_bounds_for_lods(&lod_bounds);
        let mut view = Self {
            name,
            scene: Arc::new(scene),
            yaw: 35.0,
            pitch: 10.0,
            zoom: 1.0,
            pan: [0.0, 0.0],
            image: None,
            error: None,
            drag: None,
            hidden_sections: Vec::new(),
            visibility_fallback: false,
            skin_pose: false,
            lod_index: 0,
            animation_playing: false,
            animation_time: 0.0,
            inspector_open: true,
            inspector_tab,
            selected_section: 0,
            properties_json: None,
            inspector_state: None,
            lod_bounds,
            fit_bounds,
            shading: ShadingMode::Lit,
            // 3D 查看器默认显示地面网格（与 FModel 查看器一致）。
            show_grid: true,
            sun_elevation_degrees: 20.0,
            snooper_effects: true,
            render_resolution: render::ViewerResolution::X15,
            show_bones: false,
            last_backend: None,
            last_stats: None,
            focus,
            timeline_slider: None,
            slider_subscription: None,
            render_generation: 0,
            rendering: false,
            frame_pump: frame_pump::FramePump::new(),
            bone_filter: None,
            srgb_textures: true,
            vertex_colors: false,
            wireframe: false,
            material_names: Vec::new(),
        };
        view.hidden_sections = vec![false; view.scene_mesh().map_or(0, |mesh| mesh.sections.len())];
        view
    }
    /// 场景中已解析材质列表。
    fn materials(&self) -> &[lumark_unreal::ResolvedMaterial] {
        match self.scene.as_ref() {
            ViewerScene::Mesh { materials, .. } | ViewerScene::SkeletalMesh { materials, .. } => {
                materials
            }
            _ => &[],
        }
    }

    /// 某个材质在 FModel 默认规则下是否隐藏 section。
    ///
    /// FModel `Section.SetupMaterial`：材质为空（没有任何贴图）或半透明时
    /// 默认不显示。Lumark 仍允许在面板中手动显示。
    fn material_auto_hidden(&self, material_index: usize) -> bool {
        let Some(material) = self.materials().get(material_index) else {
            return true;
        };
        let Some(appearance) = material.appearance.as_ref() else {
            return true;
        };
        appearance.textures.is_empty() || appearance.translucent
    }

    /// 按 FModel 默认规则计算 section 可见性（true=隐藏）。
    fn default_hidden_sections(&self) -> Vec<bool> {
        let Some(mesh) = self.scene_mesh() else {
            return Vec::new();
        };
        mesh.sections
            .iter()
            .map(|section| {
                let material_index =
                    usize::try_from(section.material_index.max(0)).unwrap_or(usize::MAX);
                self.material_auto_hidden(material_index)
            })
            .collect()
    }

    /// 重置 section 可见性到 FModel 默认规则；切换 LOD 时调用。
    pub(super) fn reset_section_visibility(&mut self) {
        let mut hidden = self.default_hidden_sections();
        // FModel 默认会隐藏无贴图/半透明 section；如果这会隐藏整个模型，
        // 查看器不能只剩背景，回退为全部显示并提示。
        self.visibility_fallback = !hidden.is_empty() && hidden.iter().all(|hidden| *hidden);
        if self.visibility_fallback {
            hidden.fill(false);
        }
        self.hidden_sections = hidden;
    }

    /// 当前 section 对应的材质。
    pub(super) fn section_material(
        &self,
        section_index: usize,
    ) -> Option<&lumark_unreal::ResolvedMaterial> {
        let mesh = self.scene_mesh()?;
        let section = mesh.sections.get(section_index)?;
        let material_index = usize::try_from(section.material_index.max(0)).ok()?;
        self.materials().get(material_index)
    }
    /// 附带材质槽名称（由详情解析结果提供）。
    pub(crate) fn with_material_names(mut self, names: Vec<String>) -> Self {
        self.material_names = names;
        self
    }

    /// 附带资产的 JSON 属性文本（由预览链路生成）。
    pub(crate) fn with_properties_json(mut self, json: Option<Arc<str>>) -> Self {
        self.properties_json = json;
        self
    }

    /// 当前查看器持有的 JSON 属性文本。
    pub(super) fn properties_json_text(&self) -> Option<Arc<str>> {
        self.properties_json.clone()
    }

    fn kind_label(&self) -> &'static str {
        match self.scene.as_ref() {
            ViewerScene::Mesh { .. } => tr("模型查看器", "Model viewer"),
            ViewerScene::Skeleton(_) => tr("骨骼查看器", "Skeleton viewer"),
            ViewerScene::SkeletalMesh { .. } => tr("骨骼网格查看器", "Skeletal mesh viewer"),
            ViewerScene::Material { .. } => tr("材质查看器", "Material viewer"),
        }
    }
    fn scene_mesh(&self) -> Option<&MeshLod> {
        match self.scene.as_ref() {
            ViewerScene::Mesh { lods, .. } => lods.get(self.lod_index).or_else(|| lods.first()),
            ViewerScene::Skeleton(_) => None,
            ViewerScene::SkeletalMesh { lods, .. } => {
                lods.get(self.lod_index).or_else(|| lods.first())
            }
            ViewerScene::Material { .. } => None,
        }
    }

    /// 返回当前场景?LOD 列表
    fn static_lods(&self) -> Option<&[MeshLod]> {
        match self.scene.as_ref() {
            ViewerScene::Mesh { lods, .. } | ViewerScene::SkeletalMesh { lods, .. } => Some(lods),
            _ => None,
        }
    }

    /// 当前场景是否包含蒙皮权重（用于显示蒙皮演示按钮）。
    fn has_skin(&self) -> bool {
        matches!(
            self.scene.as_ref(),
            ViewerScene::SkeletalMesh { skins, .. } if skins.iter().any(Option::is_some)
        )
    }

    /// 是否具备可播放动画
    fn has_animation(&self) -> bool {
        let has_animation = matches!(
            self.scene.as_ref(),
            ViewerScene::SkeletalMesh {
                animation: Some(_),
                ..
            }
        );
        has_animation && self.has_skin()
    }

    /// 当前动画时长
    fn animation_length(&self) -> Option<f32> {
        match self.scene.as_ref() {
            ViewerScene::SkeletalMesh {
                animation: Some(animation),
                ..
            } => Some(animation.length),
            _ => None,
        }
    }

    /// 当前动画的时长和总帧数
    fn animation_info(&self) -> Option<(f32, u32)> {
        match self.scene.as_ref() {
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
            .child(
                div()
                    .debug_selector(|| "model-viewer-titlebar".into())
                    .w_full()
                    .flex_shrink_0()
                    .child(self.render_title_bar(cx)),
            )
            .child(
                div()
                    .debug_selector(|| "model-viewer-toolbar".into())
                    .w_full()
                    .flex_shrink_0()
                    .child(self.render_toolbar(cx)),
            )
            .child(self.render_content(cx))
            .child(self.render_status_bar(cx))
    }
}
#[cfg(test)]
mod tests;
