//! SkeletalMesh 参考姿态的线性混合蒙皮。
//!
//! 本模块不依赖动画解码：调用方给出骨骼局部旋转增量，即可用已解析的
//! `SkinWeights` 计算变形后的网格，用于验证蒙皮权重和后续动画播放。

use crate::{AnimSequenceData, MeshLod, MeshVertexAttributes, Skeleton, SkinWeights};

/// 一次骨骼局部旋转增量。
#[derive(Debug, Clone, Copy)]
pub struct BoneRotation {
    /// 骨骼索引。
    pub bone: usize,
    /// 局部空间旋转增量（四元数 xyzw）。
    pub rotation: [f64; 4],
}

/// 每根骨骼的局部姿态。
#[derive(Debug, Clone, Copy)]
pub struct BonePose {
    /// 局部平移。
    pub translation: [f64; 3],
    /// 局部旋转（四元数 xyzw）。
    pub rotation: [f64; 4],
    /// 局部缩放（当前蒙皮忽略缩放）。
    pub scale: [f64; 3],
}

/// 对一个 SkeletalMesh 应用骨骼局部旋转并返回变形网格。
pub fn skin_mesh(
    mesh: &MeshLod,
    skeleton: &Skeleton,
    weights: &SkinWeights,
    rotations: &[BoneRotation],
) -> MeshLod {
    let reference = world_transforms(skeleton, &[]);
    let pose = world_transforms(skeleton, rotations);
    apply_skin(mesh, &reference, &pose, weights)
}

/// 按每根骨骼的完整局部姿态执行 LBS 蒙皮。
pub fn skin_mesh_pose(
    mesh: &MeshLod,
    skeleton: &Skeleton,
    weights: &SkinWeights,
    poses: &[BonePose],
) -> MeshLod {
    let reference = world_transforms(skeleton, &[]);
    let pose = pose_world_transforms(skeleton, poses);
    apply_skin(mesh, &reference, &pose, weights)
}

/// 按秒采样动画并执行 CPU LBS 蒙皮；供 CPU 预览与导出校验使用。
pub fn skin_mesh_animation(
    mesh: &MeshLod,
    skeleton: &Skeleton,
    weights: &SkinWeights,
    animation: &AnimSequenceData,
    time_seconds: f32,
) -> MeshLod {
    let sample = sample_animation(skeleton, animation, time_seconds);
    skin_mesh_pose(mesh, skeleton, weights, &sample.poses)
}

/// 动画在某一时刻的采样结果。
#[derive(Debug, Clone)]
pub struct AnimationSample {
    /// 采样时间（秒，已钳制到 `[0, length]`）。
    pub time: f32,
    /// 相对时间比例（0–1）。
    pub ratio: f32,
    /// 每根骨骼的局部姿态（顺序与骨架一致）。
    pub poses: Vec<BonePose>,
    /// 每根骨骼的蒙皮矩阵（世界姿态 × 参考姿态逆，列主序 4×4 f32）。
    pub skin_matrices: Vec<[f32; 16]>,
}

/// 按秒采样动画；时间会自动钳制到 `[0, length]`。
pub fn sample_animation(
    skeleton: &Skeleton,
    animation: &AnimSequenceData,
    time_seconds: f32,
) -> AnimationSample {
    let duration = animation.length.max(0.0);
    let (time, ratio) = if duration > f32::EPSILON {
        let time = time_seconds.clamp(0.0, duration);
        (time, (time / duration).clamp(0.0, 1.0))
    } else {
        (0.0, 0.0)
    };
    let poses = animation_poses(skeleton, animation, ratio);
    let skin_matrices = skin_matrices_from_poses(skeleton, &poses);
    AnimationSample {
        time,
        ratio,
        poses,
        skin_matrices,
    }
}

/// 由局部姿态计算每根骨骼的蒙皮矩阵（世界姿态 × 参考姿态逆）。
///
/// 返回列主序 4×4 f32 矩阵，可直接上传 GPU 蒙皮或写入 glTF 的关节矩阵。
pub fn skin_matrices_from_poses(skeleton: &Skeleton, poses: &[BonePose]) -> Vec<[f32; 16]> {
    let reference = world_transforms(skeleton, &[]);
    let pose = pose_world_transforms(skeleton, poses);
    pose.iter()
        .zip(reference.iter())
        .map(|(pose, reference)| {
            let skin = multiply(pose, &reference.inverse_rigid());
            skin.0.map(|value| value as f32)
        })
        .collect()
}

