//! 诊断：在加密 IoStore 容器上读取 ContainerHeader 与包 ID（密钥仅内存）。
//!
//! 用法：`iostore_header_probe <game-root> <container-file> [package-name]`。

use std::error::Error;

use lumark_archive::{IoStoreContainer, package_id};
use lumark_unreal::{Mappings, ZenScriptObjects, extract_key_for_game, parse_package, scan_game};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let package_name = args.next();
    let game = scan_game(std::path::Path::new(&root))?;
    let container = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == container_name)
        })
        .ok_or("未找到指定容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到 AES 密钥")?;
    let container = IoStoreContainer::open(&container.path, Some(key.as_str()))?;
    println!(
        "toc: entries={} encrypted={} compression={:?}",
        container.entry_count(),
        container.encrypted(),
        container.compression_methods()
    );
    let header = container.container_header()?;
    println!(
        "container header: version={} id=0x{:016X} packages={}",
        header.version,
        header.container_id,
        header.packages.len()
    );
    if let Some(name) = package_name {
        let id = package_id(&name);
        println!("package name: {name}");
        println!("package id: 0x{id:016X}");
        if let Some(package) = header.package(id) {
            println!("imported packages: {}", package.imported_packages.len());
        } else {
            println!("store entry: not found");
        }
        println!(
            "ExportBundleData chunks: {}",
            container.find_package_chunks(id, 1).len()
        );
        println!(
            "BulkData chunks: {}",
            container.find_package_chunks(id, 2).len()
        );
        // 用目录索引反查真实包 ID，帮助确认 UE 包名。
        let file = container
            .files()
            .iter()
            .find(|file| file.path.ends_with(&name) || file.path.contains(&name));
        if let Some(file) = file {
            let actual = container
                .package_id(file.toc_index)
                .ok_or("目录条目没有 package id")?;
            println!(
                "directory: {} toc={} package_id=0x{actual:016X}",
                file.path, file.toc_index
            );
            println!("header has actual id: {}", header.package(actual).is_some());
            let data = container.read_package_by_id(actual)?;
            println!("package data bytes: {}", data.len());
            let global_path = container
                .toc_path()
                .parent()
                .map(|parent| parent.join("global.utoc"))
                .ok_or("没有 global.utoc")?;
            let scripts = ZenScriptObjects::from_container(&global_path, Some(key.as_str()))?;
            let package = parse_package(data, Some(&scripts), None)?;
            let info = package.info();
            println!(
                "zen package: name={} exports={} imports={} imported_public_hashes={}",
                info.package_name,
                info.exports.len(),
                info.import_map.len(),
                info.imported_public_export_hashes.len()
            );
            if let Some(export) = info
                .exports
                .iter()
                .find(|export| export.class_name == "World")
            {
                println!(
                    "world export: name={} index={} public_hash=0x{:016X}",
                    export.name,
                    info.exports
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, export))
                        .unwrap_or(0),
                    export.public_export_hash
                );
            }
            for (index, import) in info.import_map.iter().take(5).enumerate() {
                println!("import[{index}] = 0x{import:016X}");
            }
            let mapping_path = game
                .mapping_candidates
                .iter()
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "usmap")
                })
                .ok_or("没有 .usmap")?;
            let mappings = Mappings::parse_file(mapping_path)?;
            let world = package.read_world_summary(&mappings)?;
            println!(
                "world: actors={} persistent={:?}",
                world.actor_count, world.persistent_level_name
            );
            for actor in world
                .actors
                .iter()
                .filter(|actor| actor.class_name == "StaticMeshActor")
                .take(2)
            {
                println!("actor {} export={}", actor.name, actor.export_index);
                if let Ok(properties) =
                    package.read_export_properties(actor.export_index, &mappings)
                {
                    for (key, value) in properties.properties.iter().take(24) {
                        let summary = match value {
                            lumark_unreal::PropValue::Object(index) => format!("Object({index})"),
                            lumark_unreal::PropValue::Name(name) => format!("Name({name})"),
                            lumark_unreal::PropValue::Struct(fields) => {
                                format!("Struct({} fields)", fields.len())
                            }
                            other => format!("{other:?}"),
                        };
                        println!("  {key} = {summary}");
                    }
                    if let Some(lumark_unreal::PropValue::Object(component)) = properties
                        .properties
                        .iter()
                        .find(|(key, _)| key == "StaticMeshComponent")
                        .map(|(_, value)| value)
                        && *component > 0
                    {
                        let component_index = (*component - 1) as usize;
                        println!("  component export={component_index}");
                        if let Ok(component_properties) =
                            package.read_export_properties(component_index, &mappings)
                        {
                            for (key, value) in component_properties.properties.iter() {
                                let summary = match value {
                                    lumark_unreal::PropValue::Object(index) => {
                                        format!("Object({index})")
                                    }
                                    lumark_unreal::PropValue::Struct(fields) => fields
                                        .iter()
                                        .filter_map(|(name, value)| match value {
                                            lumark_unreal::PropValue::Float(number) => {
                                                Some(format!("{name}={number:.3}"))
                                            }
                                            _ => None,
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                    other => format!("{other:?}"),
                                };
                                println!("    {key} = {summary}");
                            }
                            // 解析 StaticMesh 的 FPackageObjectIndex 到外部包导出。
                            if let Some(lumark_unreal::PropValue::Object(mesh_object)) =
                                component_properties
                                    .properties
                                    .iter()
                                    .find(|(key, _)| key == "StaticMesh")
                                    .map(|(_, value)| value)
                                && *mesh_object < 0
                            {
                                let import_index = (-*mesh_object - 1) as usize;
                                if let Some(raw) = info.import_map.get(import_index) {
                                    let kind = raw >> 62;
                                    println!(
                                        "    resolve import[{import_index}] raw=0x{raw:016X} kind={kind}"
                                    );
                                    if kind == 2 {
                                        let package_index =
                                            ((raw & ((1u64 << 62) - 1)) >> 32) as usize;
                                        let hash_index = (raw & 0xffff_ffff) as usize;
                                        let imported_ids = header
                                            .package(actual)
                                            .map(|package| package.imported_packages.clone())
                                            .unwrap_or_default();
                                        println!(
                                            "    package_index={package_index} hash_index={hash_index} imported_ids={}",
                                            imported_ids.len()
                                        );
                                        if let (Some(target_package), Some(hash)) = (
                                            imported_ids.get(package_index),
                                            info.imported_public_export_hashes.get(hash_index),
                                        ) {
                                            println!(
                                                "    target package=0x{target_package:016X} public_hash=0x{hash:016X}"
                                            );
                                            if let Ok(target_data) =
                                                container.read_package_by_id(*target_package)
                                                && let Ok(target_package) =
                                                    parse_package(target_data, Some(&scripts), None)
                                            {
                                                let export = target_package
                                                    .info()
                                                    .exports
                                                    .iter()
                                                    .enumerate()
                                                    .find(|(_, export)| {
                                                        export.public_export_hash == *hash
                                                    });
                                                match export {
                                                    Some((index, export)) => println!(
                                                        "    resolved export[{index}] {} class={} public=0x{:016X}",
                                                        export.name,
                                                        export.class_name,
                                                        export.public_export_hash
                                                    ),
                                                    None => println!(
                                                        "    resolved export: not found (exports={})",
                                                        target_package.info().exports.len()
                                                    ),
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for candidate in [
                file.path.clone(),
                file.path
                    .strip_suffix(".umap")
                    .or_else(|| file.path.strip_suffix(".uasset"))
                    .unwrap_or(&file.path)
                    .to_string(),
                "/Game/KingS/GinkgoTree".to_string(),
                "Game/KingS/GinkgoTree".to_string(),
                "TogetherBNB/Content/KingS/GinkgoTree".to_string(),
            ] {
                let hash = package_id(&candidate);
                println!(
                    "candidate {candidate:?} -> 0x{hash:016X}{}",
                    if hash == actual { " MATCH" } else { "" }
                );
            }
        } else {
            println!("directory: no matching file");
        }
    }
    Ok(())
}
