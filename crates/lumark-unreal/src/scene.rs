//! 把 Unreal 资产转换为引擎无关的 [`lumark_scene::Scene`]。
//!
//! 这一层只做数据整形：f64 LWC → f32、UE 坐标系约定标记、材质外观与贴图映射；
//! 渲染层和 UI 不直接依赖 Unreal 类型，也为未来的 Unity 适配保留同一入口。

use lumark_scene::{
    Axis, Bounds, MaterialData, MaterialHandle, MaterialTextures, MeshData, MeshHandle,
    MeshSection, MeshSkin, NodeKind, OrmLayout, Scene, SceneNode, SceneNodeId, TextureData,
    Transform,
};

use crate::materials::detect_orm_layout;
use crate::{
    AnimSequenceData, MaterialAppearance, MaterialTextureKind, MaterialViewerData, MeshLod,
    ResolvedMaterial, SkeletalMeshPreview, Skeleton, StaticMesh, sample_animation,
};

/// 1 UE 单位（厘米）对应的场景单位（米）。
pub const UE_UNIT_SCALE: f32 = 0.01;

/// 由静态网格与已解析材质构建场景。
pub fn static_mesh_scene(
    scene_name: &str,
    mesh: &StaticMesh,
    materials: &[ResolvedMaterial],
) -> Scene {
    let mut scene = Scene::new(scene_name);
    scene.source = mesh.name.clone();
    scene.up_axis = Axis::Z;
    scene.unit_scale = UE_UNIT_SCALE;
    let slot_materials = add_resolved_materials(&mut scene, materials);
    let mesh_handle = scene.add_mesh(convert_mesh(&mesh.lod, &mesh.name));
    let node = SceneNode {
        name: mesh.name.clone(),
        parent: None,
        transform: Transform::IDENTITY,
        mesh: Some(mesh_handle),
        materials: slot_materials,
        visible: true,
        kind: NodeKind::StaticMesh,
        metadata: vec![
            ("LOD".to_string(), mesh.lod_count.to_string()),
            ("Section".to_string(), mesh.lod.sections.len().to_string()),
        ],
    };
    scene.add_node(node);
    scene
}

/// 由骨骼网格与已解析材质构建场景。
///
/// 参考骨架以空节点形式加入节点树（供骨骼树面板使用），首个可渲染 LOD 挂在
/// 根节点下；动画与 GPU 蒙皮由渲染层处理。
pub fn skeletal_mesh_scene(
    scene_name: &str,
    preview: &SkeletalMeshPreview,
    materials: &[ResolvedMaterial],
) -> Scene {
    let mut scene = Scene::new(scene_name);
    scene.source = preview.skeleton.name.clone();
    scene.up_axis = Axis::Z;
    scene.unit_scale = UE_UNIT_SCALE;
    let slot_materials = add_resolved_materials(&mut scene, materials);

    // 参考骨架节点。
    let mut bone_nodes: Vec<Option<SceneNodeId>> = vec![None; preview.skeleton.bones.len()];
    for (index, bone) in preview.skeleton.bones.iter().enumerate() {
        let parent = usize::try_from(bone.parent_index)
            .ok()
            .filter(|parent| *parent < index)
            .and_then(|parent| bone_nodes.get(parent).copied().flatten());
        let node = SceneNode {
            name: bone.name.clone(),
            parent,
            transform: Transform {
                translation: [
                    bone.translation[0] as f32,
                    bone.translation[1] as f32,
                    bone.translation[2] as f32,
                ],
                rotation: [
                    bone.rotation[0] as f32,
                    bone.rotation[1] as f32,
                    bone.rotation[2] as f32,
                    bone.rotation[3] as f32,
                ],
                scale: [
                    bone.scale[0] as f32,
                    bone.scale[1] as f32,
                    bone.scale[2] as f32,
                ],
            },
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Other("Bone".to_string()),
            metadata: Vec::new(),
        };
        bone_nodes[index] = Some(scene.add_node(node));
    }

    if let Some(lod) = preview.lods.first().or(preview.geometry.as_ref()) {
        let mut mesh_data = convert_mesh(lod, &preview.skeleton.name);
        if let Some(skin) = &preview.skin {
            fill_mesh_skin(&mut mesh_data, &preview.skeleton, skin, &bone_nodes);
        }
        let mesh_handle = scene.add_mesh(mesh_data);
        let node = SceneNode {
            name: preview.skeleton.name.clone(),
            parent: bone_nodes.first().copied().flatten(),
            transform: Transform::IDENTITY,
            mesh: Some(mesh_handle),
            materials: slot_materials,
            visible: true,
            kind: NodeKind::SkeletalMesh,
            metadata: vec![
                ("骨骼".to_string(), preview.skeleton.bones.len().to_string()),
                ("LOD".to_string(), preview.lod_count.to_string()),
                (
                    "蒙皮顶点".to_string(),
                    preview
                        .skin
                        .as_ref()
                        .map(|skin| skin.vertices)
                        .unwrap_or(0)
                        .to_string(),
                ),
            ],
        };
        scene.add_node(node);
    }
    scene
}

