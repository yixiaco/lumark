//! 诊断：通过 `GameAssetSource` 读取真实游戏容器中的资产包。
//!
//! 用法：`inspect_source <game-root> <container> <entry> [mapping.usmap]`
//! 不打印完整 AES 密钥。

use std::error::Error;
use std::path::PathBuf;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, extract_key_for_game, parse_package, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少资产路径")?;
    let mapping_path = args.next();

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
    let options = GameAssetOptions {
        key: Some(key.as_str()),
        global,
        oodle: None,
        mappings: mapping_path.clone().map(PathBuf::from),
    };
    let mut source = GameAssetSource::open(&container_path, options)?;
    println!(
        "kind={:?} entries={} encrypted={} needs_oodle={} methods={:?} warnings={:?}",
        source.kind(),
        source.entry_count(),
        source.encrypted(),
        source.needs_oodle(),
        source.compression_methods(),
        source.warnings()
    );

    let inputs = source.read_package(&entry)?;
    println!(
        "uasset={} uexp={:?} ubulk={:?}",
        inputs.uasset.len(),
        inputs.uexp.as_ref().map(Vec::len),
        inputs.ubulk.as_ref().map(Vec::len)
    );
    if let Some(path) = mapping_path {
        let mappings = Mappings::parse_file(std::path::Path::new(&path))?;
        let package = parse_package(inputs.uasset, source.scripts(), None)?;
        println!(
            "package={} exports={} structs={}",
            package.info().package_name,
            package.info().exports.len(),
            mappings.structs.len()
        );
    }
    Ok(())
}
