//! 手工诊断：用真实旧式 SkeletalMesh + AnimSequence 验证蒙皮播放链路。
//!
//! 用法：`legacy_skin_play <mesh.uasset> <mesh.uexp> <anim.uasset> <anim.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{
    LegacyPackage, Mappings, read_legacy_anim_sequence, read_legacy_skeletal_mesh_geometry,
    skinning::{animation_poses, skin_mesh_pose},
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let mesh_asset = args.next().ok_or("missing mesh.uasset")?;
    let mesh_uexp = args.next().ok_or("missing mesh.uexp")?;
    let anim_asset = args.next().ok_or("missing anim.uasset")?;
    let anim_uexp = args.next().ok_or("missing anim.uexp")?;
    let mapping = args.next().ok_or("missing .usmap")?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;

    let mesh_package =
        LegacyPackage::parse(&std::fs::read(mesh_asset)?, &std::fs::read(mesh_uexp)?)?;
    let geometry = read_legacy_skeletal_mesh_geometry(&mesh_package, 0, &mappings)?;
    println!(
        "mesh bones={} verts={} tris={} skin={} lods={}",
        geometry.skeleton.bones.len(),
        geometry.mesh.positions.len(),
        geometry.mesh.indices.len() / 3,
        geometry.skin.is_some(),
        geometry.lod_count
    );

    let anim_package =
        LegacyPackage::parse(&std::fs::read(anim_asset)?, &std::fs::read(anim_uexp)?)?;
    let animation = read_legacy_anim_sequence(&anim_package, 0, &mappings)?;
    println!(
        "anim={} tracks={} frames={} length={:.3}",
        animation.name,
        animation.track_to_bone.len(),
        animation.frames,
        animation.length
    );

    let Some(skin) = geometry.skin.as_ref() else {
        println!("网格没有蒙皮权重，无法演示动画");
        return Ok(());
    };
    let rest = geometry.mesh.clone();
    let mut max_delta = 0.0f32;
    for ratio in [0.0f32, 0.5, 1.0] {
        let poses = animation_poses(&geometry.skeleton, &animation, ratio);
        let posed = skin_mesh_pose(&rest, &geometry.skeleton, skin, &poses);
        let delta = posed
            .positions
            .iter()
            .zip(rest.positions.iter())
            .map(|(a, b)| {
                let dx = a[0] - b[0];
                let dy = a[1] - b[1];
                let dz = a[2] - b[2];
                (dx * dx + dy * dy + dz * dz).sqrt() as f32
            })
            .fold(0.0f32, f32::max);
        max_delta = max_delta.max(delta);
        println!("ratio={ratio} max_vertex_delta={delta:.3}");
    }
    println!("max_delta={max_delta:.3}");
    Ok(())
}
