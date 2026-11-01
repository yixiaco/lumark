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

/// 加密 PAK 的资产解压必须带上游戏 AES 密钥，不能把每个条目都误报成“需要密码”。
#[test]
fn encrypted_pak_asset_inputs_use_aes_key() {
    const KEY: &str = "94d25bc3aeb420e0be914edc9d5435a1eaab5f2864e09e94019ac205b727a7de";
    let pak = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lumark-archive/tests/fixtures/unreal/pack_v11_encrypt.pak");
    if !pak.is_file() {
        return;
    }
    let global = std::path::Path::new("missing-global.utoc");
    match read_ue_asset_inputs(&pak, global, "test.txt", None) {
        Err(error) => assert_eq!(error, "需要密码"),
        Ok(_) => panic!("未提供密钥时应拒绝读取加密 PAK 条目"),
    }
    let (data, ubulk, uexp, scripts) = read_ue_asset_inputs(&pak, global, "test.txt", Some(KEY))
        .unwrap_or_else(|error| panic!("加密 PAK 条目读取失败：{error}"));
    assert!(!data.is_empty(), "应读到加密条目的真实内容");
    assert!(ubulk.is_none());
    assert!(uexp.is_none());
    assert!(scripts.is_none());
}

/// 对象索引会解析成导入/导出名称，便于材质依赖阅读。
#[test]
fn resolves_object_references_in_property_values() {
    let value = PropValue::Struct(vec![
        ("Parent".to_string(), PropValue::Object(-2)),
        ("Texture".to_string(), PropValue::Object(1)),
        ("Empty".to_string(), PropValue::Object(0)),
    ]);
    let resolve = |index: i32| match index {
        -2 => Some("M_UE4Man_Body".to_string()),
        1 => Some("SelfExport".to_string()),
        _ => None,
    };
    let text = format_prop_value_with(&value, &resolve);
    assert!(text.contains("M_UE4Man_Body"), "{text}");
    assert!(text.contains("SelfExport"), "{text}");
    assert!(text.contains("Empty=None"), "{text}");
}

/// 用真实 FF7R 材质资产验证材质预览行（可选环境变量）。
///
/// 需要设置 `LUMARK_FF7R_MATERIAL_FIXTURE`（含 M_UE4Man_ChestLogo.uasset/.uexp）
/// 与 `LUMARK_FF7R_MAPPINGS`；未配置时跳过，避免测试依赖游戏数据。
#[test]
fn material_rows_from_real_fixture_when_available() {
    let (Ok(base), Ok(mappings_path)) = (
        std::env::var("LUMARK_FF7R_MATERIAL_FIXTURE"),
        std::env::var("LUMARK_FF7R_MAPPINGS"),
    ) else {
        return;
    };
    let base = std::path::PathBuf::from(base);
    let asset = std::fs::read(base.join("M_UE4Man_ChestLogo.uasset")).expect("读取材质 uasset");
    let uexp = std::fs::read(base.join("M_UE4Man_ChestLogo.uexp")).expect("读取材质 uexp");
    let rows = legacy_material_rows(
        &asset,
        &uexp,
        Some(std::path::Path::new(&mappings_path)),
        "MaterialInstanceConstant",
        "M_UE4Man_ChestLogo",
    );
    assert!(rows.iter().any(|(key, _)| key == "材质类"));
    assert!(
        rows.iter()
            .any(|(key, value)| key == "依赖" && value.contains("M_UE4Man_Body")),
        "应显示父材质引用：{rows:?}"
    );
    assert!(
        rows.iter().any(|(key, _)| key == "纹理引用"),
        "应显示纹理引用：{rows:?}"
    );
}
