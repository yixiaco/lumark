//! 诊断：单资产按配置导出（多格式 + 材质旁挂）。
//!
//! 用法：`export_asset <game-root> <container> <mesh-entry> <mapping.usmap> <out-dir> [formats]`

use std::error::Error;
use std::path::Path;

use lumark_export::TextureExportFormat;
use lumark_unreal::{
    AssetExportConfig, AssetExportProgress, BatchExportFormat, GameAssetOptions, GameAssetSource,
    Mappings, export_asset, extract_key_for_game, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mesh_entry = args.next().ok_or("缺少网格资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出目录")?;
    let formats = args.next().unwrap_or_else(|| "glb,usda".to_string());

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
    let config = AssetExportConfig {
        output_dir: output.clone().into(),
        formats: formats
            .split(',')
            .filter_map(|part| match part.trim().to_ascii_lowercase().as_str() {
                "glb" | "gltf" => Some(BatchExportFormat::Glb),
                "uemodel" => Some(BatchExportFormat::UeModel),
                "usda" => Some(BatchExportFormat::Usda),
                "psk" => Some(BatchExportFormat::Psk),
                _ => None,
            })
            .collect(),
        texture_format: TextureExportFormat::Png,
        ..AssetExportConfig::default()
    };
    let report = export_asset(
        &mut source,
        &mappings,
        &mesh_entry,
        &config,
        None,
        |progress| match progress {
            AssetExportProgress::Writing { format, path } => {
                println!("写 {format} → {}", path.display());
            }
            AssetExportProgress::Sidecars => println!("写材质旁挂"),
            AssetExportProgress::Finished => println!("完成"),
        },
    )?;
    println!(
        "asset={} files={} sidecars={} verts={} tris={} bones={} elapsed={:.2}s",
        report.asset_path,
        report.files.len(),
        report.sidecar_files.len(),
        report.vertices,
        report.triangles,
        report.bones,
        report.elapsed_seconds
    );
    println!("report={}", report.to_json_pretty()?);
    Ok(())
}
