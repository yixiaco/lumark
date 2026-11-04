//! 查看器窗口的单元测试。

use super::animation::AnimationTimeline;
use super::render::filter_sections;
use super::*;

/// 测试用空材质列表。
fn no_materials() -> std::sync::Arc<Vec<lumark_unreal::ResolvedMaterial>> {
    std::sync::Arc::new(Vec::new())
}
use gpui_kit::test::TestWindowExt;
use lumark_unreal::{
    LegacyPackage, Mappings, MeshSection, read_legacy_anim_sequence,
    read_legacy_skeletal_mesh_geometry,
};

/// 窗口初始化时生成可读取的预览图片
#[test]
fn viewer_renders_initial_preview_image() {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let view = AssetViewerWindowView::new(
        "Mesh".to_string(),
        ViewerScene::Mesh {
            materials: no_materials(),
            lods: vec![mesh],
        },
    );
    assert!(view.error.is_none());
    assert_eq!(view.hidden_sections.len(), 1);
    assert!(view.image.is_some());
    assert!(
        view.image
            .as_ref()
            .and_then(|image| image.as_bytes(0))
            .is_some_and(|bytes| !bytes.is_empty())
    );
}

/// 查看器必须实际绘制渲染图像元素（防止画布空白回归）。
#[gpui_kit::test]
fn viewer_paints_rendered_image_element(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let (_view, cx) = cx.add_window_view(|_, _| {
        AssetViewerWindowView::new(
            "Mesh".to_string(),
            ViewerScene::Mesh {
                materials: no_materials(),
                lods: vec![mesh],
            },
        )
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let canvas = cx
        .debug_bounds("model-viewer-canvas")
        .expect("画布应记录布局 bounds");
    let image = cx
        .debug_bounds("model-viewer-image-container")
        .expect("渲染图像容器应记录布局 bounds");
    assert!(
        image.size.height > px(100.) && image.size.width > px(100.),
        "渲染图像容器尺寸异常（{image:?}），查看器会显示空白画布"
    );
    assert!(
        image.origin.y >= canvas.origin.y - px(1.0),
        "渲染图像容器应位于画布内部"
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
        attributes: Default::default(),
    };
    let lod1 = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let mut view = AssetViewerWindowView::new(
        "Mesh".to_string(),
        ViewerScene::Mesh {
            materials: no_materials(),
            lods: vec![lod0, lod1],
        },
    );
    assert_eq!(view.static_lods().map_or(0, <[MeshLod]>::len), 2);
    view.lod_index = 1;
    view.render_preview();
    assert!(view.error.is_none());
    assert!(view.image.is_some());
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
        attributes: Default::default(),
    };
    let lod1 = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: Vec::new(),
        attributes: Default::default(),
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
            materials: no_materials(),
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
    assert!(view.image.is_some());
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
        attributes: Default::default(),
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
            materials: no_materials(),
            lods: vec![mesh],
            skeleton,
            skins: vec![None],
            animation: None,
        },
    );
    assert!(view.error.is_none());
    assert!(view.image.is_some());
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
        attributes: Default::default(),
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
            materials: no_materials(),
            lods: vec![mesh],
            skeleton,
            skins: vec![Some(skin)],
            animation: None,
        },
    );
    view.skin_pose = true;
    view.render_preview();
    assert!(view.error.is_none());
    assert!(view.image.is_some());
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
        attributes: Default::default(),
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
    assert!(view.image.is_some());
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
        materials: no_materials(),
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
        occlusion: None,
    };
    let mut view =
        AssetViewerWindowView::new("MAT_Teapot".to_string(), ViewerScene::Material { viewer });
    assert!(view.error.is_none());
    assert!(view.image.is_some());
    let before = view
        .image
        .as_ref()
        .and_then(|image| image.as_bytes(0))
        .map(<[u8]>::to_vec)
        .expect("读取材质球像素");
    view.yaw = 150.0;
    view.pitch = 10.0;
    view.render_preview();
    assert!(view.error.is_none());
    let after = view
        .image
        .as_ref()
        .and_then(|image| image.as_bytes(0))
        .map(<[u8]>::to_vec)
        .expect("读取旋转后的材质球像素");
    assert_ne!(before, after, "旋转后应重绘材质球");
    assert_eq!(
        (view.scene_mesh().is_none(), view.static_lods().is_none()),
        (true, true)
    );
}