/// 按每根骨骼的局部姿态构造世界变换。
fn pose_world_transforms(skeleton: &Skeleton, poses: &[BonePose]) -> Vec<Mat4> {
    let mut transforms = Vec::with_capacity(skeleton.bones.len());
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let local_pose = poses.get(index).copied().unwrap_or(BonePose {
            translation: bone.translation,
            rotation: bone.rotation,
            scale: bone.scale,
        });
        let local = Mat4::from_quat_translation(
            normalize_quat(local_pose.rotation),
            local_pose.translation,
        );
        let parent = usize::try_from(bone.parent_index).ok();
        let transform = if let Some(parent) = parent.and_then(|parent| transforms.get(parent)) {
            multiply(parent, &local)
        } else {
            local
        };
        transforms.push(transform);
    }
    transforms
}

/// 使用参考/当前世界变换执行蒙皮。
fn apply_skin(mesh: &MeshLod, reference: &[Mat4], pose: &[Mat4], weights: &SkinWeights) -> MeshLod {
    let vertex_count = mesh.positions.len();
    let has_normals =
        !mesh.attributes.normals.is_empty() && mesh.attributes.normals.len() == vertex_count;
    let has_tangents =
        !mesh.attributes.tangents.is_empty() && mesh.attributes.tangents.len() == vertex_count;
    let mut positioned = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(if has_normals { vertex_count } else { 0 });
    let mut tangents = Vec::with_capacity(if has_tangents { vertex_count } else { 0 });
    for (vertex, position) in mesh.positions.iter().enumerate() {
        let (influences, vertex_weights) =
            match (weights.influences.get(vertex), weights.weights.get(vertex)) {
                (Some(influences), Some(weights)) => (influences, weights),
                _ => {
                    positioned.push(*position);
                    if has_normals {
                        normals.push(mesh.attributes.normals[vertex]);
                    }
                    if has_tangents {
                        tangents.push(mesh.attributes.tangents[vertex]);
                    }
                    continue;
                }
            };
        let total: f64 = vertex_weights.iter().map(|weight| f64::from(*weight)).sum();
        if total <= 0.0 {
            positioned.push(*position);
            if has_normals {
                normals.push(mesh.attributes.normals[vertex]);
            }
            if has_tangents {
                tangents.push(mesh.attributes.tangents[vertex]);
            }
            continue;
        }
        let mut result = [0.0f64; 3];
        let mut normal_sum = [0.0f64; 3];
        let mut tangent_sum = [0.0f64; 3];
        for (index, weight) in vertex_weights.iter().enumerate() {
            let weight = f64::from(*weight);
            if weight <= 0.0 {
                continue;
            }
            let Some(bone) = influences.get(index).copied().map(usize::from) else {
                continue;
            };
            let (Some(reference), Some(pose)) = (reference.get(bone), pose.get(bone)) else {
                continue;
            };
            let skin = multiply(pose, &reference.inverse_rigid());
            let transformed = transform_point(&skin, *position);
            for axis in 0..3 {
                result[axis] += transformed[axis] * weight;
            }
            if has_normals {
                let normal = mesh.attributes.normals[vertex];
                let rotated = transform_direction(
                    &skin,
                    [normal[0] as f64, normal[1] as f64, normal[2] as f64],
                );
                for axis in 0..3 {
                    normal_sum[axis] += rotated[axis] * weight;
                }
            }
            if has_tangents {
                let tangent = mesh.attributes.tangents[vertex];
                let rotated = transform_direction(
                    &skin,
                    [tangent[0] as f64, tangent[1] as f64, tangent[2] as f64],
                );
                for axis in 0..3 {
                    tangent_sum[axis] += rotated[axis] * weight;
                }
            }
        }
        positioned.push([result[0] / total, result[1] / total, result[2] / total]);
        if has_normals {
            normals.push(normalize_direction_f32(normal_sum));
        }
        if has_tangents {
            let direction = normalize_direction_f32(tangent_sum);
            tangents.push([
                direction[0],
                direction[1],
                direction[2],
                mesh.attributes.tangents[vertex][3],
            ]);
        }
    }
    MeshLod {
        positions: positioned,
        indices: mesh.indices.clone(),
        sections: mesh.sections.clone(),
        attributes: MeshVertexAttributes {
            normals,
            tangents,
            uvs: mesh.attributes.uvs.clone(),
            colors: mesh.attributes.colors.clone(),
        },
    }
}

