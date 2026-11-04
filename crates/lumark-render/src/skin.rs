//! 线性混合蒙皮（LBS）公共数学。
//!
//! `MeshData.skin` 提供关节索引/权重、inverse bind 矩阵与可选姿态覆盖；
//! 本模块把「关节最终矩阵」与逐顶点混合结果提供给 CPU 光栅与 GPU 上传。

use lumark_scene::{Bounds, MeshData, Scene, SceneNodeId};

/// 4×4 单位矩阵。
pub(crate) const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// 计算渲染取景用的场景包围盒。
///
/// `Scene::compute_bounds` 只用节点世界变换：蒙皮网格的顶点实际由关节矩阵
/// 决定，直接用节点变换会把模型算到错误的位置（查看器取景偏移、近距离裁切）。
/// 这里与光栅化/GPU 上传保持同一套蒙皮结果，并遍历全部顶点，避免抽样在
/// 极端镂空网格上产生包围盒误差。
pub(crate) fn render_bounds(scene: &Scene) -> Bounds {
    let mut bounds = Bounds::EMPTY;
    for (node_index, node) in scene.nodes.iter().enumerate() {
        if !node.visible {
            continue;
        }
        let Some(handle) = node.mesh else {
            continue;
        };
        let Some(mesh) = scene.meshes.get(handle.0) else {
            continue;
        };
        let Some(matrices) = rest_skin_matrices(scene, mesh) else {
            let Some(world) = scene.world_transform(SceneNodeId(node_index)) else {
                continue;
            };
            for position in &mesh.positions {
                bounds.expand_point(world.transform_point(*position));
            }
            continue;
        };
        if mesh.positions.is_empty() {
            continue;
        }
        let skin = mesh.skin.as_ref();
        for (index, position) in mesh.positions.iter().enumerate() {
            let (joints, weights) = skin
                .map(|skin| {
                    (
                        skin.joint_indices.get(index).copied().unwrap_or([0; 4]),
                        skin.joint_weights.get(index).copied().unwrap_or([0.0; 4]),
                    )
                })
                .unwrap_or(([0; 4], [0.0; 4]));
            let (skinned, _, _) = skin_vertex(
                &matrices,
                joints,
                weights,
                *position,
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
            );
            bounds.expand_point(skinned);
        }
    }
    bounds
}

/// 返回网格的关节最终矩阵；静态网格或数据不完整时返回 `None`。
pub(crate) fn skin_matrices(scene: &Scene, mesh: &MeshData) -> Option<Vec<[[f32; 4]; 4]>> {
    let skin = mesh.skin.as_ref()?;
    if !skin.is_valid() {
        return None;
    }
    if let Some(pose) = &skin.pose {
        if pose.len() != skin.joints.len() {
            return None;
        }
        return Some(pose.iter().map(unflatten).collect());
    }
    rest_skin_matrices(scene, mesh)
}

/// 返回网格休止姿态的关节矩阵（忽略动画姿态覆盖）。
///
/// 取景包围盒使用休止姿态，动画播放时相机不会跟着角色移动。
fn rest_skin_matrices(scene: &Scene, mesh: &MeshData) -> Option<Vec<[[f32; 4]; 4]>> {
    let skin = mesh.skin.as_ref()?;
    if !skin.is_valid() {
        return None;
    }
    let mut matrices = Vec::with_capacity(skin.joints.len());
    for (joint, inverse) in skin.joints.iter().zip(skin.inverse_bind_matrices.iter()) {
        let world = scene.world_transform(*joint)?.matrix();
        matrices.push(multiply(&world, &unflatten(inverse)));
    }
    Some(matrices)
}

/// 逐顶点混合并变换位置/法线/切线。
pub(crate) fn skin_vertex(
    matrices: &[[[f32; 4]; 4]],
    joints: [u16; 4],
    weights: [f32; 4],
    position: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
) -> ([f32; 3], [f32; 3], [f32; 4]) {
    let total: f32 = weights.iter().map(|weight| weight.max(0.0)).sum();
    if total <= f32::EPSILON {
        return (position, normal, tangent);
    }
    let mut blended = [[0.0f32; 4]; 4];
    for influence in 0..4 {
        let Some(matrix) = matrices.get(joints[influence] as usize) else {
            continue;
        };
        let weight = weights[influence].max(0.0) / total;
        for column in 0..4 {
            for row in 0..4 {
                blended[column][row] += matrix[column][row] * weight;
            }
        }
    }
    let position = transform_point(&blended, position);
    let normal = normalize(transform_direction(&blended, normal));
    let direction = normalize(transform_direction(
        &blended,
        [tangent[0], tangent[1], tangent[2]],
    ));
    (
        position,
        normal,
        [direction[0], direction[1], direction[2], tangent[3]],
    )
}

/// 列主序 4×4 矩阵展平。
pub(crate) fn flatten(matrix: &[[f32; 4]; 4]) -> [f32; 16] {
    let mut flat = [0.0f32; 16];
    for column in 0..4 {
        flat[column * 4..column * 4 + 4].copy_from_slice(&matrix[column]);
    }
    flat
}