/// 材质查看器只显示通用视角操作，不显示蒙皮/LOD 等网格专用按钮。
#[gpui_kit::test]
fn material_viewer_toolbar_hides_mesh_controls(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let scene = ViewerScene::Material {
        viewer: lumark_unreal::MaterialViewerData {
            appearance: lumark_unreal::MaterialAppearance::default(),
            base_color: None,
            normal: None,
            metallic_roughness: None,
            occlusion: None,
        },
    };
    let (_view, cx) =
        cx.add_window_view(|_, _| AssetViewerWindowView::new("MAT_Teapot".to_string(), scene));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("model-reset").visible());
        assert!(window.find("model-zoom-in").visible());
        assert!(
            window.try_find("model-skin-pose").is_none(),
            "材质查看器不应显示蒙皮演示按钮"
        );
        assert!(
            window.try_find("model-lod").is_none(),
            "材质查看器不应显示 LOD 按钮"
        );
    });
}

/// 检查器面板默认显示、关闭后隐藏，并列出 section/LOD 控件。
#[gpui_kit::test]
fn inspector_panel_renders_and_toggles(cx: &mut gpui_kit::TestAppContext) {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let scene = ViewerScene::Mesh {
        materials: no_materials(),
        lods: vec![mesh.clone(), mesh],
    };
    cx.update(gpui_kit::init);
    let (view, cx) =
        cx.add_window_view(|_, _| AssetViewerWindowView::new("SM_Inspector".to_string(), scene));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("model-inspector-close").visible());
        assert!(
            window.find("model-inspector-lod-1").visible(),
            "多 LOD 时应显示 LOD 列表"
        );
        view.update(cx, |view, cx| {
            view.inspector_open = false;
            cx.notify();
            assert!(!view.inspector_open);
        });
    });
}

/// 异步帧泵：后台渲染完成后更新图像，旧代号结果被丢弃。
#[gpui_kit::test]
fn viewer_async_preview_updates_image(cx: &mut gpui_kit::TestAppContext) {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_, _| {
        AssetViewerWindowView::new(
            "SM_Async".to_string(),
            ViewerScene::Mesh {
                materials: no_materials(),
                lods: vec![mesh],
            },
        )
    });
    view.update(cx, |view, cx| {
        view.image = None;
        view.request_preview(cx);
    });
    for _ in 0..100 {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| view.image.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (has_image, rendering) =
        view.read_with(cx, |view, _| (view.image.is_some(), view.rendering));
    assert!(has_image, "异步渲染应更新图像");
    assert!(!rendering, "渲染完成后应复位 rendering");
}