/// 用动画在指定时间更新场景蒙皮姿态。
pub fn apply_animation(
    scene: &mut Scene,
    skeleton: &Skeleton,
    animation: &AnimSequenceData,
    time_seconds: f32,
) -> bool {
    let sample = sample_animation(skeleton, animation, time_seconds);
    apply_skin_pose(scene, &sample.skin_matrices)
}

/// 把最终蒙皮矩阵写入场景中第一个蒙皮网格。
pub fn apply_skin_pose(scene: &mut Scene, skin_matrices: &[[f32; 16]]) -> bool {
    for mesh in &mut scene.meshes {
        let Some(skin) = mesh.skin.as_mut() else {
            continue;
        };
        if skin_matrices.len() != skin.joints.len() {
            return false;
        }
        skin.pose = Some(skin_matrices.to_vec());
        return true;
    }
    false
}

/// 填充逐顶点关节索引、权重与 inverse bind 矩阵。
fn fill_mesh_skin(
    mesh: &mut MeshData,
    skeleton: &Skeleton,
    skin: &crate::SkinWeights,
    bone_nodes: &[Option<SceneNodeId>],
) {
    let joints: Vec<SceneNodeId> = bone_nodes.iter().flatten().copied().collect();
    if joints.len() != skeleton.bones.len() {
        return;
    }
    let inverse_bind_matrices = bind_inverse_matrices(skeleton);
    let mut joint_indices = Vec::with_capacity(mesh.positions.len());
    let mut joint_weights = Vec::with_capacity(mesh.positions.len());
    for vertex in 0..mesh.positions.len() {
        let mut indices = [0u16; 4];
        let mut weights = [0f32; 4];
        if let (Some(influences), Some(raw_weights)) =
            (skin.influences.get(vertex), skin.weights.get(vertex))
        {
            let total: f32 = raw_weights.iter().map(|weight| f32::from(*weight)).sum();
            let mut slot = 0;
            for influence in 0..raw_weights.len().min(4) {
                let weight = f32::from(raw_weights[influence]);
                let bone = influences[influence];
                if weight <= 0.0 || bone as usize >= joints.len() || slot >= 4 {
                    continue;
                }
                indices[slot] = bone;
                weights[slot] = if total > 0.0 { weight / total } else { 0.0 };
                slot += 1;
            }
        }
        joint_indices.push(indices);
        joint_weights.push(weights);
    }
    mesh.skin = Some(MeshSkin {
        joints,
        joint_indices,
        joint_weights,
        inverse_bind_matrices,
        pose: None,
    });
}