/// 用矩阵的 3×3 旋转部分变换方向向量。
fn transform_direction(matrix: &Mat4, direction: [f64; 3]) -> [f64; 3] {
    [
        matrix.0[0] * direction[0] + matrix.0[4] * direction[1] + matrix.0[8] * direction[2],
        matrix.0[1] * direction[0] + matrix.0[5] * direction[1] + matrix.0[9] * direction[2],
        matrix.0[2] * direction[0] + matrix.0[6] * direction[1] + matrix.0[10] * direction[2],
    ]
}

/// 归一化方向向量；零向量回退到 +Z。
fn normalize_direction_f32(value: [f64; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length <= f64::EPSILON {
        return [0.0, 0.0, 1.0];
    }
    [
        (value[0] / length) as f32,
        (value[1] / length) as f32,
        (value[2] / length) as f32,
    ]
}

/// 按动画轨道采样生成每根骨骼的局部姿态。
pub fn animation_poses(
    skeleton: &Skeleton,
    animation: &AnimSequenceData,
    ratio: f32,
) -> Vec<BonePose> {
    let tracks = animation.sample(ratio);
    let mut poses: Vec<BonePose> = skeleton
        .bones
        .iter()
        .map(|bone| BonePose {
            translation: bone.translation,
            rotation: bone.rotation,
            scale: bone.scale,
        })
        .collect();
    for (track, pose) in tracks.iter().enumerate() {
        let Some(bone) = animation
            .track_to_bone
            .get(track)
            .and_then(|bone| usize::try_from(*bone).ok())
        else {
            continue;
        };
        let Some(target) = poses.get_mut(bone) else {
            continue;
        };
        *target = BonePose {
            translation: [
                pose.translation[0] as f64,
                pose.translation[1] as f64,
                pose.translation[2] as f64,
            ],
            rotation: [
                pose.rotation[0] as f64,
                pose.rotation[1] as f64,
                pose.rotation[2] as f64,
                pose.rotation[3] as f64,
            ],
            scale: [
                pose.scale[0] as f64,
                pose.scale[1] as f64,
                pose.scale[2] as f64,
            ],
        };
    }
    poses
}

/// 选择蒙皮权重总和最大的非根骨骼，返回索引和名称。
pub fn dominant_skinned_bone(
    skeleton: &Skeleton,
    weights: &SkinWeights,
) -> Option<(usize, String)> {
    let mut totals = vec![0u64; skeleton.bones.len()];
    for (vertex, influences) in weights.influences.iter().enumerate() {
        let Some(vertex_weights) = weights.weights.get(vertex) else {
            continue;
        };
        for (index, weight) in vertex_weights.iter().enumerate() {
            let Some(bone) = influences.get(index).copied().map(usize::from) else {
                continue;
            };
            if let Some(total) = totals.get_mut(bone) {
                *total += u64::from(*weight);
            }
        }
    }
    skeleton
        .bones
        .iter()
        .enumerate()
        .filter(|(index, bone)| bone.parent_index >= 0 && *index < totals.len())
        .max_by_key(|(index, _)| totals[*index])
        .map(|(index, bone)| (index, bone.name.clone()))
}

/// 计算骨骼世界变换（忽略缩放，与查看器的参考姿态一致）。
fn world_transforms(skeleton: &Skeleton, rotations: &[BoneRotation]) -> Vec<Mat4> {
    let mut transforms = Vec::with_capacity(skeleton.bones.len());
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let mut local = normalize_quat(bone.rotation);
        if let Some(delta) = rotations.iter().find(|rotation| rotation.bone == index) {
            local = quat_mul(normalize_quat(delta.rotation), local);
        }
        let local = Mat4::from_quat_translation(local, bone.translation);
        let parent = usize::try_from(bone.parent_index).ok();
        let transform = if let Some(parent) = parent.and_then(|parent| transforms.get(parent)) {
            multiply(parent, &local)
        } else {
            local
        };
        transforms.push(transform);
    }
    transforms
}

/// 4×4 列主序矩阵。
#[derive(Debug, Clone, Copy)]
struct Mat4([f64; 16]);

