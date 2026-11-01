//! 内置模型/骨骼查看器窗口。
//!
//! 使用 CPU 光栅器在拖拽时重绘；左键拖拽旋转，工具栏缩放/重置。
//! 该窗口只读，不修改原包或游戏目录?
use std::path::PathBuf;

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::component::TitleBar;
use lumark_unreal::{MeshLod, MeshSection, Skeleton};

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

/// 动画时间轴显示状态。
///
/// 把进度、帧号和帧步进计算集中到纯函数，工具栏渲染与逐帧按钮共用，
/// 也便于在无窗口环境下测试。
#[derive(Debug, Clone, Copy)]
struct AnimationTimeline {
    length: f32,
    total_frames: u32,
    time: f32,
}

impl AnimationTimeline {
    /// 创建时间轴状态；时长为 0 时进度固定为 0。
    fn new(length: f32, total_frames: u32, time: f32) -> Self {
        let total_frames = total_frames.max(1);
        let time = if length > 0.0 {
            time.clamp(0.0, length)
        } else {
            0.0
        };
        Self {
            length,
            total_frames,
            time,
        }
    }

    /// 当前播放进度（0–1）。
    fn ratio(&self) -> f32 {
        if self.length > 0.0 {
            (self.time / self.length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// 当前帧号（1 起）。
    fn frame(&self) -> u32 {
        let last_frame = self.total_frames.saturating_sub(1) as f32;
        let frame = (self.ratio() * last_frame).round() as u32 + 1;
        frame.min(self.total_frames)
    }

    /// 帧号/时间标签。
    fn label(&self) -> String {
        format!(
            "{}/{} · {:.2}s / {:.2}s",
            self.frame(),
            self.total_frames,
            self.time,
            self.length
        )
    }

    /// 按帧步进后的时间（按总帧数换算帧间隔，并夹在时长范围内）。
    fn stepped(&self, frames: i32) -> f32 {
        if self.length <= 0.0 {
            return 0.0;
        }
        let frame_time = self.length / self.total_frames.saturating_sub(1).max(1) as f32;
        (self.time + frames as f32 * frame_time).clamp(0.0, self.length)
    }
}

/// 模型查看器窗口
pub(crate) struct AssetViewerWindowView {
    name: String,
    scene: ViewerScene,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    image_path: PathBuf,
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

    /// 按当前相机参数重新渲染并替换图片文件
    fn render_preview(&mut self) {
        let visible_mesh = self
            .scene_mesh()
            .map(|mesh| filter_sections(mesh, &self.hidden_sections));
        let rgba = match &self.scene {
            ViewerScene::Mesh { lods } => {
                let fallback = lods.first();
                let visible = visible_mesh
                    .as_ref()
                    .or_else(|| lods.get(self.lod_index))
                    .or(fallback);
                if let Some(mesh) = visible {
                    lumark_unreal::render_static_mesh_orbit(
                        mesh, 900, 600, self.yaw, self.pitch, self.zoom,
                    )
                } else {
                    vec![0u8; 900 * 600 * 4]
                }
            }
            ViewerScene::Skeleton(skeleton) => lumark_unreal::render_skeleton_orbit(
                skeleton, 900, 600, self.yaw, self.pitch, self.zoom,
            ),
            ViewerScene::Material { viewer } => {
                let ball = lumark_unreal::render_material_ball_orbit(
                    &viewer.appearance,
                    viewer.base_color.as_ref().map(|texture| {
                        lumark_unreal::MaterialPreviewTexture::new(
                            &texture.rgba,
                            texture.width,
                            texture.height,
                        )
                    }),
                    viewer.normal.as_ref().map(|texture| {
                        lumark_unreal::MaterialPreviewTexture::new(
                            &texture.rgba,
                            texture.width,
                            texture.height,
                        )
                    }),
                    viewer.metallic_roughness.as_ref().map(|texture| {
                        lumark_unreal::MaterialPreviewTexture::new(
                            &texture.rgba,
                            texture.width,
                            texture.height,
                        )
                    }),
                    self.yaw,
                    self.pitch,
                    self.zoom,
                    600,
                );
                compose_square_canvas(&ball, 600, 900, 600)
            }
            ViewerScene::SkeletalMesh {
                lods,
                skeleton,
                skins,
                animation,
            } => {
                let fallback = lods.first();
                let visible = visible_mesh
                    .as_ref()
                    .or_else(|| lods.get(self.lod_index))
                    .or(fallback)
                    .expect("SkeletalMesh 场景至少包含一个 LOD");
                let skin = skins
                    .get(self.lod_index)
                    .and_then(Option::as_ref)
                    .or_else(|| skins.first().and_then(Option::as_ref));
                let animated = if (self.animation_playing || self.animation_time > 0.0)
                    && let (Some(animation), Some(skin)) = (animation, skin)
                {
                    let ratio = if animation.length > 0.0 {
                        (self.animation_time / animation.length).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let poses =
                        lumark_unreal::skinning::animation_poses(skeleton, animation, ratio);
                    Some(lumark_unreal::skinning::skin_mesh_pose(
                        visible, skeleton, skin, &poses,
                    ))
                } else {
                    None
                };
                if let Some(posed) = animated {
                    lumark_unreal::render_static_mesh_orbit(
                        &posed, 900, 600, self.yaw, self.pitch, self.zoom,
                    )
                } else {
                    let posed = self.skin_pose.then(|| {
                        skin.and_then(|skin| {
                            let (bone, _) =
                                lumark_unreal::skinning::dominant_skinned_bone(skeleton, skin)?;
                            let half = std::f64::consts::PI / 24.0;
                            let rotation = lumark_unreal::skinning::BoneRotation {
                                bone,
                                rotation: [half.sin(), 0.0, 0.0, half.cos()],
                            };
                            Some(lumark_unreal::skinning::skin_mesh(
                                visible,
                                skeleton,
                                skin,
                                &[rotation],
                            ))
                        })
                    });
                    if let Some(Some(posed)) = posed {
                        lumark_unreal::render_static_mesh_orbit(
                            &posed, 900, 600, self.yaw, self.pitch, self.zoom,
                        )
                    } else {
                        lumark_unreal::render_skeletal_mesh_orbit(
                            visible, skeleton, 900, 600, self.yaw, self.pitch, self.zoom,
                        )
                    }
                }
            }
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

    /// 查看器类型名称
    fn kind_label(&self) -> &'static str {
        match self.scene {
            ViewerScene::Mesh { .. } => tr("模型查看器", "Model viewer"),
            ViewerScene::Skeleton(_) => tr("骨骼查看器", "Skeleton viewer"),
            ViewerScene::SkeletalMesh { .. } => tr("骨骼网格查看器", "Skeletal mesh viewer"),
            ViewerScene::Material { .. } => tr("材质查看器", "Material viewer"),
        }
    }

    /// 渲染标题栏
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

    /// 返回当前场景的网格（骨骼场景返回 None）
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

    /// 是否具备可播放动画
    fn has_animation(&self) -> bool {
        let has_animation = matches!(
            self.scene,
            ViewerScene::SkeletalMesh {
                animation: Some(_),
                ..
            }
        );
        let has_skin = matches!(
            &self.scene,
            ViewerScene::SkeletalMesh { skins, .. }
                if skins.iter().any(Option::is_some)
        );
        has_animation && has_skin
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

    /// 按帧步进动画（正数向后，负数向前）
    fn step_animation(&mut self, frames: i32, cx: &mut Context<Self>) {
        let Some((length, total)) = self.animation_info() else {
            return;
        };
        if length <= 0.0 {
            return;
        }
        let timeline = AnimationTimeline::new(length, total, self.animation_time);
        self.animation_playing = false;
        self.animation_time = timeline.stepped(frames);
        self.render_preview();
        cx.notify();
    }

    /// 回到动画开头
    fn reset_animation(&mut self, cx: &mut Context<Self>) {
        self.animation_playing = false;
        self.animation_time = 0.0;
        self.render_preview();
        cx.notify();
    }

    /// 播放/暂停动画
    fn toggle_animation(&mut self, cx: &mut Context<Self>) {
        if !self.has_animation() {
            return;
        }
        self.animation_playing = !self.animation_playing;
        if !self.animation_playing {
            self.animation_time = 0.0;
        }
        if self.animation_playing {
            self.spawn_animation_loop(cx);
        }
        self.render_preview();
        cx.notify();
    }

    /// 以约 20fps 推进动画时间并重绘
    fn spawn_animation_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .spawn(async {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    })
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if !this.animation_playing {
                            return false;
                        }
                        let Some(length) = this.animation_length() else {
                            this.animation_playing = false;
                            return false;
                        };
                        if length <= 0.0 {
                            this.animation_playing = false;
                            return false;
                        }
                        this.animation_time = (this.animation_time + 0.05) % length;
                        this.render_preview();
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    /// 渲染工具栏
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
            .children((0..self.hidden_sections.len()).map(|index| {
                let visible = !self.hidden_sections[index];
                let mut section_button = button(format!("model-section-{index}"))
                    .xsmall()
                    .label(format!("S{index}"))
                    .tooltip(tr(
                        "切换 section/材质可见性",
                        "Toggle section/material visibility",
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(hidden) = this.hidden_sections.get_mut(index) {
                            *hidden = !*hidden;
                        }
                        this.render_preview();
                        cx.notify();
                    }));
                if visible {
                    section_button = section_button.primary();
                } else {
                    section_button = section_button.ghost();
                }
                section_button
            }))
            .children(
                self.static_lods()
                    .filter(|lods| lods.len() > 1)
                    .map(|lods| {
                        button("model-lod")
                            .ghost()
                            .label(format!("LOD {}/{}", self.lod_index + 1, lods.len()))
                            .tooltip(tr("切换 LOD", "Switch LOD"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let count = this.static_lods().map_or(0, <[MeshLod]>::len);
                                if count > 1 {
                                    this.lod_index = (this.lod_index + 1) % count;
                                    this.hidden_sections =
                                        vec![
                                            false;
                                            this.scene_mesh().map_or(0, |mesh| mesh.sections.len())
                                        ];
                                    this.render_preview();
                                    cx.notify();
                                }
                            }))
                    }),
            )
            .children(self.has_animation().then(|| {
                let (length, total) = self.animation_info().unwrap_or((0.0, 1));
                let timeline = AnimationTimeline::new(length, total, self.animation_time);
                let ratio = timeline.ratio();
                let label = if self.animation_playing {
                    tr("暂停动画", "Pause animation")
                } else {
                    tr("播放动画", "Play animation")
                };
                let mut play_button = button("model-animation")
                    .ghost()
                    .label(label)
                    .tooltip(tr(
                        "播放/暂停当前 AnimSequence",
                        "Play/pause the current AnimSequence",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_animation(cx);
                    }));
                if self.animation_playing {
                    play_button = play_button.primary();
                }
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(play_button)
                    .child(
                        button("model-animation-start")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Refresh.element(12.))
                            .tooltip(tr("回到开头", "Go to start"))
                            .accessibility_label(tr("回到动画开头", "Go to animation start"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reset_animation(cx);
                            })),
                    )
                    .child(
                        button("model-animation-prev")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::ArrowLeft.element(12.))
                            .tooltip(tr("上一帧", "Previous frame"))
                            .accessibility_label(tr("上一帧", "Previous frame"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.step_animation(-1, cx);
                            })),
                    )
                    .child(
                        button("model-animation-next")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::ArrowRight.element(12.))
                            .tooltip(tr("下一帧", "Next frame"))
                            .accessibility_label(tr("下一帧", "Next frame"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.step_animation(1, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("model-animation-timeline")
                            .w(px(120.))
                            .h(px(4.))
                            .rounded_full()
                            .bg(cx.theme().border)
                            .child(
                                div()
                                    .h(px(4.))
                                    .rounded_full()
                                    .bg(cx.theme().primary)
                                    .w(px(120.0 * ratio)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(timeline.label()),
                    )
            }))
            .child({
                let mut pose_button = button("model-skin-pose")
                    .ghost()
                    .label(tr("蒙皮演示", "Skin pose"))
                    .tooltip(tr(
                        "对主要蒙皮骨骼施加局部旋转，演示 LBS 蒙皮",
                        "Rotate the dominant skinned bone to preview LBS deformation",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skin_pose = !this.skin_pose;
                        this.render_preview();
                        cx.notify();
                    }));
                if self.skin_pose {
                    pose_button = pose_button.primary();
                }
                pose_button
            })
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{}%", (this_zoom_percent(self.zoom)))),
            )
    }

