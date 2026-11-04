//! 诊断：打印 Zen 材质/材质实例的属性图、父材质链与贴图对象引用。
//!
//! 用法：`inspect_material_graph <game-root> <container> <mapping.usmap> <material-entry> [more...]`
//!
//! 输出每个材质的包 ID、导出类、属性键、TextureParameterValues / Parent /
//! CachedExpressionData 的形状，以及导入对象解析出的目标包/导出名，用于实现
//! 完整材质图还原时对照。

use std::error::Error;
use std::path::Path;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, PackageInfo, PropValue, UnrealPackage,
    extract_key_for_game, parse_package, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let entries = args.collect::<Vec<_>>();
    if entries.is_empty() {
        return Err("缺少材质资产路径".into());
    }

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

    for entry in &entries {
        println!("==== {entry}");
        let package_id = source.package_id_for_path(entry);
        println!("package_id={package_id:?}");
        let Ok(inputs) = source.read_package(entry) else {
            println!("读取包失败");
            continue;
        };
        let Ok(package) = parse_package(inputs.uasset, source.scripts(), None) else {
            println!("解析包失败");
            continue;
        };
        let imported_ids = package_id
            .and_then(|id| source.imported_packages(id).ok())
            .unwrap_or_default();
        inspect_package(&mut source, &package, &imported_ids, &mappings, 0);
    }
    Ok(())
}

/// 打印一个材质包的导出与属性，并递归父材质。
fn inspect_package(
    source: &mut GameAssetSource,
    package: &UnrealPackage,
    imported_ids: &[u64],
    mappings: &Mappings,
    depth: usize,
) {
    if depth > 4 {
        println!("  [depth 限制]");
        return;
    }
    let info = package.info();
    for (index, export) in info.exports.iter().enumerate() {
        println!(
            "  export[{index}] name={} class={} kind={:?} hash=0x{:016X}",
            export.name, export.class_name, export.kind, export.public_export_hash
        );
        if !(export.class_name.to_ascii_lowercase().contains("material")) {
            continue;
        }
        let Ok(properties) = package.read_export_properties(index, mappings) else {
            println!("    属性读取失败");
            continue;
        };
        let keys = properties
            .properties
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>();
        println!("    属性键: {keys:?}");
        for (key, value) in &properties.properties {
            match key.as_str() {
                "Parent" => {
                    println!("    Parent 原始={value:?}");
                    if let PropValue::Object(import_index) = value
                        && *import_index < 0
                    {
                        let import_index = (-*import_index - 1) as usize;
                        match resolve_import(info, imported_ids, import_index) {
                            Some((package_id, hash)) => {
                                println!(
                                    "    Parent -> package_id=0x{package_id:016X} hash=0x{hash:016X}"
                                );
                                inspect_target(source, package_id, hash, mappings, depth + 1);
                            }
                            None => println!("    Parent 解析失败（import={import_index}）"),
                        }
                    }
                }
                "TextureParameterValues" => {
                    println!(
                        "    TextureParameterValues={}",
                        describe_parameter_entries(source, value, info, imported_ids,)
                    );
                }
                "VectorParameterValues" | "ScalarParameterValues" => {
                    println!(
                        "    {key}={}",
                        describe_parameter_entries(source, value, info, imported_ids,)
                    );
                }
                "CachedExpressionData" | "StaticParameters" | "StaticParametersRuntime" => {
                    println!("    {key}={}", describe_value(value, 0));
                }
                _ => {}
            }
        }
    }
}

/// 读取目标包并打印目标导出的类名；材质则继续展开父链。
fn inspect_target(
    source: &mut GameAssetSource,
    package_id: u64,
    hash: u64,
    mappings: &Mappings,
    depth: usize,
) {
    let Ok((uasset, _)) = source.read_package_by_id(package_id) else {
        println!("      读取目标包失败");
        return;
    };
    let Ok(package) = parse_package(uasset, source.scripts(), None) else {
        println!("      解析目标包失败");
        return;
    };
    let Some(export) = package
        .info()
        .exports
        .iter()
        .find(|export| export.public_export_hash == hash)
    else {
        println!(
            "      目标包内未找到 hash 对应导出（exports={}）",
            package.info().exports.len()
        );
        return;
    };
    println!(
        "      目标导出 name={} class={}",
        export.name, export.class_name
    );
    let imported_ids = source.imported_packages(package_id).unwrap_or_default();
    inspect_package(source, &package, &imported_ids, mappings, depth);
}