impl Mat4 {
    /// 由四元数和平移构造变换矩阵。
    fn from_quat_translation(quat: [f64; 4], translation: [f64; 3]) -> Self {
        let [x, y, z, w] = normalize_quat(quat);
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        let wx = w * x;
        let wy = w * y;
        let wz = w * z;
        Self([
            1.0 - 2.0 * (yy + zz),
            2.0 * (xy + wz),
            2.0 * (xz - wy),
            0.0,
            2.0 * (xy - wz),
            1.0 - 2.0 * (xx + zz),
            2.0 * (yz + wx),
            0.0,
            2.0 * (xz + wy),
            2.0 * (yz - wx),
            1.0 - 2.0 * (xx + yy),
            0.0,
            translation[0],
            translation[1],
            translation[2],
            1.0,
        ])
    }

    /// 刚体逆变换：旋转转置 + 平移反向。
    fn inverse_rigid(&self) -> Self {
        let r = &self.0;
        let inverse = [
            r[0], r[4], r[8], 0.0, r[1], r[5], r[9], 0.0, r[2], r[6], r[10], 0.0, 0.0, 0.0, 0.0,
            1.0,
        ];
        let t = [r[12], r[13], r[14]];
        let tx = -(inverse[0] * t[0] + inverse[4] * t[1] + inverse[8] * t[2]);
        let ty = -(inverse[1] * t[0] + inverse[5] * t[1] + inverse[9] * t[2]);
        let tz = -(inverse[2] * t[0] + inverse[6] * t[1] + inverse[10] * t[2]);
        let mut result = inverse;
        result[12] = tx;
        result[13] = ty;
        result[14] = tz;
        Self(result)
    }
}

/// 矩阵乘法。
fn multiply(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut result = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            let mut value = 0.0;
            for index in 0..4 {
                value += a.0[index * 4 + row] * b.0[column * 4 + index];
            }
            result[column * 4 + row] = value;
        }
    }
    Mat4(result)
}

/// 变换点。
fn transform_point(matrix: &Mat4, point: [f64; 3]) -> [f64; 3] {
    [
        matrix.0[0] * point[0] + matrix.0[4] * point[1] + matrix.0[8] * point[2] + matrix.0[12],
        matrix.0[1] * point[0] + matrix.0[5] * point[1] + matrix.0[9] * point[2] + matrix.0[13],
        matrix.0[2] * point[0] + matrix.0[6] * point[1] + matrix.0[10] * point[2] + matrix.0[14],
    ]
}