/// 静止 180ms 后自动补一帧高质量画面（帧泵端到端）。
#[gpui_kit::test]
fn viewer_settles_to_high_quality_frame(cx: &mut gpui_kit::TestAppContext) {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        AssetViewerWindowView::new_with_focus(
            "SM_Settle".to_string(),
            ViewerScene::Mesh {
                materials: no_materials(),
                lods: vec![mesh],
            },
            cx.focus_handle(),
            window,
            cx,
        )
    });
    // 交互帧由后台帧泵产出。
    let mut has_image = false;
    for _ in 0..400 {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| view.image.is_some()) {
            has_image = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(has_image, "首帧应由后台帧泵产出");
    // 静止超过 180ms 后应触发并完成高质量帧。
    let mut settle_requested = false;
    for _ in 0..400 {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| view.frame_pump.has_settled_frame()) {
            settle_requested = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(settle_requested, "静止 180ms 后应补高质量帧");
    let mut settled_done = false;
    for _ in 0..400 {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| {
            !view.frame_pump.in_flight() && view.frame_pump.has_settled_frame()
        }) {
            settled_done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(settled_done, "高质量帧应渲染完成");
}

/// 查看器接收材质槽名称，供 section 列表展示。
#[test]
fn viewer_accepts_material_names() {
    let mesh = MeshLod {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let view = AssetViewerWindowView::new(
        "SM_Mat".to_string(),
        ViewerScene::Mesh {
            materials: no_materials(),
            lods: vec![mesh],
        },
    )
    .with_material_names(vec!["M_Brick".to_string()]);
    assert_eq!(view.material_names, vec!["M_Brick"]);
}

/// 骨骼搜索大小写不敏感，空搜索匹配全部。
#[test]
fn bone_search_matches_case_insensitive() {
    assert!(super::inspector::bone_matches("Hand_L", ""));
    assert!(super::inspector::bone_matches("Hand_L", "hand"));
    assert!(super::inspector::bone_matches("Chest", "st"));
    assert!(!super::inspector::bone_matches("Head", "hand"));
}

/// 快捷键：F 重置、G 网格、L/N 着色，未知键不消费。
#[gpui_kit::test]
fn viewer_keyboard_shortcuts_update_state(cx: &mut gpui_kit::TestAppContext) {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        AssetViewerWindowView::new_with_focus(
            "SM_Keys".to_string(),
            ViewerScene::Mesh {
                materials: no_materials(),
                lods: vec![mesh],
            },
            cx.focus_handle(),
            window,
            cx,
        )
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(
                view.timeline_slider.is_some(),
                "正式查看器应创建时间轴 Slider"
            );
            assert!(view.bone_filter.is_some(), "正式查看器应创建骨骼搜索框");
            view.yaw = 90.0;
            view.zoom = 2.0;
            assert!(view.handle_shortcut("f", cx));
            assert_eq!(view.yaw, 35.0);
            assert_eq!(view.zoom, 1.0);
            assert!(view.show_grid, "3D 查看器默认显示地面网格");
            assert!(view.handle_shortcut("g", cx));
            assert!(!view.show_grid, "G 键应关闭网格");
            assert!(view.handle_shortcut("g", cx));
            assert!(view.show_grid);
            assert!(view.handle_shortcut("w", cx));
            assert!(view.wireframe);
            assert!(view.handle_shortcut("l", cx));
            assert_eq!(view.shading, lumark_render::ShadingMode::Unlit);
            assert!(view.handle_shortcut("n", cx));
            assert_eq!(view.shading, lumark_render::ShadingMode::Normals);
            assert!(!view.handle_shortcut("z", cx), "未知键不应消费");
        });
    });
}

/// 着色模式、网格叠加与 section 隐藏都会改变渲染结果。
#[test]
fn viewer_shading_grid_and_sections_render() {
    let mesh = MeshLod {
        positions: vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
        ],
        // UE 索引是左手系顺时针正面；`convert_mesh` 会翻转为渲染器 CCW。
        indices: vec![0, 2, 1, 0, 3, 2],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 2,
        }],
        attributes: Default::default(),
    };
    let mut view = AssetViewerWindowView::new(
        "SM_Shading".to_string(),
        ViewerScene::Mesh {
            materials: no_materials(),
            lods: vec![mesh],
        },
    );
    // 3D 查看器默认显示网格；先关掉，便于对比着色与叠加层差异。
    view.show_grid = false;
    view.render_preview();
    let bytes = |view: &AssetViewerWindowView| {
        view.image
            .as_ref()
            .and_then(|image| image.as_bytes(0))
            .map(<[u8]>::to_vec)
            .expect("查看器图像")
    };
    assert!(view.error.is_none());
    assert!(view.last_backend.is_some(), "应记录渲染后端");
    let lit = bytes(&view);
    view.shading = lumark_render::ShadingMode::Unlit;
    view.render_preview();
    assert_ne!(lit, bytes(&view), "切换无光照着色应改变图像");
    let unlit = bytes(&view);
    view.show_grid = true;
    view.render_preview();
    assert_ne!(unlit, bytes(&view), "显示网格应改变图像");
    view.hidden_sections[0] = true;
    view.render_preview();
    assert!(view.error.is_none());
    let hidden = bytes(&view);
    assert_ne!(hidden, unlit, "隐藏 section 应改变图像");
}

