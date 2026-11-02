//! 诊断：把 SkeletalMesh + AnimSequence 导出为带 SkelAnimation 的 USDA。
//!
//! 用法：`export_usda_anim <game-root> <container> <mesh> <anim> <mapping.usmap> <out.usda>`

use std::error::Error;
use std::path::Path;

use lumark_export::{
    ExportOptions, UsdaAnimKey, UsdaAnimation, write_resources, write_usda_with_animation,
};
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, animation_poses, build_mesh_export,
    extract_key_for_game, parse_package, read_anim_sequence, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mesh_entry = args.next().ok_or("缺少 SkeletalMesh 资产路径")?;
    let anim_entry = args.next().ok_or("缺少 AnimSequence 资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出 .usda")?;

    let game = scan_game(Path::new(&root))?;
    let container = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == container_name)
        })
        .ok_or("未找到容器")?
        .path
        .clone();
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到密钥")?;
    let global = container
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;
    let mut source = GameAssetSource::open(
        &container,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(mapping_path.into()),
        },
    )?;

    // 骨架。
    let mesh_inputs = source.read_package(&mesh_entry)?;
    let mesh_package = parse_package(mesh_inputs.uasset, source.scripts(), None)?;
    let mesh_index = mesh_package
        .info()
        .exports
        .iter()
        .position(|export| {
            export.kind == AssetKind::SkeletalMesh && export.class_name == "SkeletalMesh"
        })
        .ok_or("未找到 SkeletalMesh 导出")?;
    let preview = mesh_package.read_skeletal_mesh_preview(
        mesh_index,
        &mappings,
        mesh_inputs.ubulk.as_deref(),
    )?;
    let skeleton = preview.skeleton.clone();
    let bundle = build_mesh_export(&mut source, &mesh_entry, &mappings, true, 512)?;

    // 动画。
    let anim_inputs = source.read_package(&anim_entry)?;
    let anim_package = parse_package(anim_inputs.uasset, source.scripts(), None)?;
    let anim_index = anim_package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "AnimSequence")
        .ok_or("未找到 AnimSequence 导出")?;
    let animation = read_anim_sequence(&anim_package, anim_index, &mappings)?;
    let frames = animation.frames.max(1);
    let mut keys = Vec::with_capacity(frames as usize);
    for frame in 0..frames {
        let ratio = if frames > 1 {
            frame as f32 / (frames - 1) as f32
        } else {
            0.0
        };
        let poses = animation_poses(&skeleton, &animation, ratio);
        keys.push(
            poses
                .iter()
                .map(|pose| UsdaAnimKey {
                    translation: [
                        pose.translation[0] as f32,
                        pose.translation[1] as f32,
                        pose.translation[2] as f32,
                    ],
                    rotation: [
                        pose.rotation[0] as f32,
                        pose.rotation[1] as f32,
                        pose.rotation[2] as f32,
                        pose.rotation[3] as f32,
                    ],
                    scale: [
                        pose.scale[0] as f32,
                        pose.scale[1] as f32,
                        pose.scale[2] as f32,
                    ],
                })
                .collect(),
        );
    }
    let usda_animation = UsdaAnimation {
        name: animation.name.clone(),
        frames,
        fps: if animation.length > 0.0 {
            frames as f32 / animation.length
        } else {
            30.0
        },
        keys,
    };
    let result = write_usda_with_animation(
        &bundle.mesh,
        &bundle.materials,
        &ExportOptions::default(),
        Some(&usda_animation),
    )?;
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, &result.text)?;
    let written = write_resources(
        output_path.parent().unwrap_or(Path::new(".")),
        &result.resources,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "usda={} bytes={} frames={} bones={} sidecars={}",
        output,
        result.text.len(),
        usda_animation.frames,
        bundle.mesh.bones.len(),
        written.len()
    );
    Ok(())
}