/// 四元数乘法。
fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// 归一化四元数。
fn normalize_quat(mut quat: [f64; 4]) -> [f64; 4] {
    let length = quat.iter().map(|value| value * value).sum::<f64>().sqrt();
    if length <= f64::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    for value in &mut quat {
        *value /= length;
    }
    quat
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bone, MeshSection};

    /// 根骨骼旋转 90° 会带动完全绑定到它的顶点。
    #[test]
    fn skins_vertex_with_root_rotation() {
        let mesh = MeshLod {
            positions: vec![[1.0, 0.0, 0.0]],
            indices: vec![0, 0, 0],
            sections: vec![MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 0,
            }],
            attributes: Default::default(),
        };
        let skeleton = Skeleton {
            name: "SK_Test".to_string(),
            bones: vec![Bone {
                name: "Root".to_string(),
                parent_index: -1,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            }],
        };
        let mut influences = [[0u16; 12]; 1];
        influences[0][0] = 0;
        let mut weights = [[0u16; 12]; 1];
        weights[0][0] = 255;
        let skin = SkinWeights {
            vertices: 1,
            max_influences: 1,
            variable: false,
            influences: influences.to_vec(),
            weights: weights.to_vec(),
        };
        let half = std::f64::consts::FRAC_PI_4;
        let rotation = BoneRotation {
            bone: 0,
            rotation: [0.0, 0.0, half.sin(), half.cos()],
        };
        let posed = skin_mesh(&mesh, &skeleton, &skin, &[rotation]);
        assert!((posed.positions[0][0]).abs() < 1.0e-6);
        assert!((posed.positions[0][1] - 1.0).abs() < 1.0e-6);
    }

    /// 参考局部姿态作为完整 pose 输入时不应改变顶点。
    #[test]
    fn identity_pose_matches_rest() {
        let mesh = MeshLod {
            positions: vec![[1.0, 2.0, 3.0]],
            indices: Vec::new(),
            sections: Vec::new(),
            attributes: Default::default(),
        };
        let skeleton = Skeleton {
            name: "SK_Test".to_string(),
            bones: vec![Bone {
                name: "Root".to_string(),
                parent_index: -1,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            }],
        };
        let mut influences = [[0u16; 12]; 1];
        influences[0][0] = 0;
        let mut weights = [[0u16; 12]; 1];
        weights[0][0] = 255;
        let skin = SkinWeights {
            vertices: 1,
            max_influences: 1,
            variable: false,
            influences: influences.to_vec(),
            weights: weights.to_vec(),
        };
        let poses = [BonePose {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }];
        let posed = skin_mesh_pose(&mesh, &skeleton, &skin, &poses);
        assert_eq!(posed.positions[0], [1.0, 2.0, 3.0]);
    }

    /// 没有权重的顶点保持原位。
    #[test]
    fn keeps_unweighted_vertices() {
        let mesh = MeshLod {
            positions: vec![[3.0, 4.0, 5.0]],
            indices: Vec::new(),
            sections: Vec::new(),
            attributes: Default::default(),
        };
        let skeleton = Skeleton {
            name: "SK_Empty".to_string(),
            bones: Vec::new(),
        };
        let skin = SkinWeights {
            vertices: 0,
            max_influences: 0,
            variable: false,
            influences: Vec::new(),
            weights: Vec::new(),
        };
        let posed = skin_mesh(&mesh, &skeleton, &skin, &[]);
        assert_eq!(posed.positions[0], [3.0, 4.0, 5.0]);
    }

    /// 按秒采样会钳制到动画时长，并生成与骨骼数一致的姿态/矩阵。
    #[test]
    fn samples_animation_with_clamped_time() {
        let skeleton = Skeleton {
            name: "SK_Test".to_string(),
            bones: vec![
                Bone {
                    name: "Root".to_string(),
                    parent_index: -1,
                    translation: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                Bone {
                    name: "Child".to_string(),
                    parent_index: 0,
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ],
        };
        let animation = AnimSequenceData::empty_for_test(vec![0, 1], 2.0, 24);
        let start = sample_animation(&skeleton, &animation, -1.0);
        assert_eq!(start.time, 0.0);
        assert_eq!(start.ratio, 0.0);
        assert_eq!(start.poses.len(), 2);
        assert_eq!(start.skin_matrices.len(), 2);
        let end = sample_animation(&skeleton, &animation, 99.0);
        assert_eq!(end.time, 2.0);
        assert_eq!(end.ratio, 1.0);
    }

    /// 蒙皮时法线与切线会跟随骨骼旋转并重新归一化。
    #[test]
    fn skins_normals_and_tangents_with_rotation() {
        let mesh = MeshLod {
            positions: vec![[1.0, 0.0, 0.0]],
            indices: vec![0, 0, 0],
            sections: Vec::new(),
            attributes: MeshVertexAttributes {
                normals: vec![[1.0, 0.0, 0.0]],
                tangents: vec![[1.0, 0.0, 0.0, 1.0]],
                uvs: Vec::new(),
                colors: Vec::new(),
            },
        };
        let skeleton = Skeleton {
            name: "SK_Test".to_string(),
            bones: vec![Bone {
                name: "Root".to_string(),
                parent_index: -1,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            }],
        };
        let mut influences = [[0u16; 12]; 1];
        influences[0][0] = 0;
        let mut weights = [[0u16; 12]; 1];
        weights[0][0] = 255;
        let skin = SkinWeights {
            vertices: 1,
            max_influences: 1,
            variable: false,
            influences: influences.to_vec(),
            weights: weights.to_vec(),
        };
        let half = std::f64::consts::FRAC_PI_4;
        let rotation = BoneRotation {
            bone: 0,
            rotation: [0.0, 0.0, half.sin(), half.cos()],
        };
        let posed = skin_mesh(&mesh, &skeleton, &skin, &[rotation]);
        assert!((posed.positions[0][0]).abs() < 1e-6);
        assert!((posed.positions[0][1] - 1.0).abs() < 1e-6);
        assert!((posed.attributes.normals[0][0]).abs() < 1e-6);
        assert!((posed.attributes.normals[0][1] - 1.0).abs() < 1e-6);
        assert!((posed.attributes.tangents[0][0]).abs() < 1e-6);
        assert!((posed.attributes.tangents[0][1] - 1.0).abs() < 1e-6);
        assert_eq!(posed.attributes.tangents[0][3], 1.0);
    }
}
