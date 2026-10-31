//! 手工诊断：不依赖 `.usmap` 解码 FF7R 旧式纹理基础 mip。
//!
//! 用法：`legacy_texture <asset.uasset> <asset.uexp> <asset.ubulk> <out.png>`

use std::error::Error;

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("缺少 .uasset")?;
    let uexp = args.next().ok_or("缺少 .uexp")?;
    let ubulk = args.next().ok_or("缺少 .ubulk")?;
    let output = args.next().ok_or("缺少输出 PNG")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let ubulk_data = std::fs::read(ubulk)?;
    let texture = lumark_unreal::read_legacy_texture(&asset_data, &uexp_data, Some(&ubulk_data))?;
    println!(
        "texture={} {}x{} format={} mips={}",
        texture.name,
        texture.width,
        texture.height,
        texture.pixel_format,
        texture.mips.len()
    );
    let rgba = texture.decode_mip_rgba(0)?;
    let image = image::RgbaImage::from_raw(texture.width as u32, texture.height as u32, rgba)
        .ok_or("图像尺寸不匹配")?;
    image.save(&output)?;
    println!("saved {output}");
    Ok(())
}