/// 轨道拖拽方向与 FModel 一致：向右拖拽减小 yaw，向下拖拽抬高相机。
#[test]
fn orbit_drag_matches_fmodel_direction() {
    let (yaw, pitch) = super::body::apply_orbit_drag(35.0, 25.0, 10.0, 0.0);
    assert!((yaw - 31.0).abs() < 1.0e-4, "向右拖拽应绕 -Up 旋转：{yaw}");
    assert_eq!(pitch, 25.0);
    let (yaw_left, _) = super::body::apply_orbit_drag(35.0, 25.0, -10.0, 0.0);
    assert!((yaw_left - 39.0).abs() < 1.0e-4, "向左拖拽应增大 yaw");
    let (_, pitch_down) = super::body::apply_orbit_drag(0.0, 0.0, 0.0, 10.0);
    assert!((pitch_down - 4.0).abs() < 1.0e-4, "向下拖拽抬高相机");
    let (wrapped, _) = super::body::apply_orbit_drag(0.0, 0.0, 1000.0, 0.0);
    assert!((0.0..360.0).contains(&wrapped), "yaw 应归一化到 [0,360)");
    let (_, clamped) = super::body::apply_orbit_drag(0.0, 0.0, 0.0, 1000.0);
    assert_eq!(clamped, 89.0, "pitch 应限制在 ±89 度");
}

/// 查看器应使用已解析材质贴图，而不是默认灰色材质。
#[test]
fn viewer_applies_resolved_materials() {
    let mesh = MeshLod {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
        // UE 索引是左手系顺时针正面；`convert_mesh` 会翻转为渲染器 CCW。
        indices: vec![0, 2, 1],
        sections: vec![MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: 1,
        }],
        attributes: Default::default(),
    };
    let appearance = lumark_unreal::MaterialAppearance {
        base_color: [1.0, 1.0, 1.0],
        ..Default::default()
    };
    let material = lumark_unreal::ResolvedMaterial {
        slot: lumark_unreal::MaterialSlot {
            name: "M_Red".to_string(),
            package: None,
        },
        package_path: None,
        appearance: Some(appearance.clone()),
        viewer: Some(lumark_unreal::MaterialViewerData {
            appearance,
            base_color: Some(lumark_unreal::MaterialViewerImage {
                rgba: vec![255, 0, 0, 255],
                width: 1,
                height: 1,
            }),
            normal: None,
            metallic_roughness: None,
            occlusion: None,
        }),
        loaded_assets: Vec::new(),
        warnings: Vec::new(),
    };
    let mut textured = AssetViewerWindowView::new(
        "Mesh".to_string(),
        ViewerScene::Mesh {
            materials: std::sync::Arc::new(vec![material]),
            lods: vec![mesh.clone()],
        },
    );
    let mut plain = AssetViewerWindowView::new(
        "Mesh".to_string(),
        ViewerScene::Mesh {
            materials: std::sync::Arc::new(Vec::new()),
            lods: vec![mesh],
        },
    );
    let image_bytes = |view: &AssetViewerWindowView| {
        view.image
            .as_ref()
            .and_then(|image| image.as_bytes(0))
            .map(<[u8]>::to_vec)
            .expect("查看器图像")
    };
    textured.render_preview();
    plain.render_preview();
    assert_ne!(
        image_bytes(&textured),
        image_bytes(&plain),
        "已解析材质贴图应改变查看器渲染结果"
    );
}
