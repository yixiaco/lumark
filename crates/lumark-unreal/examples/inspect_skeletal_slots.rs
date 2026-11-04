//! 诊断：打印 SkeletalMesh 的 SkeletalMaterials 记录布局（44 字节/项）。
//!
//! 用法：`inspect_skeletal_slots <game-root> <container> <mapping.usmap> <mesh-entry>`
//!
//! 用于确认材质槽结构里 MaterialInterface 的 FPackageIndex 偏移，以便把
//! 材质槽解析从“按名字模糊匹配”升级为“按对象引用精确解析”。

use std::error::Error;
use std::path::Path;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, extract_key_for_game, parse_package, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let entry = args.next().ok_or("缺少 SkeletalMesh 资产路径")?;

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
    let package_id = source.package_id_for_path(&entry);
    let _ = package_id;
    let inputs = source.read_package(&entry)?;
    let package = parse_package(inputs.uasset, source.scripts(), None)?;
    let exports = package.info().exports.clone();
    let index = exports
        .iter()
        .position(|export| export.class_name == "SkeletalMesh")
        .ok_or("未找到 SkeletalMesh 导出")?;
    let properties = package.read_export_properties(index, &mappings)?;
    let data = package.export_data(index).ok_or("导出数据越界")?;
    let bytes_read = properties.bytes_read;
    println!("bytes_read={bytes_read} data_len={}", data.len());
    let read_i32 = |at: usize| -> i32 {
        data.get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(i32::from_le_bytes)
            .unwrap_or(i32::MIN)
    };
    let valid_name = |at: usize| -> Option<String> {
        let name_index = read_i32(at);
        let number = read_i32(at + 4);
        if name_index < 0 || !(0..=64).contains(&number) {
            return None;
        }
        package
            .names()
            .get(name_index as usize)
            .filter(|name| !name.is_empty() && name.as_str() != "None")
            .cloned()
    };
    // 枚举可能的 SkeletalMaterials 起点：数量 1..64 且每条记录 44 字节，
    // 记录内 +8 的 FName 索引合法。用于确认结构偏移，不参与正式解析。
    for candidate in bytes_read..(bytes_read + 4096).min(data.len().saturating_sub(8)) {
        let count = read_i32(candidate);
        if !(1..=64).contains(&count) {
            continue;
        }
        let start = candidate + 4;
        let end = start + count as usize * 44;
        if end > data.len() {
            continue;
        }
        let mut names = Vec::new();
        let mut ok = true;
        for item in 0..count as usize {
            let record = start + item * 44;
            match valid_name(record + 8) {
                Some(name) => names.push(name),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || names.is_empty() {
            continue;
        }
        println!(
            "候选起点 offset={candidate} count={count} 首槽={:?} 末槽={:?}",
            names.first(),
            names.last()
        );
        for (item, name) in names.iter().enumerate().take(20) {
            let record = start + item * 44;
            let index0 = read_i32(record);
            let index4 = read_i32(record + 4);
            let index8 = read_i32(record + 8);
            println!("  slot[{item}] name={name} +0={index0} +4={index4} +8={index8}");
        }
        break;
    }
    Ok(())
}