    /// 渲染画布
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

    /// 渲染底部状态栏
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
                ViewerScene::Mesh { lods } => {
                    let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                    match mesh {
                        Some(mesh) => format!(
                            "LOD {}/{} · {} {} · {} {}",
                            self.lod_index + 1,
                            lods.len(),
                            mesh.positions.len(),
                            tr("顶点", "vertices"),
                            mesh.triangle_count(),
                            tr("三角形", "triangles")
                        ),
                        None => tr("无 LOD", "No LOD").to_string(),
                    }
                }
                ViewerScene::Skeleton(skeleton) => {
                    format!("{} {}", skeleton.bones.len(), tr("根骨骼", "bones"))
                }
                ViewerScene::SkeletalMesh { lods, skeleton, .. } => {
                    let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                    match mesh {
                        Some(mesh) => format!(
                            "LOD {}/{} · {} {} · {} {} · {} {}",
                            self.lod_index + 1,
                            lods.len(),
                            mesh.positions.len(),
                            tr("顶点", "vertices"),
                            mesh.triangle_count(),
                            tr("三角形", "triangles"),
                            skeleton.bones.len(),
                            tr("根骨骼", "bones")
                        ),
                        None => tr("无 LOD", "No LOD").to_string(),
                    }
                }
                ViewerScene::Material { viewer } => {
                    let textures = [
                        viewer.base_color.is_some(),
                        viewer.normal.is_some(),
                        viewer.metallic_roughness.is_some(),
                    ]
                    .into_iter()
                    .filter(|present| *present)
                    .count();
                    format!(
                        "{} · {} {}",
                        tr("近似 PBR 材质球", "Approximate PBR material ball"),
                        textures,
                        tr("张贴图", "textures")
                    )
                }
            })
            .child(tr("左键拖拽旋转", "Drag to orbit").to_string())
            .child(format!("{}%", this_zoom_percent(self.zoom)))
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

