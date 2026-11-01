//! 手工诊断：解码 FF7R 旧式 AnimSequence 并打印采样姿态。
//!
//! 用法：`legacy_anim_play <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings, read_legacy_anim_sequence};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("missing .uasset")?;
    let uexp = args.next().ok_or("missing .uexp")?;
    let mapping = args.next().ok_or("missing .usmap")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    let animation = read_legacy_anim_sequence(&package, 0, &mappings)?;
    println!(
        "name={} length={} frames={} tracks={}",
        animation.name,
        animation.length,
        animation.frames,
        animation.track_to_bone.len()
    );
    for ratio in [0.0f32, 0.25, 0.5, 1.0] {
        let poses = animation.sample(ratio);
        println!("--- ratio={ratio} poses={}", poses.len());
        for (track, pose) in poses.iter().take(3).enumerate() {
            println!(
                "track{track} bone={} t={:?} r={:?} s={:?}",
                animation.track_to_bone[track], pose.translation, pose.rotation, pose.scale
            );
        }
    }
    // 对比首尾帧，确认数据确实随动画时间变化。
    let first = animation.sample(0.0);
    let last = animation.sample(1.0);
    let mut changed = 0usize;
    for (a, b) in first.iter().zip(last.iter()) {
        if a.translation != b.translation || a.rotation != b.rotation {
            changed += 1;
        }
    }
    println!("changed_tracks={changed}");
    Ok(())
}