/// 计算每个关节的 inverse bind 矩阵（列主序 4×4）。
fn bind_inverse_matrices(skeleton: &Skeleton) -> Vec<[f32; 16]> {
    let mut worlds: Vec<[[f32; 4]; 4]> = Vec::with_capacity(skeleton.bones.len());
    for bone in &skeleton.bones {
        let local = Transform {
            translation: [
                bone.translation[0] as f32,
                bone.translation[1] as f32,
                bone.translation[2] as f32,
            ],
            rotation: [
                bone.rotation[0] as f32,
                bone.rotation[1] as f32,
                bone.rotation[2] as f32,
                bone.rotation[3] as f32,
            ],
            scale: [
                bone.scale[0] as f32,
                bone.scale[1] as f32,
                bone.scale[2] as f32,
            ],
        }
        .matrix();
        let world = usize::try_from(bone.parent_index)
            .ok()
            .and_then(|parent| worlds.get(parent))
            .map(|parent| multiply_matrix(parent, &local))
            .unwrap_or(local);
        worlds.push(world);
    }
    worlds.iter().map(inverse_matrix).collect()
}

/// 列主序 4×4 乘法。
fn multiply_matrix(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0f32; 4]; 4];
    for (column, target) in result.iter_mut().enumerate() {
        for (row, value) in target.iter_mut().enumerate() {
            *value = (0..4).map(|index| a[index][row] * b[column][index]).sum();
        }
    }
    result
}

