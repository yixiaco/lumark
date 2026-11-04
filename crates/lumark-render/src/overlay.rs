//! 叠加层几何：地面网格与骨骼线段（世界空间线段列表）。
//!
//! CPU 光栅与 GPU 线管线共用同一几何生成，保证两种后端叠加层一致。

use lumark_scene::Scene;

/// 一条世界空间线段。
pub(crate) type LineSegment = ([f32; 3], [f32; 3]);

/// 生成地面网格线段（Z=0 平面，间距按场景尺度自适应）。
pub(crate) fn grid_lines(scene: &Scene) -> Vec<LineSegment> {
    let bounds = crate::render_bounds(scene);
    let extent = if bounds.is_empty() {
        100.0
    } else {
        bounds.extent().max(1.0)
    };
    let step = 10f32.powf((extent / 10.0).log10().floor()).max(1.0);
    let half = step * 10.0;
    let mut lines = Vec::with_capacity(42);
    for index in -10..=10 {
        let offset = index as f32 * step;
        lines.push(([-half, offset, 0.0], [half, offset, 0.0]));
        lines.push(([offset, -half, 0.0], [offset, half, 0.0]));
    }
    lines
}

/// 生成骨骼线段：关节世界位置 → 父关节世界位置。
pub(crate) fn bone_lines(scene: &Scene) -> Vec<LineSegment> {
    let mut lines = Vec::new();
    for mesh in &scene.meshes {
        let Some(skin) = mesh.skin.as_ref() else {
            continue;
        };
        if !skin.is_valid() {
            continue;
        }
        let positions: Vec<[f32; 3]> = skin
            .joints
            .iter()
            .enumerate()
            .map(|(index, joint)| joint_world_position(scene, skin, index, *joint))
            .collect();
        for (index, joint) in skin.joints.iter().enumerate() {
            let Some(parent) = scene.node(*joint).and_then(|node| node.parent) else {
                continue;
            };
            let Some(parent_index) = skin
                .joints
                .iter()
                .position(|candidate| *candidate == parent)
            else {
                continue;
            };
            lines.push((positions[parent_index], positions[index]));
        }
    }
    lines
}

/// 生成线框三角形边（世界空间），跳过隐藏 section/材质槽。
pub(crate) fn wireframe_lines(
    scene: &Scene,
    hidden_material_slots: &[usize],
    hidden_sections: &[usize],
) -> Vec<LineSegment> {
    let mut lines = Vec::new();
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
        let Some(world) = scene.world_transform(lumark_scene::SceneNodeId(node_index)) else {
            continue;
        };
        let positions: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|position| world.transform_point(*position))
            .collect();
        let ranges: Vec<(usize, usize, usize)> = if mesh.sections.is_empty() {
            vec![(0, mesh.indices.len(), 0)]
        } else {
            mesh.sections
                .iter()
                .map(|section| {
                    (
                        section.first_index as usize,
                        section.index_count as usize,
                        section.material_index,
                    )
                })
                .collect()
        };
        for (section_index, (first, count, material_index)) in ranges.into_iter().enumerate() {
            if hidden_material_slots.contains(&material_index)
                || hidden_sections.contains(&section_index)
            {
                continue;
            }
            let end = (first + count).min(mesh.indices.len());
            let mut index = first;
            while index + 3 <= end {
                let a = mesh.indices[index] as usize;
                let b = mesh.indices[index + 1] as usize;
                let c = mesh.indices[index + 2] as usize;
                if let (Some(a), Some(b), Some(c)) =
                    (positions.get(a), positions.get(b), positions.get(c))
                {
                    lines.push((*a, *b));
                    lines.push((*b, *c));
                    lines.push((*c, *a));
                }
                index += 3;
            }
        }
    }
    lines
}

/// 关节世界位置：有姿态时用姿态矩阵变换 bind 位置。
fn joint_world_position(
    scene: &Scene,
    skin: &lumark_scene::MeshSkin,
    index: usize,
    joint: lumark_scene::SceneNodeId,
) -> [f32; 3] {
    let bind = scene
        .world_transform(joint)
        .map(|transform| transform.translation)
        .unwrap_or([0.0; 3]);
    let Some(pose) = skin.pose.as_ref().and_then(|pose| pose.get(index)) else {
        return bind;
    };
    [
        pose[0] * bind[0] + pose[4] * bind[1] + pose[8] * bind[2] + pose[12],
        pose[1] * bind[0] + pose[5] * bind[1] + pose[9] * bind[2] + pose[13],
        pose[2] * bind[0] + pose[6] * bind[1] + pose[10] * bind[2] + pose[14],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_scene::{MeshData, MeshSection, NodeKind, SceneNode, Transform};

    /// 空场景网格仍生成 42 条线段。
    #[test]
    fn grid_has_lines_for_empty_scene() {
        let scene = Scene::new("Empty");
        assert_eq!(grid_lines(&scene).len(), 42);
    }

    /// 线框为每个三角形生成 3 条边。
    #[test]
    fn wireframe_lines_cover_triangle_edges() {
        let mut scene = Scene::new("Wire");
        let mesh = scene.add_mesh(MeshData::new(
            "Tri",
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            vec![0, 1, 2],
        ));
        scene.add_node(SceneNode {
            name: "Tri".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh),
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        assert_eq!(wireframe_lines(&scene, &[], &[]).len(), 3);
        assert!(wireframe_lines(&scene, &[0], &[]).is_empty());
    }

    /// 骨骼线连接父子关节。
    #[test]
    fn bone_lines_follow_joint_hierarchy() {
        let mut scene = Scene::new("Skin");
        let mut mesh = MeshData::new("Tri", vec![[0.0; 3]; 3], vec![0, 1, 2]);
        mesh.sections = vec![MeshSection {
            material_index: 0,
            first_index: 0,
            index_count: 3,
        }];
        mesh.skin = Some(lumark_scene::MeshSkin {
            joints: vec![lumark_scene::SceneNodeId(0), lumark_scene::SceneNodeId(1)],
            joint_indices: vec![[0; 4]; 3],
            joint_weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            inverse_bind_matrices: vec![[0.0; 16]; 2],
            pose: None,
        });
        let mesh_handle = scene.add_mesh(mesh);
        scene.add_node(SceneNode {
            name: "Root".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Other("Bone".to_string()),
            metadata: Vec::new(),
        });
        scene.add_node(SceneNode {
            name: "Child".to_string(),
            parent: Some(lumark_scene::SceneNodeId(0)),
            transform: Transform {
                translation: [0.0, 0.0, 10.0],
                ..Transform::IDENTITY
            },
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Other("Bone".to_string()),
            metadata: Vec::new(),
        });
        scene.add_node(SceneNode {
            name: "Mesh".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh_handle),
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::SkeletalMesh,
            metadata: Vec::new(),
        });
        let lines = bone_lines(&scene);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, [0.0, 0.0, 0.0]);
        assert_eq!(lines[0].1, [0.0, 0.0, 10.0]);
    }
}
