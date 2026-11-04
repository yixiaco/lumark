//! 查看器渲染：把当前视角整理成只读快照，由后台线程构建场景并离屏渲染。
//!
//! UI 线程只复制 `Arc<ViewerScene>` 与相机/开关参数（[`RenderSnapshot`]），
//! 场景构建、材质贴图与 GPU 渲染都在 worker 内完成；骨骼/材质查看器的轻量
//! 光栅同样走 worker，避免阻塞界面线程。

use std::sync::Arc;

use gpui_kit::RenderImage;
use lumark_render::RenderSettings;
use lumark_scene::Scene;
#[cfg(test)]
use lumark_unreal::MeshSection;
use lumark_unreal::{
    AnimSequenceData, BonePose, MeshLod, SkeletalMeshPreview, Skeleton, SkinWeights,
};

use super::*;
use crate::render_service::{render_scene_image, render_target_to_image};

/// 查看器画布宽度（离屏渲染固定尺寸，显示时按 Contain 缩放）。
pub(super) const VIEWER_WIDTH: u32 = 900;
/// 查看器画布高度。
pub(super) const VIEWER_HEIGHT: u32 = 600;
/// 材质球画面边宽（居中放入画布）。
const MATERIAL_BALL_SIZE: u32 = 600;

/// 渲染画质档位：交互时便宜，静止后补一帧精细画面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Interactive 仅测试路径使用
pub(super) enum RenderQuality {
    /// 交互帧：SS=1、无 MSAA。
    Interactive,
    /// 拖拽帧：在 Interactive 基础上再降到 0.75 分辨率；静止后补全分辨率。
    Dragging,
    /// 静止帧：SS=2 + 4×MSAA。
    Settled,
}

impl RenderQuality {
    /// 该档位的超采样倍数。
    fn supersample(self) -> u32 {
        match self {
            Self::Interactive | Self::Dragging => 1,
            Self::Settled => 2,
        }
    }

    /// 该档位的 MSAA 采样数。
    fn msaa_samples(self) -> u32 {
        match self {
            Self::Interactive | Self::Dragging => 1,
            Self::Settled => 4,
        }
    }
}

/// 一帧渲染的只读快照：UI 线程构建成本是 O(1)（只有 Arc 与少量参数）。
pub(super) struct RenderSnapshot {
    /// 查看器场景（Arc 共享，worker 内按需构建 `Scene`）。
    pub(super) scene: Arc<ViewerScene>,
    /// 资产名（用作场景名）。
    pub(super) name: String,
    /// 轨道相机水平角。
    pub(super) yaw: f32,
    /// 轨道相机俯仰角。
    pub(super) pitch: f32,
    /// 缩放倍率。
    pub(super) zoom: f32,
    /// 右键平移累计屏幕像素。
    pub(super) pan: [f32; 2],
    /// 当前 LOD 索引。
    pub(super) lod_index: usize,
    /// 是否正在播放动画。
    pub(super) animation_playing: bool,
    /// 动画时间（秒）。
    pub(super) animation_time: f32,
    /// 是否显示蒙皮演示姿态。
    pub(super) skin_pose: bool,
    /// 着色模式。
    pub(super) shading: ShadingMode,
    /// 是否显示地面网格。
    pub(super) show_grid: bool,
    /// 是否显示骨骼线。
    pub(super) show_bones: bool,
    /// 线框模式。
    pub(super) wireframe: bool,
    /// 基础色/自发光贴图是否按 sRGB 采样。
    pub(super) srgb_textures: bool,
    /// 是否把顶点色乘到基础色。
    pub(super) vertex_colors: bool,
    /// 被隐藏的 section。
    pub(super) hidden_sections: Vec<bool>,
    /// 画质档位。
    pub(super) quality: RenderQuality,
    /// 实际离屏渲染宽高。
    pub(super) width: u32,
    pub(super) height: u32,
    /// 固定取景包围盒（全部 LOD 并集）。
    pub(super) fit_bounds: Option<lumark_scene::Bounds>,
}

/// 一帧渲染结果。
pub(super) struct ViewerFrame {
    /// GPUI 可直接显示的图像。
    pub(super) image: Arc<RenderImage>,
    /// 实际渲染后端标签（GPUI 路径有，轻量光栅为空）。
    pub(super) backend: Option<String>,
    /// 场景规模统计（GPUI 路径有）。
    pub(super) stats: Option<lumark_render::SceneStats>,
}

impl AssetViewerWindowView {
    /// 收集当前视角的只读快照（UI 线程开销只有 Arc/小数组复制）。
    pub(super) fn snapshot(&self, quality: RenderQuality) -> RenderSnapshot {
        let scale = if quality == RenderQuality::Dragging {
            0.75
        } else {
            1.0
        };
        let width = ((VIEWER_WIDTH as f32 * scale).round() as u32).max(16);
        let height = ((VIEWER_HEIGHT as f32 * scale).round() as u32).max(16);
        RenderSnapshot {
            scene: self.scene.clone(),
            name: self.name.clone(),
            yaw: self.yaw,
            pitch: self.pitch,
            zoom: self.zoom,
            pan: self.pan,
            lod_index: self.lod_index,
            animation_playing: self.animation_playing,
            animation_time: self.animation_time,
            skin_pose: self.skin_pose,
            shading: self.shading,
            show_grid: self.show_grid,
            show_bones: self.show_bones,
            wireframe: self.wireframe,
            srgb_textures: self.srgb_textures,
            vertex_colors: self.vertex_colors,
            hidden_sections: self.hidden_sections.clone(),
            quality,
            width,
            height,
            fit_bounds: self.fit_bounds,
        }
    }

