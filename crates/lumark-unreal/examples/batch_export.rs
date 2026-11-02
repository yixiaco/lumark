//! 诊断：按导出配置批量导出容器内网格资产。
//!
//! 用法：`batch_export <game-root> <container> <mapping.usmap> <out-dir> [formats] [filter] [limit] [cancel-after]`
//! 其中 `formats` 为逗号分隔的 `glb,uemodel,usda,psk`，默认 `glb,uemodel`；
//! `cancel-after` 表示成功导出 N 个资产后触发取消（验证任务控制）。

use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lumark_unreal::{
    BatchExportConfig, BatchExportFormat, BatchProgress, GameAssetOptions, GameAssetSource,
    Mappings, export_archive_cancellable, extract_key_for_game, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出目录")?;
    let formats = args.next().unwrap_or_else(|| "glb,uemodel".to_string());
    let filter = args.next();
    let limit = args.next().and_then(|value| value.parse::<usize>().ok());
    let cancel_after = args.next().and_then(|value| value.parse::<usize>().ok());

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

    let formats = parse_formats(&formats)?;
    let config = BatchExportConfig {
        output_dir: output.clone().into(),
        formats,
        asset_filter: filter,
        limit,
        overwrite: true,
        ..BatchExportConfig::default()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let mut exported_count = 0usize;
    let report = export_archive_cancellable(
        &mut source,
        &mappings,
        &config,
        Some(cancel.as_ref()),
        |progress| match progress {
            BatchProgress::Analyzing {
                index,
                total,
                asset,
            } => {
                println!("[{index}/{total}] {asset}");
            }
            BatchProgress::Exported { asset, files } => {
                exported_count += 1;
                println!("  完成 {asset} → {} 个文件", files.len());
                if cancel_after.is_some_and(|limit| exported_count >= limit) {
                    cancel.store(true, Ordering::Relaxed);
                    println!("  已触发取消标志");
                }
            }
            BatchProgress::Skipped { asset, reason } => {
                println!("  跳过 {asset}：{reason}");
            }
            BatchProgress::Failed { asset, message } => {
                println!("  失败 {asset}：{message}");
            }
            BatchProgress::Cancelled { asset } => {
                println!("  已取消：{asset}");
            }
        },
    )?;
    println!(
        "汇总：导出 {} 个资产 / {} 个文件，跳过 {}，失败 {}，取消 {}，耗时 {:.2}s",
        report.exported.len(),
        report.file_count(),
        report.skipped,
        report.failures.len(),
        report.cancelled,
        report.elapsed_seconds
    );
    for failure in report.failures.iter().take(10) {
        println!("失败明细：{} → {}", failure.asset, failure.message);
    }
    let report_path = Path::new(&output).join("batch-report.json");
    std::fs::write(&report_path, report.to_json_pretty()?)?;
    println!("报告：{}", report_path.display());
    Ok(())
}

/// 解析格式列表。
fn parse_formats(value: &str) -> Result<Vec<BatchExportFormat>, String> {
    let mut formats = Vec::new();
    for part in value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let format = match part.to_ascii_lowercase().as_str() {
            "glb" | "gltf" => BatchExportFormat::Glb,
            "uemodel" | "ueformat" => BatchExportFormat::UeModel,
            "usda" | "usd" => BatchExportFormat::Usda,
            "psk" => BatchExportFormat::Psk,
            other => return Err(format!("未知导出格式：{other}")),
        };
        formats.push(format);
    }
    if formats.is_empty() {
        return Err("至少选择一种导出格式".to_string());
    }
    Ok(formats)
}