/// 描述参数数组条目（名称 + 引用对象）。
fn describe_parameter_entries(
    source: &mut GameAssetSource,
    value: &PropValue,
    info: &PackageInfo,
    imported_ids: &[u64],
) -> String {
    let PropValue::Array(entries) = value else {
        return format!("{value:?}");
    };
    let mut parts = Vec::new();
    for entry in entries.iter().take(24) {
        let PropValue::Struct(fields) = entry else {
            continue;
        };
        let name = fields
            .iter()
            .find(|(key, _)| key == "ParameterInfo")
            .and_then(|(_, value)| match value {
                PropValue::Struct(info_fields) => info_fields
                    .iter()
                    .find(|(key, _)| key == "Name")
                    .and_then(|(_, value)| match value {
                        PropValue::Name(name) | PropValue::String(name) => Some(name.clone()),
                        _ => None,
                    }),
                _ => None,
            })
            .unwrap_or_default();
        let object = fields
            .iter()
            .find(|(key, _)| key == "ParameterValue")
            .map(|(_, value)| value);
        match object {
            Some(PropValue::Object(index)) if *index < 0 => {
                let import_index = (-*index - 1) as usize;
                match resolve_import(info, imported_ids, import_index) {
                    Some((package_id, hash)) => {
                        let target = read_target_name(source, package_id, hash);
                        parts.push(format!("{name}->{target}(0x{package_id:016X})"));
                    }
                    None => parts.push(format!("{name}->import({import_index})")),
                }
            }
            Some(PropValue::Struct(fields)) => {
                // 标量/向量参数值是内联结构，打印字段数。
                parts.push(format!("{name}->struct{}", fields.len()));
            }
            other => parts.push(format!("{name}->{other:?}")),
        }
    }
    format!("[{}] {}", parts.len(), parts.join(", "))
}

/// 读取目标包内指定 hash 导出的名字与类名。
fn read_target_name(source: &mut GameAssetSource, package_id: u64, hash: u64) -> String {
    let Ok((uasset, _)) = source.read_package_by_id(package_id) else {
        return "读取失败".to_string();
    };
    let Ok(package) = parse_package(uasset, source.scripts(), None) else {
        return "解析失败".to_string();
    };
    package
        .info()
        .exports
        .iter()
        .find(|export| export.public_export_hash == hash)
        .map(|export| format!("{}/{}", export.class_name, export.name))
        .unwrap_or_else(|| "未找到导出".to_string())
}

/// 解析 `FPackageIndex` 导入（与 world_scene 中的实现一致）。
fn resolve_import(
    info: &PackageInfo,
    imported_ids: &[u64],
    import_index: usize,
) -> Option<(u64, u64)> {
    let raw = *info.import_map.get(import_index)?;
    if raw == u64::MAX || (raw >> 62) != 2 {
        return None;
    }
    let value = raw & ((1u64 << 62) - 1);
    let package_index = (value >> 32) as usize;
    let hash_index = (value & 0xffff_ffff) as usize;
    let package_id = *imported_ids.get(package_index)?;
    let hash = *info.imported_public_export_hashes.get(hash_index)?;
    Some((package_id, hash))
}

/// 递归描述属性值，限制深度与元素数量。
fn describe_value(value: &PropValue, depth: usize) -> String {
    if depth > 3 {
        return "...".to_string();
    }
    match value {
        PropValue::Struct(fields) => {
            let parts = fields
                .iter()
                .take(24)
                .map(|(key, value)| format!("{key}:{}", describe_value(value, depth + 1)))
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(", "))
        }
        PropValue::Array(values) => format!(
            "Array[{}]",
            values
                .iter()
                .take(8)
                .map(|value| describe_value(value, depth + 1))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    }
}