/// 展平数组还原矩阵。
pub(crate) fn unflatten(flat: &[f32; 16]) -> [[f32; 4]; 4] {
    let mut matrix = [[0.0f32; 4]; 4];
    for column in 0..4 {
        matrix[column].copy_from_slice(&flat[column * 4..column * 4 + 4]);
    }
    matrix
}

/// 列主序乘法。
pub(crate) fn multiply(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0f32; 4]; 4];
    for (column, target) in result.iter_mut().enumerate() {
        for (row, value) in target.iter_mut().enumerate() {
            *value = (0..4).map(|index| a[index][row] * b[column][index]).sum();
        }
    }
    result
}

/// 矩阵变换点。
fn transform_point(matrix: &[[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    let vector = [point[0], point[1], point[2], 1.0];
    let x = matrix[0][0] * vector[0]
        + matrix[1][0] * vector[1]
        + matrix[2][0] * vector[2]
        + matrix[3][0];
    let y = matrix[0][1] * vector[0]
        + matrix[1][1] * vector[1]
        + matrix[2][1] * vector[2]
        + matrix[3][1];
    let z = matrix[0][2] * vector[0]
        + matrix[1][2] * vector[1]
        + matrix[2][2] * vector[2]
        + matrix[3][2];
    [x, y, z]
}

/// 矩阵变换方向（忽略平移）。
fn transform_direction(matrix: &[[f32; 4]; 4], direction: [f32; 3]) -> [f32; 3] {
    [
        matrix[0][0] * direction[0] + matrix[1][0] * direction[1] + matrix[2][0] * direction[2],
        matrix[0][1] * direction[0] + matrix[1][1] * direction[1] + matrix[2][1] * direction[2],
        matrix[0][2] * direction[0] + matrix[1][2] * direction[1] + matrix[2][2] * direction[2],
    ]
}

/// 归一化；零向量回退 +Z。
fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, value[1] / length, value[2] / length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单位矩阵蒙皮保持顶点不变。
    #[test]
    fn identity_skin_keeps_vertex() {
        let (position, normal, tangent) = skin_vertex(
            &[IDENTITY],
            [0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!((position[0] - 1.0).abs() < 1e-6);
        assert!((position[1] - 2.0).abs() < 1e-6);
        assert!((position[2] - 3.0).abs() < 1e-6);
        assert!((normal[1] - 1.0).abs() < 1e-6);
        assert!((tangent[0] - 1.0).abs() < 1e-6);
    }

    /// 平移矩阵蒙皮移动顶点与法线方向保持不变。
    #[test]
    fn translated_skin_moves_position() {
        let matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 20.0, 30.0, 1.0],
        ];
        let (position, normal, _) = skin_vertex(
            &[matrix],
            [0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!((position[0] - 11.0).abs() < 1e-6);
        assert!((position[1] - 22.0).abs() < 1e-6);
        assert!((position[2] - 33.0).abs() < 1e-6);
        assert!((normal[2] - 1.0).abs() < 1e-6);
    }

    /// 两个关节各 50% 权重取矩阵平均。
    #[test]
    fn blends_two_joint_weights() {
        let a = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut b = a;
        b[3] = [10.0, 0.0, 0.0, 1.0];
        let (position, _, _) = skin_vertex(
            &[a, b],
            [0, 1, 0, 0],
            [0.5, 0.5, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!((position[0] - 5.0).abs() < 1e-6);
    }

    /// 蒙皮包围盒按蒙皮后的顶点统计，而不是节点世界变换。
    #[test]
    fn render_bounds_follow_skinned_vertices() {
        use lumark_scene::{MeshSection, NodeKind, SceneNode, Transform};
        let mut scene = Scene::new("SkinBounds");
        let root = scene.add_node(SceneNode {
            name: "Root".to_string(),
            parent: None,
            transform: Transform::from_translation([0.0, 0.0, 100.0]),
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Other("Bone".to_string()),
            metadata: Vec::new(),
        });
        let mut mesh = MeshData::new(
            "SkinTri",
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            vec![0, 1, 2],
        );
        mesh.sections = vec![MeshSection {
            material_index: 0,
            first_index: 0,
            index_count: 3,
        }];
        // inverse bind 抵消根骨骼的 +100 平移，休止姿态下顶点保持 bind 坐标。
        mesh.skin = Some(lumark_scene::MeshSkin {
            joints: vec![root],
            joint_indices: vec![[0; 4]; 3],
            joint_weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            inverse_bind_matrices: vec![[
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -100.0, 1.0,
            ]],
            pose: None,
        });
        let handle = scene.add_mesh(mesh);
        scene.add_node(SceneNode {
            name: "SkinTri".to_string(),
            parent: Some(root),
            transform: Transform::IDENTITY,
            mesh: Some(handle),
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::SkeletalMesh,
            metadata: Vec::new(),
        });
        let naive = scene.compute_bounds();
        assert!(
            (naive.min[2] - 100.0).abs() < 1e-3,
            "节点变换把网格算到 +100"
        );
        let skinned = render_bounds(&scene);
        assert!((skinned.min[2]).abs() < 1e-3, "蒙皮后应在 bind 坐标");
        assert!((skinned.max[0] - 10.0).abs() < 1e-3);
        assert!((skinned.max[1] - 10.0).abs() < 1e-3);
    }
}
