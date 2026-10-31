//! 手工诊断：扫描容器中声明多 LOD 的 StaticMesh。
//!
//! 用法：`find_lods <game-root> <mapping.usmap> <oodle-dll> <container> <path-filter>`

use std::error::Error;
use std::path::Path;

use lumark_archive::{IoStoreContainer, configure_oodle_library};
use lumark_unreal::{Mappings, ZenScriptObjects, extract_key_for_game, parse_package, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let oodle = args.next().ok_or("缺少 Oodle DLL")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let filter = args.next().unwrap_or_default().to_ascii_lowercase();
    configure_oodle_library(Some(oodle.into())).map_err(|error| format!("Oodle：{error}"))?;
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
    let container = IoStoreContainer::open(&container_path, Some(key.as_str()))?;
    let global = container_path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let scripts = global
        .as_deref()
        .map(|path| ZenScriptObjects::from_container(path, Some(key.as_str())))
        .transpose()?;
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;
    let mut shown = 0usize;
    for file in container.files() {
        if !file.path.ends_with(".uasset") || !file.path.to_ascii_lowercase().contains(&filter) {
            continue;
        }
        let Ok(data) = container.read_file(&file.path) else {
            continue;
        };
        let Ok(package) = parse_package(data, scripts.as_ref(), None) else {
            continue;
        };
        let Some(index) = package
            .info()
            .exports
            .iter()
            .position(|export| export.class_name == "StaticMesh")
        else {
            continue;
        };
        let Ok(mesh) = package.read_static_mesh(index, &mappings) else {
            continue;
        };
        if mesh.lod_count > 1 {
            println!(
                "{} lods={} lod0_verts={} lod0_tris={}",
                file.path,
                mesh.lod_count,
                mesh.lod.positions.len(),
                mesh.lod.triangle_count()
            );
            shown += 1;
            if shown >= 12 {
                break;
            }
        }
    }
    println!("multi_lod_found={shown}");
    Ok(())
}
