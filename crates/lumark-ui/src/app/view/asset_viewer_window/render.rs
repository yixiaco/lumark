//! 查看器渲染：网格/骨骼网格走引擎无关场景 + 共享渲染服务（GPU 优先），
//! 骨骼线与材质球仍使用轻量光栅函数。

use lumark_render::RenderSettings;
use lumark_scene::{Axis, NodeKind, Scene, SceneNode, Transform};
#[cfg(test)]
use lumark_unreal::MeshSection;
use lumark_unreal::{
    AnimSequenceData, BonePose, MeshLod, SkeletalMeshPreview, Skeleton, SkinWeights, convert_mesh,
};

use super::*;
use crate::render_service::{render_scene_image, render_target_to_image};

impl AssetViewerWindowView {
    pub(super) fn render_preview(&mut self) {
        // 骨骼/材质查看器沿用轻量光栅（需要骨骼线段/材质球专用绘制）。
        let legacy_rgba = match &self.scene {
            ViewerScene::Skeleton(skeleton) => Some(lumark_unreal::render_skeleton_orbit(
                skeleton, 900, 600, self.yaw, self.pitch, self.zoom,
            )),
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
                Some(compose_square_canvas(&ball, 600, 900, 600))
            }
            _ => None,
        };
        if let Some(rgba) = legacy_rgba {
            self.apply_rgba(rgba);
            return;
        }

        let scene = match &self.scene {
            ViewerScene::Mesh { lods } => {
                let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                mesh.map(|mesh| self.static_scene(mesh))
            }
            ViewerScene::SkeletalMesh {
                lods,
                skeleton,
                skins,
                animation,
            } => {
                let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                let skin = skins
                    .get(self.lod_index)
                    .and_then(Option::as_ref)
                    .or_else(|| skins.first().and_then(Option::as_ref));
                mesh.map(|mesh| self.skeletal_scene(mesh, skeleton, skin, animation.as_deref()))
            }
            ViewerScene::Skeleton(_) | ViewerScene::Material { .. } => None,
        };
        match scene {
            Some(scene) => self.render_scene_preview(scene),
            None => {
                self.error = Some(tr("当前场景没有可渲染网格", "No renderable mesh").to_string());
            }
        }
    }

    /// 构建静态网格场景。
    fn static_scene(&self, mesh: &MeshLod) -> Scene {
        let mut scene = Scene::new(self.name.clone());
        scene.source = self.name.clone();
        scene.up_axis = Axis::Z;
        scene.unit_scale = 0.01;
        let handle = scene.add_mesh(convert_mesh(mesh, &self.name));
        scene.add_node(SceneNode {
            name: self.name.clone(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(handle),
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        scene
    }

    /// 构建骨骼网格场景并写入动画/蒙皮姿态。
    fn skeletal_scene(
        &self,
        mesh: &MeshLod,
        skeleton: &Skeleton,
        skin: Option<&SkinWeights>,
        animation: Option<&AnimSequenceData>,
    ) -> Scene {
        let preview = SkeletalMeshPreview {
            skeleton: skeleton.clone(),
            material_slots: mesh.sections.len().max(1),
            material_names: Vec::new(),
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
        let mut scene = lumark_unreal::skeletal_mesh_scene(&self.name, &preview, &[]);
        scene.name = self.name.clone();
        if self.animation_playing || self.animation_time > 0.0 {
            if let Some(animation) = animation {
                lumark_unreal::apply_animation(
                    &mut scene,
                    skeleton,
                    animation,
                    self.animation_time,
                );
            }
        } else if self.skin_pose
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

    /// 构建当前场景与渲染设置；骨骼/材质查看器返回 `None`（走轻量光栅）。
    pub(super) fn scene_and_settings(&self) -> Option<(Scene, RenderSettings)> {
        let scene = match &self.scene {
            ViewerScene::Mesh { lods } => {
                let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                mesh.map(|mesh| self.static_scene(mesh))
            }
            ViewerScene::SkeletalMesh {
                lods,
                skeleton,
                skins,
                animation,
            } => {
                let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                let skin = skins
                    .get(self.lod_index)
                    .and_then(Option::as_ref)
                    .or_else(|| skins.first().and_then(Option::as_ref));
                mesh.map(|mesh| self.skeletal_scene(mesh, skeleton, skin, animation.as_deref()))
            }
            ViewerScene::Skeleton(_) | ViewerScene::Material { .. } => None,
        }?;
        let settings = RenderSettings {
            width: 900,
            height: 600,
            supersample: 2,
            srgb_textures: self.srgb_textures,
            background: [0.055, 0.06, 0.075],
            yaw_degrees: self.yaw,
            pitch_degrees: self.pitch,
            distance_scale: (2.2 / self.zoom).clamp(0.15, 20.0),
            shading: self.shading,
            show_grid: self.show_grid,
            show_bones: self.show_bones,
            hidden_sections: self
                .hidden_sections
                .iter()
                .enumerate()
                .filter_map(|(index, hidden)| hidden.then_some(index))
                .collect(),
            ..RenderSettings::default()
        };
        Some((scene, settings))
    }

    /// 使用共享渲染服务渲染场景（GPU 优先，失败回退 CPU）。
    fn render_scene_preview(&mut self, scene: Scene) {
        let settings = RenderSettings {
            width: 900,
            height: 600,
            supersample: 2,
            srgb_textures: self.srgb_textures,
            background: [0.055, 0.06, 0.075],
            yaw_degrees: self.yaw,
            pitch_degrees: self.pitch,
            distance_scale: (2.2 / self.zoom).clamp(0.15, 20.0),
            shading: self.shading,
            show_grid: self.show_grid,
            show_bones: self.show_bones,
            hidden_sections: self
                .hidden_sections
                .iter()
                .enumerate()
                .filter_map(|(index, hidden)| hidden.then_some(index))
                .collect(),
            ..RenderSettings::default()
        };
        match render_scene_image(&scene, &settings) {
            Ok(rendered) => self.apply_rendered(rendered),
            Err(error) => self.error = Some(error),
        }
    }

    /// 应用渲染结果。
    pub(super) fn apply_rendered(&mut self, rendered: crate::render_service::RenderedImage) {
        self.image = Some(rendered.image);
        self.last_backend = Some(rendered.backend.label());
        self.last_stats = Some(rendered.stats);
        self.error = None;
    }

    /// 旧路径 RGBA → RenderImage。
    fn apply_rgba(&mut self, rgba: Vec<u8>) {
        let Some(target) = lumark_render::RenderTarget::new(900, 600, rgba) else {
            self.error = Some(tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string());
            return;
        };
        let Some(image) = render_target_to_image(&target) else {
            self.error = Some(tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string());
            return;
        };
        self.image = Some(image);
        self.last_backend = None;
        self.last_stats = None;
        self.error = None;
    }
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