    /// 应用一帧渲染结果，返回被替换的旧图像。
    ///
    /// GPUI 的纹理图集按 `ImageId` 缓存，`RenderImage::new` 每帧都会分配新
    /// 编号；旧图像必须交给调用方用 `App::drop_image` 释放，否则连续渲染的
    /// 查看器会把每个历史帧都留在图集里（表现为内存持续上涨）。
    pub(super) fn apply_frame(&mut self, frame: ViewerFrame) -> Option<Arc<RenderImage>> {
        self.last_backend = frame.backend;
        self.last_stats = frame.stats;
        self.error = None;
        self.image.replace(frame.image)
    }

    /// 同步渲染一帧（仅测试/无事件循环场景使用；正式窗口走后台帧泵）。
    #[cfg(test)]
    pub(super) fn render_preview(&mut self) {
        let snapshot = self.snapshot(RenderQuality::Interactive);
        match render_snapshot(&snapshot) {
            // 测试路径没有图集，替换后直接释放旧帧即可。
            Ok(frame) => drop(self.apply_frame(frame)),
            Err(error) => self.error = Some(error),
        }
    }
}

/// 渲染一帧快照：骨骼/材质查看器走轻量光栅，网格查看器构建场景后走共享服务。
pub(super) fn render_snapshot(snapshot: &RenderSnapshot) -> Result<ViewerFrame, String> {
    match &*snapshot.scene {
        ViewerScene::Skeleton(skeleton) => {
            let rgba = lumark_unreal::render_skeleton_orbit(
                skeleton,
                snapshot.width,
                snapshot.height,
                snapshot.yaw,
                snapshot.pitch,
                snapshot.zoom,
            );
            frame_from_rgba(rgba, snapshot.width, snapshot.height)
        }
        ViewerScene::Material { viewer } => {
            let orm_layout = viewer
                .appearance
                .texture(lumark_unreal::MaterialTextureKind::MetallicRoughness)
                .map(|texture| lumark_unreal::materials::detect_orm_layout(&texture.name))
                .unwrap_or(lumark_scene::OrmLayout::AoRoughMetal);
            let settings = lumark_unreal::MaterialBallSettings {
                // 与 3D 查看器默认一致：头灯 + FModel 法线约定 + 双面继承。
                headlight: true,
                fmodel_normals: true,
                orm_layout,
                two_sided: viewer.appearance.two_sided,
            };
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
                viewer.occlusion.as_ref().map(|texture| {
                    lumark_unreal::MaterialPreviewTexture::new(
                        &texture.rgba,
                        texture.width,
                        texture.height,
                    )
                }),
                settings,
                snapshot.yaw,
                snapshot.pitch,
                snapshot.zoom,
                MATERIAL_BALL_SIZE,
            );
            let rgba = compose_square_canvas(
                &ball,
                MATERIAL_BALL_SIZE,
                snapshot.width as usize,
                snapshot.height as usize,
            );
            frame_from_rgba(rgba, snapshot.width, snapshot.height)
        }
        _ => {
            let scene = build_scene(snapshot)
                .ok_or_else(|| tr("当前场景没有可渲染网格", "No renderable mesh").to_string())?;
            let settings = viewer_settings(snapshot);
            let rendered = render_scene_image(&scene, &settings)?;
            Ok(ViewerFrame {
                image: rendered.image,
                backend: Some(rendered.backend.label()),
                stats: Some(rendered.stats),
            })
        }
    }
}

/// 由快照构建引擎无关场景（在 worker 线程调用，重数据复制不占用 UI 线程）。
fn build_scene(snapshot: &RenderSnapshot) -> Option<Scene> {
    match &*snapshot.scene {
        ViewerScene::Mesh { lods, materials } => {
            let mesh = current_lod(lods, snapshot.lod_index)?;
            Some(static_scene(&snapshot.name, mesh, materials))
        }
        ViewerScene::SkeletalMesh {
            lods,
            skeleton,
            skins,
            materials,
            animation,
        } => {
            let mesh = current_lod(lods, snapshot.lod_index)?;
            let skin = skins
                .get(snapshot.lod_index)
                .and_then(Option::as_ref)
                .or_else(|| skins.first().and_then(Option::as_ref));
            Some(skeletal_scene(
                snapshot,
                mesh,
                skeleton,
                skin,
                materials,
                animation.as_deref(),
            ))
        }
        ViewerScene::Skeleton(_) | ViewerScene::Material { .. } => None,
    }
}

