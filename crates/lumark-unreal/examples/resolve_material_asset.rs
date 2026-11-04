//! 诊断：直接解析单个材质/材质实例资产的外观与贴图。
//!
//! 用法：`resolve_material_asset <game-root> <container> <material-entry> <mapping.usmap>`

use std::error::Error;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, extract_key_for_game,
    resolve_material_asset, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少材质资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let game = scan_game(std::path::Path::new(&root))?;
    let container_path = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == container_name)
        })
        .map(|container| container.path.clone())
        .ok_or("未找到指定容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到 AES 密钥")?;
    let global = container_path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let mut source = GameAssetSource::open(
        &container_path,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(mapping_path.into()),
        },
    )?;
    let resolved = resolve_material_asset(
        &mut source,
        &entry,
        Some(&mappings),
        &MaterialResolveOptions::default(),
    );
    println!(
        "material={:?} appearance={:?}",
        resolved.package_path,
        resolved.appearance.as_ref().map(|appearance| (
            appearance.base_color,
            appearance.metallic,
            appearance.roughness,
            appearance.textures.len()
        ))
    );
    if let Some(viewer) = &resolved.viewer {
        println!(
            "textures base={} normal={} orm={} ao={}",
            viewer.base_color.is_some(),
            viewer.normal.is_some(),
            viewer.metallic_roughness.is_some(),
            viewer.occlusion.is_some()
        );
        for (label, image) in [
            ("base", viewer.base_color.as_ref()),
            ("normal", viewer.normal.as_ref()),
            ("orm", viewer.metallic_roughness.as_ref()),
            ("ao", viewer.occlusion.as_ref()),
        ] {
            if let Some(image) = image {
                let pixels = image.rgba.len() / 4;
                let mut sums = [0u64; 4];
                for pixel in image.rgba.as_chunks::<4>().0 {
                    for channel in 0..4 {
                        sums[channel] += u64::from(pixel[channel]);
                    }
                }
                let count = pixels.max(1) as u64;
                println!(
                    "  {label}: {}x{} avg=({}, {}, {}, {})",
                    image.width,
                    image.height,
                    sums[0] / count,
                    sums[1] / count,
                    sums[2] / count,
                    sums[3] / count
                );
            }
        }
    }
    for asset in &resolved.loaded_assets {
        println!("loaded={asset}");
    }
    for warning in &resolved.warnings {
        println!("warning={warning}");
    }
    Ok(())
}
