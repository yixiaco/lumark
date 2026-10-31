//! 手工验收：解码 AnimSequence 并驱动 SkeletalMesh LBS 蒙皮。
//!
//! 用法：`anim_play <game-root> <mapping.usmap> <oodle-dll> <container> <mesh-entry> <anim-entry> <out.png>`

use std::error::Error;
use std::path::Path;

use lumark_archive::{IoStoreContainer, configure_oodle_library};
use lumark_unreal::skinning::{BonePose, skin_mesh_pose};
use lumark_unreal::{
    Mappings, ZenScriptObjects, extract_key_for_game, parse_package, read_anim_sequence, scan_game,
};

/// 运行验收。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let oodle = args.next().ok_or("缺少 Oodle DLL")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mesh_entry = args.next().ok_or("缺少 SkeletalMesh 路径")?;
    let anim_entry = args.next().ok_or("缺少 AnimSequence 路径")?;
    let output = args.next().ok_or("缺少输出 PNG")?;

    configure_oodle_library(Some(oodle.into())).map_err(|error| format!("Oodle：{error}"))?;
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
    let container = IoStoreContainer::open(&container_path, Some(key.as_str()))?;
    let global = container_path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let scripts = global
        .as_deref()
        .map(|path| ZenScriptObjects::from_container(path, Some(key.as_str())))
        .transpose()?;
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;

    let mesh_data = container.read_file(&mesh_entry)?;
    let mesh_package = parse_package(mesh_data, scripts.as_ref(), None)?;
    let mesh_index = mesh_package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "SkeletalMesh")
        .ok_or("未找到 SkeletalMesh 导出")?;
    let ubulk = mesh_entry
        .strip_suffix(".uasset")
        .map(|stem| format!("{stem}.ubulk"))
        .and_then(|path| container.read_file(&path).ok());
    let preview =
        mesh_package.read_skeletal_mesh_preview(mesh_index, &mappings, ubulk.as_deref())?;
    let mesh = preview.geometry.ok_or("SkeletalMesh 缺少几何")?;
    let weights = preview.skin.ok_or("SkeletalMesh 缺少蒙皮权重")?;

    let anim_data = container.read_file(&anim_entry)?;
    let anim_package = parse_package(anim_data, scripts.as_ref(), None)?;
    let anim_index = anim_package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "AnimSequence")
        .ok_or("未找到 AnimSequence 导出")?;
    let animation = read_anim_sequence(&anim_package, anim_index, &mappings)?;
    println!(
        "anim={} len={:.3}s frames={} tracks={}",
        animation.name,
        animation.length,
        animation.frames,
        animation.track_to_bone.len()
    );

    for (ratio, suffix) in [
        (0.0f32, "_0"),
        (0.25f32, "_25"),
        (0.5f32, "_50"),
        (0.75f32, "_75"),
        (1.0f32, "_100"),
    ] {
        let tracks = animation.sample(ratio);
        let mut poses: Vec<BonePose> = preview
            .skeleton
            .bones
            .iter()
            .map(|bone| BonePose {
                translation: bone.translation,
                rotation: bone.rotation,
                scale: [1.0; 3],
            })
            .collect();
        for (track, pose) in tracks.iter().enumerate() {
            let Some(bone) = animation
                .track_to_bone
                .get(track)
                .and_then(|bone| usize::try_from(*bone).ok())
            else {
                continue;
            };
            let Some(target) = poses.get_mut(bone) else {
                continue;
            };
            *target = BonePose {
                translation: [
                    pose.translation[0] as f64,
                    pose.translation[1] as f64,
                    pose.translation[2] as f64,
                ],
                rotation: [
                    pose.rotation[0] as f64,
                    pose.rotation[1] as f64,
                    pose.rotation[2] as f64,
                    pose.rotation[3] as f64,
                ],
                scale: [1.0; 3],
            };
        }
        let posed = skin_mesh_pose(&mesh, &preview.skeleton, &weights, &poses);
        let rgba = lumark_unreal::render_static_mesh_orbit(&posed, 480, 360, 35.0, 25.0, 1.0);
        if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
            let path = format!("{}{}.png", output.trim_end_matches(".png"), suffix);
            image.save(&path)?;
            println!("ratio={ratio} saved={path}");
        }
    }
    Ok(())
}
