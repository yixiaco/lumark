//! 诊断：把材质资产导出为贴图 + materials.json 旁挂文件。
//!
//! 用法：`export_material_sidecars <game-root> <container> <mapping.usmap> <material> <out-dir> [png|jpeg|tga|webp]`

use std::error::Error;
use std::path::Path;

use lumark_export::{
    MaterialSidecarOptions, TextureExportFormat, material_sidecars, write_resources,
};
use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, export_materials,
    extract_key_for_game, resolve_material_asset, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let material_entry = args.next().ok_or("缺少材质资产路径")?;
    let output = args.next().ok_or("缺少输出目录")?;
    let format = match args.next().unwrap_or_else(|| "png".to_string()).as_str() {
        "png" => TextureExportFormat::Png,
        "jpg" | "jpeg" => TextureExportFormat::Jpeg,
        "tga" => TextureExportFormat::Tga,
        "webp" => TextureExportFormat::Webp,
        other => return Err(format!("未知贴图格式：{other}").into()),
    };

    let game = scan_game(Path::new(&root))?;
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
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;
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
        &material_entry,
        Some(&mappings),
        &MaterialResolveOptions::default(),
    );
    let materials = export_materials(&[resolved]);
    let resources = material_sidecars(
        &materials,
        &MaterialSidecarOptions {
            format,
            quality: 90,
        },
    )?;
    let written = write_resources(Path::new(&output), &resources)?;
    println!(
        "material={} format={} files={}",
        materials
            .first()
            .map(|material| material.name.as_str())
            .unwrap_or(""),
        format.label(),
        written.len()
    );
    for path in written {
        println!("  {}", path.display());
    }
    Ok(())
}
