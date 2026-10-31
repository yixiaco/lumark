use super::*;
use lumark_archive::{ArchiveFormat, CompressionOptions};

/// 小型 struct/数组会展开一层，便于详情面板直接读取字段。
#[test]
fn formats_nested_property_values() {
    let value = PropValue::Struct(vec![
        ("Name".to_string(), PropValue::Name("Bone".to_string())),
        (
            "Indices".to_string(),
            PropValue::Array(vec![PropValue::Int(1), PropValue::Int(2)]),
        ),
    ]);
    assert_eq!(format_prop_value(&value), "{Name=Bone, Indices=[1, 2]}");
}

/// 使用 CUE4Parse UE5.8 fixture 验证完整纹理预览链路。
///
/// 需要把 `LUMARK_UE_FIXTURE_ROOT` 指向 `CUE4Parse.Tests/Fixtures/UE5_8`；
/// 未配置时跳过，避免测试依赖外部仓库。
#[test]
fn loads_fixture_assets_when_available() {
    let Ok(root) = std::env::var("LUMARK_UE_FIXTURE_ROOT") else {
        return;
    };
    let base = std::path::PathBuf::from(root);
    let toc = base.join(
        "IoStore/Unversioned/Uncompressed/CUE4ParseFixtures-Minimal-Uncompressed-Windows.utoc",
    );
    let global = base.join("IoStore/Unversioned/global.utoc");
    let mapping = base.join("Mappings/CUE4ParseFixtures-Uncompressed.usmap");
    if !toc.is_file() || !global.is_file() || !mapping.is_file() {
        return;
    }
    let temp = tempfile::tempdir().expect("temp dir");
    let output = temp.path().join("preview.png");
    let result = load_ue_asset_preview(
        &toc,
        &global,
        "CUE4ParseFixtures/Content/Fixtures/Textures/T_BC1.uasset",
        Some(&mapping),
        None,
        None,
        &output,
    );
    match result {
        UePreviewResult::Texture {
            path,
            name,
            subtitle,
            rows,
        } => {
            assert_eq!(name, "T_BC1");
            assert!(subtitle.contains("PF_DXT1"));
            assert!(
                rows.iter()
                    .any(|(label, _)| label != "属性头" && !label.is_empty())
            );
            assert!(path.is_file());
        }
        _ => panic!("fixture texture preview failed"),
    }

    // 未选择映射时仍展示包级元数据，而不是直接失败。
    match load_ue_asset_preview(
        &toc,
        &global,
        "CUE4ParseFixtures/Content/Fixtures/Textures/T_BC1.uasset",
        None,
        None,
        None,
        &temp.path().join("metadata.png"),
    ) {
        UePreviewResult::Metadata {
            class_name, rows, ..
        } => {
            assert_eq!(class_name, "Texture2D");
            assert!(rows.iter().any(|(label, _)| label == "包"));
        }
        _ => panic!("fixture no-mapping metadata failed"),
    }

    // 普通 PAK（ZIP 容器）中的同一 Zen 包也应能预览。
    let package_bytes = IoStoreContainer::open(&toc, None)
        .expect("fixture container")
        .read_file("CUE4ParseFixtures/Content/Fixtures/Textures/T_BC1.uasset")
        .expect("fixture texture entry");
    let source_dir = temp.path().join("pak-source");
    std::fs::create_dir_all(&source_dir).expect("pak source dir");
    let source = source_dir.join("T_BC1.uasset");
    std::fs::write(&source, &package_bytes).expect("write texture package");
    let pak = temp.path().join("fixture.pak");
    ArchiveService::create(
        &pak,
        &[source],
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .expect("create fixture pak");
    let pak_output = temp.path().join("pak-preview.png");
    match load_ue_asset_preview(
        &pak,
        &global,
        "T_BC1.uasset",
        Some(&mapping),
        None,
        None,
        &pak_output,
    ) {
        UePreviewResult::Texture { name, path, .. } => {
            assert_eq!(name, "T_BC1");
            assert!(path.is_file());
        }
        _ => panic!("fixture pak texture preview failed"),
    }

    let model = load_ue_asset_preview(
        &toc,
        &global,
        "CUE4ParseFixtures/Content/Fixtures/Meshes/SM_Fixture.uasset",
        Some(&mapping),
        None,
        None,
        &temp.path().join("model.png"),
    );
    match model {
        UePreviewResult::Model {
            path,
            name,
            subtitle,
            rows,
            ..
        } => {
            assert_eq!(name, "SM_Fixture");
            assert!(subtitle.contains("6 顶点"));
            assert!(subtitle.contains("2 三角形"));
            assert!(!rows.is_empty());
            assert!(path.is_file());
        }
        _ => panic!("fixture static mesh preview failed"),
    }

    let skeleton = load_ue_asset_preview(
        &toc,
        &global,
        "CUE4ParseFixtures/Content/Fixtures/Animations/SKEL_Fixture.uasset",
        Some(&mapping),
        None,
        None,
        &temp.path().join("skeleton.png"),
    );
    match skeleton {
        UePreviewResult::Skeleton {
            path,
            subtitle,
            skeleton,
            rows,
            name: _,
        } => {
            assert_eq!(skeleton.bones.len(), 3);
            assert!(subtitle.contains("3 根参考骨骼"));
            assert!(!rows.is_empty());
            assert!(path.is_file());
        }
        _ => panic!("fixture skeleton preview failed"),
    }

    let animation = load_ue_asset_preview(
        &toc,
        &global,
        "CUE4ParseFixtures/Content/Fixtures/Animations/AS_Fixture.uasset",
        Some(&mapping),
        None,
        None,
        &temp.path().join("animation.png"),
    );
    match animation {
        UePreviewResult::Metadata {
            class_name,
            timeline_seconds,
            ..
        } => {
            assert_eq!(class_name, "AnimSequence");
            assert!(timeline_seconds.is_some_and(|length| (length - 1.0).abs() < 1.0e-4));
        }
        _ => panic!("fixture animation metadata failed"),
    }

    let skeletal = load_ue_asset_preview(
        &toc,
        &global,
        "CUE4ParseFixtures/Content/Fixtures/Meshes/SK_Fixture.uasset",
        Some(&mapping),
        None,
        None,
        &temp.path().join("skeletal.png"),
    );
    match skeletal {
        UePreviewResult::Model {
            path,
            subtitle,
            mesh,
            skeleton,
            ..
        } => {
            assert_eq!(mesh.positions.len(), 6);
            assert!(mesh.triangle_count() > 0);
            assert!(!mesh.sections.is_empty(), "SkeletalMesh section palette");
            assert!(
                skeleton.is_some_and(|skeleton| skeleton.bones.len() == 3),
                "SkeletalMesh 参考骨骼应随模型预览返回"
            );
            assert!(subtitle.contains("SkeletalMesh"));
            assert!(subtitle.contains("6 顶点"), "subtitle={subtitle}");
            assert!(path.is_file());
        }
        _ => panic!("fixture skeletal mesh geometry preview failed"),
    }
}