/// 取当前 LOD（越界时回退到首个）。
fn current_lod(lods: &[MeshLod], lod_index: usize) -> Option<&MeshLod> {
    lods.get(lod_index).or_else(|| lods.first())
}

/// 构建静态网格场景（含已解析材质与贴图）。
fn static_scene(
    name: &str,
    mesh: &MeshLod,
    materials: &[lumark_unreal::ResolvedMaterial],
) -> Scene {
    let static_mesh = lumark_unreal::StaticMesh {
        name: name.to_string(),
        lod: mesh.clone(),
        lod_count: 1,
        lods: vec![mesh.clone()],
        material_slots: materials
            .iter()
            .map(|material| material.slot.clone())
            .collect(),
        // 材质已在打开查看器前解析完成，这里不再重复走对象引用解析。
        material_imports: Vec::new(),
    };
    lumark_unreal::static_mesh_scene(name, &static_mesh, materials)
}

/// 构建骨骼网格场景并写入动画/蒙皮姿态。
fn skeletal_scene(
    snapshot: &RenderSnapshot,
    mesh: &MeshLod,
    skeleton: &Skeleton,
    skin: Option<&SkinWeights>,
    materials: &[lumark_unreal::ResolvedMaterial],
    animation: Option<&AnimSequenceData>,
) -> Scene {
    let preview = SkeletalMeshPreview {
        skeleton: skeleton.clone(),
        material_slots: mesh.sections.len().max(1),
        material_names: materials
            .iter()
            .map(|material| material.slot.name.clone())
            .collect(),
        material_imports: Vec::new(),
        lod_count: 1,
        lod_sections: mesh.sections.len(),
        required_bones: mesh.positions.len(),
        bone_map: Vec::new(),
        lod_vertices: mesh.positions.len(),
        lod_triangles: mesh.indices.len() / 3,
        geometry: Some(mesh.clone()),
        lods: vec![mesh.clone()],
        lod_skins: Vec::new(),
        skin: skin.cloned(),
    };
    let mut scene = lumark_unreal::skeletal_mesh_scene(&snapshot.name, &preview, materials);
    scene.name = snapshot.name.clone();
    if snapshot.animation_playing || snapshot.animation_time > 0.0 {
        if let Some(animation) = animation {
            lumark_unreal::apply_animation(
                &mut scene,
                skeleton,
                animation,
                snapshot.animation_time,
            );
        }
    } else if snapshot.skin_pose
        && let Some(skin) = skin
        && let Some((bone, _)) = lumark_unreal::skinning::dominant_skinned_bone(skeleton, skin)
    {
        let half = std::f64::consts::PI / 24.0;
        let mut poses: Vec<BonePose> = skeleton
            .bones
            .iter()
            .map(|bone| BonePose {
                translation: bone.translation,
                rotation: bone.rotation,
                scale: bone.scale,
            })
            .collect();
        if let Some(pose) = poses.get_mut(bone) {
            pose.rotation = [half.sin(), 0.0, 0.0, half.cos()];
        }
        let matrices = lumark_unreal::skin_matrices_from_poses(skeleton, &poses);
        lumark_unreal::apply_skin_pose(&mut scene, &matrices);
    }
    scene
}

/// 由快照生成渲染设置：查看器与详情预览共用 Snooper 光照预设。
fn viewer_settings(snapshot: &RenderSnapshot) -> RenderSettings {
    let mut settings = RenderSettings::snooper_viewer(snapshot.width, snapshot.height);
    settings.supersample = snapshot.quality.supersample();
    settings.msaa_samples = snapshot.quality.msaa_samples();
    settings.srgb_textures = snapshot.srgb_textures;
    settings.wireframe = snapshot.wireframe;
    settings.yaw_degrees = snapshot.yaw;
    settings.pitch_degrees = snapshot.pitch;
    settings.pan_pixels = snapshot.pan;
    settings.distance_scale = (1.05 / snapshot.zoom).clamp(0.12, 12.0);
    settings.fit_bounds = snapshot.fit_bounds;
    settings.logical_height = VIEWER_HEIGHT;
    settings.shading = snapshot.shading;
    // FModel 默认不把顶点色乘到基础色；查看器可在工具栏打开。
    settings.vertex_colors = snapshot.vertex_colors;
    settings.show_grid = snapshot.show_grid;
    settings.show_bones = snapshot.show_bones;
    settings.hidden_sections = snapshot
        .hidden_sections
        .iter()
        .enumerate()
        .filter_map(|(index, hidden)| hidden.then_some(index))
        .collect();
    settings
}

/// RGBA 画布 → 可显示的帧。
fn frame_from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<ViewerFrame, String> {
    let target = lumark_render::RenderTarget::new(width, height, rgba)
        .ok_or_else(|| tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string())?;
    let image = render_target_to_image(&target)
        .ok_or_else(|| tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string())?;
    Ok(ViewerFrame {
        image,
        backend: None,
        stats: None,
    })
}

/// section 可见性过滤网格索引（仅测试使用）。
#[cfg(test)]
pub(super) fn filter_sections(mesh: &MeshLod, hidden: &[bool]) -> MeshLod {
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
        attributes: Default::default(),
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
