use super::*;
use crate::render_service::render_target_to_image;

impl AssetViewerWindowView {
    pub(super) fn render_preview(&mut self) {
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
        let Some(target) = lumark_render::RenderTarget::new(900, 600, rgba) else {
            self.error = Some(tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string());
            return;
        };
        let Some(image) = render_target_to_image(&target) else {
            self.error = Some(tr("资产渲染尺寸不匹配", "Asset render size mismatch").to_string());
            return;
        };
        self.image = Some(image);
        self.error = None;
    }
}

/// ?section 可见性过滤网格索引
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
