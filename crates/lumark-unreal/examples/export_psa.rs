//! 诊断：把 SkeletalMesh 的 AnimSequence 导出为 ActorX PSA。
//!
//! 用法：`export_psa <game-root> <container> <mesh-entry> <anim-entry> <mapping.usmap> <out.psa>`

use std::error::Error;
use std::path::Path;

use lumark_export::{AnimKey, ExportBone, ExportOptions, PsaExport, write_psa};
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, animation_poses, extract_key_for_game,
    parse_package, read_anim_sequence, scan_game,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let mesh_entry = args.next().ok_or("缺少 SkeletalMesh 资产路径")?;
    let anim_entry = args.next().ok_or("缺少 AnimSequence 资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出 .psa")?;

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

    // 骨架来自 SkeletalMesh。
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
    let skeleton = preview.skeleton;
    let bones: Vec<ExportBone> = skeleton
        .bones
        .iter()
        .map(|bone| ExportBone {
            name: bone.name.clone(),
            parent_index: bone.parent_index,
            position: [
                bone.translation[0] as f32,
                bone.translation[1] as f32,
                bone.translation[2] as f32,
            ],
            rotation: [
                bone.rotation[0] as f32,
                bone.rotation[1] as f32,
                bone.rotation[2] as f32,
                bone.rotation[3] as f32,
            ],
        })
        .collect();

    // 动画来自 AnimSequence。
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
    let fps = if animation.length > 0.0 {
        frames as f32 / animation.length
    } else {
        30.0
    };
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
                .map(|pose| AnimKey {
                    position: [
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
    let export = PsaExport {
        name: animation.name.clone(),
        group: "None".to_string(),
        frames,
        fps,
        bones: bones.clone(),
        keys,
    };
    let data = write_psa(&export, &ExportOptions::default())?;
    if let Some(parent) = Path::new(&output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, &data)?;
    println!(
        "psa={} bytes={} frames={} bones={} fps={:.3} chunks={}",
        output,
        data.len(),
        frames,
        bones.len(),
        fps,
        chunk_names(&data).join(",")
    );
    Ok(())
}

/// 列出 PSA chunk 名称。
fn chunk_names(data: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut position = 0usize;
    while position + 32 <= data.len() {
        let name = String::from_utf8_lossy(&data[position..position + 20])
            .trim_end_matches('\0')
            .to_string();
        let data_size = i32::from_le_bytes(
            data[position + 24..position + 28]
                .try_into()
                .unwrap_or_default(),
        );
        let count = i32::from_le_bytes(
            data[position + 28..position + 32]
                .try_into()
                .unwrap_or_default(),
        );
        names.push(name);
        position += 32 + data_size.max(0) as usize * count.max(0) as usize;
    }
    names
}