/// 列主序 4×4 求逆。
fn inverse_matrix(input: &[[f32; 4]; 4]) -> [f32; 16] {
    let m: Vec<f64> = (0..4)
        .flat_map(|column| input[column].iter().map(|value| f64::from(*value)))
        .collect();
    let mut inverse = [0.0f64; 16];
    inverse[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inverse[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inverse[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inverse[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inverse[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inverse[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inverse[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inverse[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inverse[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inverse[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inverse[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inverse[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inverse[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inverse[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inverse[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inverse[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let determinant =
        m[0] * inverse[0] + m[1] * inverse[4] + m[2] * inverse[8] + m[3] * inverse[12];
    let scale = if determinant.abs() <= f64::EPSILON {
        0.0
    } else {
        1.0 / determinant
    };
    let mut output = [0.0f32; 16];
    for (index, value) in inverse.iter().enumerate() {
        output[index] = (*value * scale) as f32;
    }
    output
}

/// 把一个已解析材质加入场景，返回句柄。
pub fn add_material(scene: &mut Scene, material: &ResolvedMaterial) -> Option<MaterialHandle> {
    let appearance = material.appearance.as_ref()?;
    Some(scene.add_material(scene_material(
        material.slot.name.as_str(),
        appearance,
        material.viewer.as_ref(),
    )))
}

/// 把 UE 近似外观转换为场景材质。
pub fn scene_material(
    name: &str,
    appearance: &MaterialAppearance,
    viewer: Option<&MaterialViewerData>,
) -> MaterialData {
    let mut textures = MaterialTextures::default();
    if let Some(viewer) = viewer {
        textures.base_color = viewer
            .base_color
            .as_ref()
            .map(|image| texture_data(name, "BaseColor", image));
        textures.normal = viewer
            .normal
            .as_ref()
            .map(|image| texture_data(name, "Normal", image));
        textures.metallic_roughness = viewer
            .metallic_roughness
            .as_ref()
            .map(|image| texture_data(name, "ORM", image));
        textures.occlusion = viewer
            .occlusion
            .as_ref()
            .map(|image| texture_data(name, "AO", image));
    }
    // 按金属/粗糙度贴图的命名自动识别通道布局；无贴图时用默认 ORM 布局。
    let orm_layout = appearance
        .texture(MaterialTextureKind::MetallicRoughness)
        .map(|reference| detect_orm_layout(&reference.name))
        .unwrap_or(OrmLayout::AoRoughMetal);
    // 半透明材质走混合，不做 alpha 裁剪；Masked 材质按阈值裁剪，Opaque 忽略贴图 alpha。
    // 若基础色贴图的 alpha 几乎全透明，通常说明它是颜色遮罩而非透明度通道，
    // 此时也不裁剪，避免把整块布料/皮肤裁掉。
    let alpha_cutoff = if appearance.translucent {
        0.0
    } else if appearance.masked {
        let visible = viewer
            .and_then(|viewer| viewer.base_color.as_ref())
            .filter(|image| image.rgba.len() >= 4)
            .map(|image| {
                let total = image.rgba.len() / 4;
                let opaque = image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] >= 128)
                    .count();
                opaque * 20 >= total
            })
            .unwrap_or(true);
        if visible { 0.5 } else { 0.0 }
    } else {
        0.0
    };
    MaterialData {
        name: name.to_string(),
        base_color: [
            appearance.base_color[0],
            appearance.base_color[1],
            appearance.base_color[2],
            1.0,
        ],
        metallic: appearance.metallic.clamp(0.0, 1.0),
        roughness: appearance.roughness.clamp(0.0, 1.0),
        emissive: appearance.emissive,
        textures,
        two_sided: appearance.two_sided,
        orm_layout,
        alpha_cutoff,
        translucent: appearance.translucent,
    }
}

/// 把 UE 索引缓冲转换为场景索引：按三角形交换第 2/3 个顶点。
///
/// UE 使用左手坐标系、正面为顺时针；渲染器与 glTF 一样按右手系逆时针
/// 正面剔除。FModel 通过 Y/Z 交换完成手性转换，这里用等价的绕序翻转，
/// 保持渲染器的 CCW 约定不变；索引数量与 section 结构都不变，尾部不足
/// 一个三角形的索引原样保留。
fn flip_ue_winding(indices: &[u32]) -> Vec<u32> {
    let mut flipped = indices.to_vec();
    for triangle in flipped.as_chunks_mut::<3>().0 {
        triangle.swap(1, 2);
    }
    flipped
}

/// 把 `MeshLod` 转换为场景网格。
pub fn convert_mesh(lod: &MeshLod, name: &str) -> MeshData {
    let positions: Vec<[f32; 3]> = lod
        .positions
        .iter()
        .map(|position| [position[0] as f32, position[1] as f32, position[2] as f32])
        .collect();
    let mut mesh = MeshData::new(name, positions, flip_ue_winding(&lod.indices));
    mesh.normals = lod.attributes.normals.clone();
    mesh.tangents = lod.attributes.tangents.clone();
    mesh.uvs = lod.attributes.uvs.clone();
    mesh.colors = lod.attributes.colors.clone();
    mesh.sections = lod
        .sections
        .iter()
        .map(|section| MeshSection {
            material_index: usize::try_from(section.material_index.max(0)).unwrap_or(0),
            first_index: u32::try_from(section.first_index.max(0)).unwrap_or(0),
            index_count: u32::try_from(section.triangle_count.max(0) * 3).unwrap_or(0),
        })
        .collect();
    mesh
}

/// 场景网格的包围盒（应用节点世界变换前的局部包围盒）。
pub fn mesh_bounds(mesh: &MeshData) -> Bounds {
    mesh.bounds
}

/// 添加已解析材质并返回“槽位 → 材质句柄”映射。
fn add_resolved_materials(
    scene: &mut Scene,
    materials: &[ResolvedMaterial],
) -> Vec<Option<MaterialHandle>> {
    materials
        .iter()
        .map(|material| add_material(scene, material))
        .collect()
}

/// 由查看器图像生成场景贴图。
fn texture_data(
    material_name: &str,
    kind: &str,
    image: &crate::MaterialViewerImage,
) -> TextureData {
    TextureData {
        name: format!("{material_name}_{kind}"),
        width: image.width,
        height: image.height,
        rgba: image.rgba.clone(),
    }
}

/// 从网格材质槽数量生成句柄映射（用于只有槽名没有材质资产的情况）。
pub fn empty_slots(count: usize) -> Vec<Option<MaterialHandle>> {
    vec![None; count]
}

/// 返回网格节点默认的 [`MeshHandle`]（若有）。
pub fn first_mesh(scene: &Scene) -> Option<MeshHandle> {
    scene
        .nodes
        .iter()
        .find_map(|node| node.mesh.filter(|mesh| mesh.0 < scene.meshes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MaterialSlot, MaterialTextureKind, MaterialTextureRef, MeshVertexAttributes};

    /// 构造一个带法线/UV 的三角形 LOD。
    fn test_lod() -> MeshLod {
        MeshLod {
            positions: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 100.0]],
            indices: vec![0, 1, 2],
            sections: vec![crate::MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 1,
            }],
            attributes: MeshVertexAttributes {
                normals: vec![[0.0, 0.0, 1.0]; 3],
                tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
                uvs: vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]],
                colors: Vec::new(),
            },
        }
    }

    /// 已解析材质转换为基础色/贴图。
    fn test_material() -> ResolvedMaterial {
        ResolvedMaterial {
            slot: MaterialSlot {
                name: "M_Test".to_string(),
                package: None,
            },
            package_path: Some("Game/M_Test.uasset".to_string()),
            appearance: Some(MaterialAppearance {
                base_color: [0.5, 0.25, 0.125],
                metallic: 0.2,
                roughness: 0.4,
                textures: vec![MaterialTextureRef {
                    name: "BaseColor".to_string(),
                    package: None,
                    package_id: None,
                    kind: MaterialTextureKind::BaseColor,
                }],
                ..Default::default()
            }),
            viewer: Some(MaterialViewerData {
                appearance: MaterialAppearance::default(),
                base_color: Some(crate::MaterialViewerImage {
                    rgba: vec![255; 4],
                    width: 1,
                    height: 1,
                }),
                normal: None,
                metallic_roughness: None,
                occlusion: None,
            }),
            loaded_assets: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// UE 顺时针正面转换为渲染器的逆时针正面；索引数量与 section 不变。
    #[test]
    fn convert_mesh_flips_ue_winding() {
        let mesh = convert_mesh(&test_lod(), "Tri");
        assert_eq!(mesh.indices, vec![0, 2, 1]);
        assert_eq!(mesh.sections[0].first_index, 0);
        assert_eq!(mesh.sections[0].index_count, 3);

        let mut lod = test_lod();
        lod.indices = vec![0, 1, 2, 3, 4, 5, 6];
        let mesh = convert_mesh(&lod, "Tri");
        assert_eq!(
            mesh.indices,
            vec![0, 2, 1, 3, 5, 4, 6],
            "每个完整三角形翻转绕序，尾部不完整索引保持原样"
        );
    }

    /// 静态网格转换保留几何、材质和 LOD 元数据。
    #[test]
    fn converts_static_mesh_scene() {
        let mesh = StaticMesh {
            name: "SM_Test".to_string(),
            lod: test_lod(),
            lod_count: 1,
            lods: vec![test_lod()],
            material_slots: vec![MaterialSlot {
                name: "M_Test".to_string(),
                package: None,
            }],
            material_imports: Vec::new(),
        };
        let scene = static_mesh_scene("Test", &mesh, &[test_material()]);
        assert_eq!(scene.stats().nodes, 1);
        assert_eq!(scene.stats().meshes, 1);
        assert_eq!(scene.stats().triangles, 1);
        assert_eq!(scene.stats().materials, 1);
        assert!(scene.validate().is_ok());
        assert_eq!(scene.up_axis, Axis::Z);
        let bounds = scene.compute_bounds();
        assert_eq!(bounds.max, [100.0, 100.0, 100.0]);
        let node = &scene.nodes[0];
        assert_eq!(node.kind, NodeKind::StaticMesh);
        assert_eq!(node.materials, vec![Some(MaterialHandle(0))]);
    }

    /// 骨骼网格转换生成骨骼节点与网格节点。
    #[test]
    fn converts_skeletal_mesh_scene() {
        let scene_skeleton = crate::Skeleton {
            name: "SK_Test".to_string(),
            bones: vec![
                crate::Bone {
                    name: "Root".to_string(),
                    parent_index: -1,
                    translation: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                crate::Bone {
                    name: "Child".to_string(),
                    parent_index: 0,
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ],
        };
        let preview = SkeletalMeshPreview {
            skeleton: scene_skeleton.clone(),
            material_slots: 1,
            material_names: vec!["M_Test".to_string()],
            material_imports: Vec::new(),
            lod_count: 1,
            lod_sections: 1,
            required_bones: 2,
            bone_map: vec![0, 1],
            lod_vertices: 3,
            lod_triangles: 1,
            geometry: Some(test_lod()),
            lods: vec![test_lod()],
            lod_skins: Vec::new(),
            skin: None,
        };
        let scene = skeletal_mesh_scene("Test", &preview, &[test_material()]);
        assert_eq!(scene.nodes.len(), 3);
        assert_eq!(scene.stats().meshes, 1);
        assert!(scene.validate().is_ok());
        let mesh_node = scene
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::SkeletalMesh)
            .expect("骨骼网格节点");
        assert_eq!(mesh_node.parent, Some(SceneNodeId(0)));
    }

    /// 骨骼网格写出蒙皮数据，姿态可写入场景。
    #[test]
    fn builds_skin_and_applies_pose() {
        let skeleton = crate::Skeleton {
            name: "SK_Test".to_string(),
            bones: vec![
                crate::Bone {
                    name: "Root".to_string(),
                    parent_index: -1,
                    translation: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                crate::Bone {
                    name: "Child".to_string(),
                    parent_index: 0,
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ],
        };
        let preview = SkeletalMeshPreview {
            skeleton: skeleton.clone(),
            material_slots: 1,
            material_names: vec!["M_Test".to_string()],
            material_imports: Vec::new(),
            lod_count: 1,
            lod_sections: 1,
            required_bones: 2,
            bone_map: vec![0, 1],
            lod_vertices: 3,
            lod_triangles: 1,
            geometry: Some(test_lod()),
            lods: vec![test_lod()],
            lod_skins: Vec::new(),
            skin: Some(crate::SkinWeights {
                vertices: 3,
                max_influences: 1,
                variable: false,
                influences: vec![[0; 12]; 3],
                weights: vec![[65535, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]; 3],
            }),
        };
        let mut scene = skeletal_mesh_scene("Test", &preview, &[test_material()]);
        let skin = scene.meshes[0].skin.as_ref().expect("蒙皮数据");
        assert!(skin.is_valid());
        assert_eq!(skin.joints.len(), 2);
        assert_eq!(skin.joint_weights[0], [1.0, 0.0, 0.0, 0.0]);
        let identity = [
            1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        assert!(apply_skin_pose(&mut scene, &[identity, identity]));
        assert_eq!(
            scene.meshes[0]
                .skin
                .as_ref()
                .unwrap()
                .pose
                .as_ref()
                .unwrap()
                .len(),
            2
        );
        assert!(!apply_skin_pose(&mut scene, &[identity]));
    }

    /// 场景材质继承双面标记，并按金属/粗糙度贴图命名识别 ORM 布局。
    #[test]
    fn scene_material_carries_two_sided_and_orm_layout() {
        let appearance = MaterialAppearance {
            two_sided: true,
            textures: vec![MaterialTextureRef {
                name: "T_Nana_SpecularMask".to_string(),
                package: None,
                package_id: None,
                kind: MaterialTextureKind::MetallicRoughness,
            }],
            ..MaterialAppearance::default()
        };
        let material = scene_material("M_Test", &appearance, None);
        assert!(material.two_sided, "应继承材质的 TwoSided");
        assert_eq!(
            material.orm_layout,
            lumark_scene::OrmLayout::SpecularMask,
            "高光遮罩贴图应识别为 SpecularMask 布局"
        );
        let plain = scene_material("M_Plain", &MaterialAppearance::default(), None);
        assert!(!plain.two_sided);
        assert_eq!(plain.orm_layout, lumark_scene::OrmLayout::AoRoughMetal);
    }
}
