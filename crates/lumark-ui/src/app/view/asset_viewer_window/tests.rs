//! 查看器窗口的单元测试。

use super::*;
use lumark_unreal::MeshSection;

/// 窗口初始化时生成可读取的预览图片
#[test]
fn viewer_renders_initial_preview_file() {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
    };
    let view =
        AssetViewerWindowView::new("Mesh".to_string(), ViewerScene::Mesh { lods: vec![mesh] });
    assert!(view.error.is_none());
    assert_eq!(view.hidden_sections.len(), 1);
    assert!(view.image_path.is_file());
    assert!(
        std::fs::metadata(&view.image_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            > 0
    );
}

/// ?LOD StaticMesh 可切?LOD 并重绘
#[test]
fn viewer_switches_static_lods() {
    let lod0 = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
    };
    let lod1 = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
    };
    let mut view = AssetViewerWindowView::new(
        "Mesh".to_string(),
        ViewerScene::Mesh {
            lods: vec![lod0, lod1],
        },
    );
    assert_eq!(view.static_lods().map_or(0, <[MeshLod]>::len), 2);
    view.lod_index = 1;
    view.render_preview();
    assert!(view.error.is_none());
    assert!(view.image_path.is_file());
}

/// ?LOD SkeletalMesh 也可切换 LOD 并重绘
#[test]
fn viewer_switches_skeletal_lods() {
    use lumark_unreal::Bone;

    let lod0 = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
    };
    let lod1 = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: Vec::new(),
    };
    let skeleton = Skeleton {
        name: "SKEL_Test".to_string(),
        bones: vec![Bone {
            name: "Root".to_string(),
            parent_index: -1,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }],
    };
    let mut view = AssetViewerWindowView::new(
        "SK_Test".to_string(),
        ViewerScene::SkeletalMesh {
            lods: vec![lod0, lod1],
            skeleton,
            skins: vec![None],
            animation: None,
        },
    );
    assert_eq!(view.static_lods().map_or(0, <[MeshLod]>::len), 2);
    view.lod_index = 1;
    view.render_preview();
    assert!(view.error.is_none());
    assert!(view.image_path.is_file());
}

/// 骨骼查看器同样生成预览图片
#[test]
fn skeletal_mesh_viewer_renders_preview_with_bones() {
    use lumark_unreal::Bone;

    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
    };
    let skeleton = Skeleton {
        name: "SKEL_Test".to_string(),
        bones: vec![Bone {
            name: "Root".to_string(),
            parent_index: -1,
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }],
    };
    let view = AssetViewerWindowView::new(
        "SK_Test".to_string(),
        ViewerScene::SkeletalMesh {
            lods: vec![mesh],
            skeleton,
            skins: vec![None],
            animation: None,
        },
    );
    assert!(view.error.is_none());
    assert!(view.image_path.is_file());
}

/// 蒙皮演示会按权重驱动骨骼变形并重新生成图片
#[test]
fn skeletal_mesh_viewer_skins_dominant_bone() {
    use lumark_unreal::{Bone, SkinWeights};

    let mesh = MeshLod {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
    };
    let skeleton = Skeleton {
        name: "SKEL_Test".to_string(),
        bones: vec![
            Bone {
                name: "Root".to_string(),
                parent_index: -1,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            Bone {
                name: "Joint".to_string(),
                parent_index: 0,
                translation: [0.0, 0.0, 1.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
        ],
    };
    let skin = SkinWeights {
        vertices: 3,
        max_influences: 1,
        variable: false,
        influences: vec![[1u16; 12]; 3],
        weights: vec![[255u16; 12]; 3],
    };
    let mut view = AssetViewerWindowView::new(
        "SK_Test".to_string(),
        ViewerScene::SkeletalMesh {
            lods: vec![mesh],
            skeleton,
            skins: vec![Some(skin)],
            animation: None,
        },
    );
    view.skin_pose = true;
    view.render_preview();
    assert!(view.error.is_none());
    assert!(view.image_path.is_file());
}
#[test]
fn section_filter_hides_triangles() {
    let mesh = MeshLod {
        positions: vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        indices: vec![0, 1, 2, 1, 3, 2],
        sections: vec![
            MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 1,
            },
            MeshSection {
                material_index: 1,
                first_index: 3,
                triangle_count: 1,
            },
        ],
    };
    let filtered = filter_sections(&mesh, &[false, true]);
    assert_eq!(filtered.sections.len(), 1);
    assert_eq!(filtered.triangle_count(), 1);
    assert_eq!(filtered.indices, vec![0, 1, 2]);
}

/// 骨骼查看器同样生成预览图片
#[test]
fn skeleton_viewer_renders_preview() {
    use lumark_unreal::Bone;

    let skeleton = Skeleton {
        name: "SKEL_Test".to_string(),
        bones: vec![
            Bone {
                name: "Root".to_string(),
                parent_index: -1,
                translation: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            Bone {
                name: "Joint".to_string(),
                parent_index: 0,
                translation: [0.0, 0.0, 5.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
        ],
    };
    let view = AssetViewerWindowView::new("SKEL".to_string(), ViewerScene::Skeleton(skeleton));
    assert!(view.error.is_none());
    assert!(view.image_path.is_file());
}
