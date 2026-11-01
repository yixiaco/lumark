//! 手工诊断 UE5 材质：读取导出属性，寻找可用的外观参数。
//!
//! 用法：`zen_material <utoc> <aes-key|-> <global.utoc> <entry> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{
    Mappings, PropValue, ZenScriptObjects, extract_key_for_game, parse_package,
    read_zen_material_appearance, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let entry = args.next().ok_or("缺少包路径")?;
    let mapping = args.next().ok_or("缺少 .usmap")?;
    let game = scan_game(std::path::Path::new(&root))?;
    let key_result = extract_key_for_game(&game, None, |_, _, _| {});
    let key = key_result.key.as_ref().map(|key| key.as_str());
    let toc = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .to_string_lossy()
                .ends_with("pakchunk0-Windows.utoc")
        })
        .map(|container| container.path.clone())
        .ok_or("缺少 pakchunk0-Windows.utoc")?
        .to_string_lossy()
        .to_string();
    let global = game
        .containers
        .iter()
        .find(|container| container.path.to_string_lossy().ends_with("global.utoc"))
        .map(|container| container.path.clone())
        .ok_or("缺少 global.utoc")?
        .to_string_lossy()
        .to_string();
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let container = IoStoreContainer::open(&toc, key)?;
    let scripts = ZenScriptObjects::from_container(Path::new(&global), key)?;
    let data = container.read_file(&entry)?;
    let package = parse_package(data, Some(&scripts), None)?;
    println!(
        "package={} class={} exports={}",
        package.info().package_name,
        package
            .info()
            .exports
            .first()
            .map(|export| export.class_name.as_str())
            .unwrap_or_default(),
        package.info().exports.len()
    );
    for (index, export) in package.info().exports.iter().enumerate() {
        println!("export{index}: {} ({})", export.name, export.class_name);
    }
    let appearance = read_zen_material_appearance(&package, &mappings);
    println!(
        "appearance base_color={:?} metallic={} roughness={} textures={:?}",
        appearance.base_color, appearance.metallic, appearance.roughness, appearance.textures
    );
    for index in 0..package.info().exports.len() {
        match package.read_export_properties(index, &mappings) {
            Ok(properties) => {
                println!("properties {index} bytes_read={}", properties.bytes_read);
                for (name, value) in &properties.properties {
                    println!("  {name} = {}", describe(value));
                }
            }
            Err(error) => println!("properties {index} error: {error}"),
        }
    }
    Ok(())
}

/// 简化格式化属性值。
fn describe(value: &PropValue) -> String {
    match value {
        PropValue::Array(values) => {
            let shown = values.iter().take(4).map(describe).collect::<Vec<_>>();
            format!("[{} 项: {}]", values.len(), shown.join(", "))
        }
        PropValue::Struct(fields) => {
            let shown = fields
                .iter()
                .take(8)
                .map(|(name, value)| format!("{name}={}", describe(value)))
                .collect::<Vec<_>>();
            format!("{{{}}}", shown.join(", "))
        }
        other => format!("{other:?}"),
    }
}
