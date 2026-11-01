//! 查看器窗口的单元测试。

use super::*;
use gpui_kit::test::TestWindowExt;
use lumark_unreal::{
    LegacyPackage, Mappings, MeshSection, read_legacy_anim_sequence,
    read_legacy_skeletal_mesh_geometry,
};

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

/// 时间轴应把时间夹到时长内，并换算成 1 起的帧号。
#[test]
fn animation_timeline_clamps_ratio_and_frame() {
    let start = AnimationTimeline::new(2.0, 5, 0.0);
    assert_eq!(start.ratio(), 0.0);
    assert_eq!(start.frame(), 1);

    let middle = AnimationTimeline::new(2.0, 5, 1.0);
    assert!((middle.ratio() - 0.5).abs() < 1e-6);
    assert_eq!(middle.frame(), 3);

    let end = AnimationTimeline::new(2.0, 5, 99.0);
    assert_eq!(end.ratio(), 1.0);
    assert_eq!(end.frame(), 5, "超长时间应夹到最后一帧");

    let empty = AnimationTimeline::new(0.0, 0, 1.0);
    assert_eq!(empty.ratio(), 0.0);
    assert_eq!(empty.frame(), 1);
    assert_eq!(empty.total_frames, 1, "至少应有一个帧号");
}

/// 逐帧按钮按总帧数换算帧间隔，并在首尾夹取。
#[test]
fn animation_timeline_steps_by_frame_and_clamps() {
    let timeline = AnimationTimeline::new(2.0, 5, 0.0);
    assert!((timeline.stepped(1) - 0.5).abs() < 1e-6);
    assert!((timeline.stepped(2) - 1.0).abs() < 1e-6);
    assert!((timeline.stepped(-1) - 0.0).abs() < 1e-6, "不能早于开头");

    let end = AnimationTimeline::new(2.0, 5, 2.0);
    assert!((end.stepped(1) - 2.0).abs() < 1e-6, "不能超过结尾");
    assert!((end.stepped(-4) - 0.0).abs() < 1e-6);
}

/// 工具栏标签应同时显示当前帧/总帧数和秒数。
#[test]
fn animation_timeline_label_shows_frame_and_time() {
    let label = AnimationTimeline::new(2.0, 5, 1.0).label();
    assert_eq!(label, "3/5 · 1.00s / 2.00s");
}

/// 用真实 FF7R 资产验证查看器能渲染动画时间轴和播放控制。
///
/// 需要设置 `LUMARK_FF7R_ANIM_FIXTURE`（含 `SK_Mannequin` 与
/// `ThirdPersonIdle` 的 `.uasset/.uexp` 目录树）和 `LUMARK_FF7R_MAPPINGS`；
/// 未配置时跳过，避免测试依赖游戏数据。
#[gpui_kit::test]
fn viewer_renders_animation_timeline_with_real_fixture(cx: &mut gpui_kit::TestAppContext) {
    let (Ok(fixture), Ok(mappings_path)) = (
        std::env::var("LUMARK_FF7R_ANIM_FIXTURE"),
        std::env::var("LUMARK_FF7R_MAPPINGS"),
    ) else {
        return;
    };
    let fixture = std::path::PathBuf::from(fixture);
    let mappings = Mappings::parse_file(std::path::Path::new(&mappings_path)).expect("映射");
    let mesh = fixture.join("SK_Mannequin");
    let anim = fixture.join("ThirdPersonIdle");
    let mesh_package = LegacyPackage::parse(
        &std::fs::read(mesh.with_extension("uasset")).expect("mesh uasset"),
        &std::fs::read(mesh.with_extension("uexp")).expect("mesh uexp"),
    )
    .expect("解析网格包");
    let geometry =
        read_legacy_skeletal_mesh_geometry(&mesh_package, 0, &mappings).expect("网格几何");
    let anim_package = LegacyPackage::parse(
        &std::fs::read(anim.with_extension("uasset")).expect("anim uasset"),
        &std::fs::read(anim.with_extension("uexp")).expect("anim uexp"),
    )
    .expect("解析动画包");
    let animation = read_legacy_anim_sequence(&anim_package, 0, &mappings).expect("动画解码");
    assert!(animation.length > 0.0);
    assert!(animation.frames > 1);
    let scene = ViewerScene::SkeletalMesh {
        lods: vec![geometry.mesh],
        skeleton: geometry.skeleton,
        skins: vec![geometry.skin],
        animation: Some(Box::new(animation)),
    };
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, _cx| {
        AssetViewerWindowView::new("SK_Mannequin".to_string(), scene)
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.find("model-animation").visible(),
            "有动画和蒙皮时应显示播放按钮"
        );
        assert!(window.find("model-animation-prev").visible());
        assert!(window.find("model-animation-next").visible());
        assert!(window.find("model-animation-start").visible());
        view.update(cx, |view, cx| {
            let (length, frames) = view.animation_info().expect("动画信息");
            assert!(
                length > 0.0 && frames > 1,
                "length={length} frames={frames}"
            );
            view.step_animation(1, cx);
            assert!(view.animation_time > 0.0, "下一帧应推进动画时间");
            assert!(!view.animation_playing, "逐帧查看时应暂停播放");
            view.reset_animation(cx);
            assert_eq!(view.animation_time, 0.0, "回到开头应重置时间");
        });
        window.draw(cx).clear(cx);
    });
}

/// 材质场景会渲染近似 PBR 材质球，拖拽旋转后重绘出新画面。
#[test]
fn viewer_renders_material_scene_and_orbits() {
    let viewer = lumark_unreal::MaterialViewerData {
        appearance: lumark_unreal::MaterialAppearance::default(),
        base_color: lumark_unreal::MaterialViewerImage::new(
            [180, 90, 40, 255].repeat(16),
            4,
            4,
            512,
        ),
        normal: None,
        metallic_roughness: None,
    };
    let mut view =
        AssetViewerWindowView::new("MAT_Teapot".to_string(), ViewerScene::Material { viewer });
    assert!(view.error.is_none());
    assert!(view.image_path.is_file());
    let before = std::fs::read(&view.image_path).expect("读取材质球图片");
    view.yaw = 150.0;
    view.pitch = 10.0;
    view.render_preview();
    assert!(view.error.is_none());
    let after = std::fs::read(&view.image_path).expect("读取旋转后的材质球图片");
    assert_ne!(before, after, "旋转后应重绘材质球");
    assert_eq!(
        (view.scene_mesh().is_none(), view.static_lods().is_none()),
        (true, true)
    );
}
