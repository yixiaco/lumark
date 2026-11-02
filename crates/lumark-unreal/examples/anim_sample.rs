//! 诊断：按秒采样 AnimSequence，输出蒙皮矩阵与 CPU 蒙皮位移。
//!
//! 用法：`anim_sample <game-root> <container> <mesh-entry> <anim-entry> <mapping.usmap>`

use std::error::Error;

use lumark_unreal::{
    GameAssetOptions, GameAssetSource, Mappings, extract_key_for_game, parse_package,
    read_anim_sequence, sample_animation, scan_game, skin_mesh_animation,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mesh_entry = args.next().ok_or("缺少 SkeletalMesh 路径")?;
    let anim_entry = args.next().ok_or("缺少 AnimSequence 路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
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
    let mappings = Mappings::parse_file(std::path::Path::new(&mapping_path))?;
    let mut source = GameAssetSource::open(
        &container_path,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(mapping_path.into()),
        },
    )?;

    let mesh_inputs = source.read_package(&mesh_entry)?;
    let mesh_package = parse_package(mesh_inputs.uasset, source.scripts(), None)?;
    let mesh_index = mesh_package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "SkeletalMesh")
        .ok_or("未找到 SkeletalMesh 导出")?;
    let preview = mesh_package.read_skeletal_mesh_preview(
        mesh_index,
        &mappings,
        mesh_inputs.ubulk.as_deref(),
    )?;
    let skeleton = &preview.skeleton;
    let mesh = preview.geometry.as_ref().ok_or("SkeletalMesh 缺少几何")?;
    let weights = preview.skin.as_ref().ok_or("SkeletalMesh 缺少蒙皮权重")?;

    let anim_inputs = source.read_package(&anim_entry)?;
    let anim_package = parse_package(anim_inputs.uasset, source.scripts(), None)?;
    let anim_index = anim_package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "AnimSequence")
        .ok_or("未找到 AnimSequence 导出")?;
    let animation = read_anim_sequence(&anim_package, anim_index, &mappings)?;
    println!(
        "anim={} length={:.3}s frames={} tracks={} bones={} verts={}",
        animation.name,
        animation.length,
        animation.frames,
        animation.track_to_bone.len(),
        skeleton.bones.len(),
        mesh.positions.len()
    );

    for time in [0.0f32, animation.length * 0.5, animation.length] {
        let sample = sample_animation(skeleton, &animation, time);
        let posed = skin_mesh_animation(mesh, skeleton, weights, &animation, time);
        let max_delta = posed
            .positions
            .iter()
            .zip(mesh.positions.iter())
            .map(|(a, b)| {
                let dx = a[0] - b[0];
                let dy = a[1] - b[1];
                let dz = a[2] - b[2];
                (dx * dx + dy * dy + dz * dz).sqrt() as f32
            })
            .fold(0.0f32, f32::max);
        println!(
            "t={:.3} ratio={:.3} poses={} matrices={} normals={} max_delta={:.3}",
            sample.time,
            sample.ratio,
            sample.poses.len(),
            sample.skin_matrices.len(),
            posed.attributes.normals.len(),
            max_delta
        );
    }

    // 越界时间应被钳制。
    let before = sample_animation(skeleton, &animation, -10.0);
    let after = sample_animation(skeleton, &animation, animation.length + 10.0);
    println!(
        "clamp before=({:.3},{:.3}) after=({:.3},{:.3})",
        before.time, before.ratio, after.time, after.ratio
    );
    Ok(())
}
