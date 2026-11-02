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
use lumark_unreal::{MeshLod, MeshSection, Skeleton};

mod animation;
mod app;
mod body;
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
}
impl AssetViewerWindowView {
    /// 创建窗口并渲染默认视角
    pub(crate) fn new(name: String, scene: ViewerScene) -> Self {
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
        };
        view.hidden_sections = vec![false; view.scene_mesh().map_or(0, |mesh| mesh.sections.len())];
        view.render_preview();
        view
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
#[cfg(test)]
mod tests;