impl Drop for AssetViewerWindowView {
    /// 删除窗口独占的临时预览图片
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.image_path);
    }
}

/// 缩放百分比
fn this_zoom_percent(zoom: f32) -> u32 {
    (zoom * 100.0).round() as u32
}

/// ?section 可见性过滤网格索引
fn filter_sections(mesh: &MeshLod, hidden: &[bool]) -> MeshLod {
    if hidden.iter().all(|hidden| !hidden) {
        return mesh.clone();
    }
    let mut indices = Vec::new();
    let mut sections = Vec::new();
    for (index, section) in mesh.sections.iter().enumerate() {
        if hidden.get(index).copied().unwrap_or(false) {
            continue;
        }
        let start = usize::try_from(section.first_index.max(0)).unwrap_or(0);
        let triangle_count = usize::try_from(section.triangle_count.max(0)).unwrap_or(0);
        let end = start
            .saturating_add(triangle_count * 3)
            .min(mesh.indices.len());
        let first_index = indices.len() as i32;
        indices.extend_from_slice(&mesh.indices[start..end]);
        sections.push(MeshSection {
            material_index: section.material_index,
            first_index,
            triangle_count: section.triangle_count,
        });
    }
    MeshLod {
        positions: mesh.positions.clone(),
        indices,
        sections,
    }
}

/// 把正方形材质球画面放到查看器画布中央，两侧用材质球背景色填充。
fn compose_square_canvas(square: &[u8], square_size: u32, width: usize, height: usize) -> Vec<u8> {
    let background = [24u8, 27, 33, 255];
    let mut canvas = vec![0u8; width * height * 4];
    for pixel in canvas.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&background);
    }
    let square_size = square_size as usize;
    let offset_x = width.saturating_sub(square_size) / 2;
    let offset_y = height.saturating_sub(square_size) / 2;
    for y in 0..square_size.min(height.saturating_sub(offset_y)) {
        let source_start = y * square_size * 4;
        let source_end = source_start + square_size.min(width.saturating_sub(offset_x)) * 4;
        let target_start = ((y + offset_y) * width + offset_x) * 4;
        let target_end = target_start + (source_end - source_start);
        canvas[target_start..target_end].copy_from_slice(&square[source_start..source_end]);
    }
    canvas
}
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
mod tests;
