//! 实机验收：UE5.2 IoStore 游戏的 bulk data 链路（NTR_IDOL）。
//!
//! 默认跳过；设置 `LUMARK_NTR_GAME_ROOT` 与 `LUMARK_NTR_MAPPINGS` 后运行：
//!
//! ```powershell
//! $env:LUMARK_NTR_GAME_ROOT = "D:\Games\NTR偶像 – 梦之约定 V2.0.5"
//! $env:LUMARK_NTR_MAPPINGS = "$env:LUMARK_NTR_GAME_ROOT\5.2.1-…\Mappings\….usmap"
//! cargo test -p lumark-unreal --test ntr_iostore -j 2
//! ```
//!
//! 只读：不修改游戏目录，也不打印密钥。

use lumark_unreal::{Mappings, extract_key_for_game, parse_package, scan_game};

/// 返回实机环境；未配置时返回 `None`，让用例直接通过。
fn local_game() -> Option<(lumark_unreal::DetectedGame, std::path::PathBuf, Mappings)> {
    let root = std::env::var("LUMARK_NTR_GAME_ROOT").ok()?;
    let mapping_path = std::env::var("LUMARK_NTR_MAPPINGS").ok()?;
    let game = scan_game(std::path::Path::new(&root)).expect("扫描游戏目录");
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path)).expect("读取映射");
    Some((game, mapping_path.into(), mappings))
}

/// 找到主 `.utoc` 容器（跳过 `global.utoc`）与 `global.utoc`。
fn main_container(
    game: &lumark_unreal::DetectedGame,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let container = game
        .containers
        .iter()
        .find(|container| {
            let name = container.path.to_string_lossy();
            name.ends_with(".utoc") && !name.ends_with("global.utoc")
        })
        .map(|container| container.path.clone())?;
    let global = container.parent()?.join("global.utoc");
    Some((container, global))
}

/// UE5.2 的 cooked 包在 Zen 头里带 bulk data 映射，纹理 mip 才能定位到 `.ubulk`。
#[test]
fn zen_packages_expose_bulk_data_map() {
    let Some((game, _, _)) = local_game() else {
        return;
    };
    let Some((container_path, global)) = main_container(&game) else {
        return;
    };
    let key = extract_key_for_game(&game, None, |_, _, _| {}).key;
    let container = lumark_archive::IoStoreContainer::open(
        &container_path,
        key.as_ref().map(|key| key.as_str()),
    )
    .expect("打开容器");
    let scripts = lumark_unreal::ZenScriptObjects::from_container(
        &global,
        key.as_ref().map(|key| key.as_str()),
    )
    .expect("读取脚本对象");

    let mut checked = 0usize;
    for entry in [
        "NTR_IDOL/Content/GM_NTR_Eri/CH/NPC2/Man_Worker/textures/T_Worker_Hair_SSC.uasset",
        "NTR_IDOL/Content/StarterContent/Props/SM_PillarFrame.uasset",
    ] {
        let Ok(data) = container.read_file(entry) else {
            continue;
        };
        let package = parse_package(data, Some(&scripts), None).expect("解析包");
        assert!(
            !package.info().bulk_data.is_empty(),
            "{entry} 应带 bulk data 映射"
        );
        checked += 1;
    }
    assert!(checked > 0, "实机样本没有可检查的包");
}

/// 外部 bulk 纹理（带 `.ubulk`）与内联 bulk 纹理都能解出 mip。
#[test]
fn textures_resolve_inline_and_external_bulk() {
    let Some((game, mapping_path, mappings)) = local_game() else {
        return;
    };
    let Some((container_path, global)) = main_container(&game) else {
        return;
    };
    let key = extract_key_for_game(&game, None, |_, _, _| {}).key;
    let mut source = lumark_unreal::GameAssetSource::open(
        &container_path,
        lumark_unreal::GameAssetOptions {
            key: key.as_ref().map(|key| key.as_str()),
            global: Some(global),
            mappings: Some(mapping_path),
            ..Default::default()
        },
    )
    .expect("打开资产来源");

    // 外部 bulk：`T_Worker_Hair_SSC` 的 12 级 mip 全在 `.ubulk` 里。
    let external =
        "NTR_IDOL/Content/GM_NTR_Eri/CH/NPC2/Man_Worker/textures/T_Worker_Hair_SSC.uasset";
    let inputs = source.read_package(external).expect("读取纹理包");
    assert!(inputs.ubulk.is_some(), "外部纹理应带 .ubulk 侧载数据");
    let package = parse_package(inputs.uasset, source.scripts(), None).expect("解析纹理包");
    let texture = package
        .read_texture2d(0, &mappings, inputs.ubulk.as_deref())
        .expect("解析外部 bulk 纹理");
    assert_eq!(texture.mips.len(), 12);
    assert_eq!(texture.mips[0].data.len(), 2_097_152);

    // 内联 bulk：中文路径纹理的 mip 直接位于导出数据内，没有 `.ubulk`。
    let inline = "NTR_IDOL/Content/duanana/pic/03_H系統/1y1.uasset";
    let inputs = source.read_package(inline).expect("读取内联纹理包");
    let package = parse_package(inputs.uasset, source.scripts(), None).expect("解析内联纹理包");
    let texture = package
        .read_texture2d(0, &mappings, inputs.ubulk.as_deref())
        .expect("解析内联 bulk 纹理");
    assert_eq!(texture.width, 1200);
    assert_eq!(texture.mips[0].data.len(), 3_844_800);
}

/// UE5.2 包的导出数据按 bundle 顺序排列，StaticMesh 属性与外部 bulk 几何都要能读出。
#[test]
fn static_mesh_uses_bundle_order_and_external_bulk() {
    let Some((game, mapping_path, mappings)) = local_game() else {
        return;
    };
    let Some((container_path, global)) = main_container(&game) else {
        return;
    };
    let key = extract_key_for_game(&game, None, |_, _, _| {}).key;
    let mut source = lumark_unreal::GameAssetSource::open(
        &container_path,
        lumark_unreal::GameAssetOptions {
            key: key.as_ref().map(|key| key.as_str()),
            global: Some(global),
            mappings: Some(mapping_path),
            ..Default::default()
        },
    )
    .expect("打开资产来源");

    let mesh_path = "NTR_IDOL/Content/StarterContent/Props/SM_PillarFrame.uasset";
    let inputs = source.read_package(mesh_path).expect("读取网格包");
    let package = parse_package(inputs.uasset, source.scripts(), None).expect("解析网格包");
    let export_index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "StaticMesh")
        .expect("包内应有 StaticMesh 导出");
    // 属性流（含 `StaticMaterials`）与几何都依赖正确的导出数据偏移。
    let properties = package
        .read_export_properties(export_index, &mappings)
        .expect("读取 StaticMesh 属性");
    assert!(
        properties
            .properties
            .iter()
            .any(|(name, _)| name == "StaticMaterials"),
        "应读出 StaticMaterials"
    );
    let mesh = package
        .read_static_mesh_with_bulk(export_index, &mappings, inputs.ubulk.as_deref())
        .expect("解析 StaticMesh 几何");
    assert!(!mesh.lod.positions.is_empty());
    assert!(mesh.lod.triangle_count() > 0);
    assert!(!mesh.lod.sections.is_empty());
}
