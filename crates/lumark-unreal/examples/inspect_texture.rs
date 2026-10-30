//! 手工诊断 UTexture2D：读取 IoStore 条目、`.usmap` 并输出尺寸/mip。
//!
//! 用法：`inspect_texture <utoc> <key|-> <global.utoc> <entry> <mapping.usmap> [ubulk-entry]`。

use std::error::Error;

use lumark_archive::IoStoreContainer;
use lumark_unreal::{Mappings, ZenScriptObjects, parse_package};

/// 读取并打印纹理概况。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let toc = args.next().ok_or("缺少 .utoc")?;
    let key = args.next().unwrap_or_else(|| "-".to_string());
    let global = args.next().ok_or("缺少 global.utoc")?;
    let entry = args.next().ok_or("缺少包路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let ubulk_entry = args.next().filter(|value| value != "-");
    let output = args.next();
    let key = (key != "-").then_some(key.as_str());
    let container = IoStoreContainer::open(&toc, key)?;
    let scripts = ZenScriptObjects::from_container(std::path::Path::new(&global), key)?;
    let data = container.read_file(&entry)?;
    let package = parse_package(data, Some(&scripts), None)?;
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let ubulk = ubulk_entry
        .map(|path| container.read_file(&path))
        .transpose()?;
    let texture = package.read_texture2d(0, &mappings, ubulk.as_deref())?;
    println!(
        "texture: {} {}x{} format={} mips={}",
        texture.name,
        texture.width,
        texture.height,
        texture.pixel_format,
        texture.mips.len()
    );
    for (index, mip) in texture.mips.iter().enumerate() {
        println!(
            "  mip {index}: {}x{}x{} bytes={} first={:02X?}",
            mip.width,
            mip.height,
            mip.depth,
            mip.data.len(),
            &mip.data[..mip.data.len().min(24)]
        );
    }
    if let Some(output) = output {
        let rgba = texture.decode_mip_rgba(0)?;
        let image = image::RgbaImage::from_raw(texture.width as u32, texture.height as u32, rgba)
            .ok_or("RGBA 图像尺寸不匹配")?;
        image.save(&output)?;
        println!("saved {output}");
    }
    Ok(())
}
