//! 诊断：把 Unreal 网格转换为场景后用 wgpu GPU 离屏渲染输出 PNG。
//!
//! 用法：`render_scene_gpu <game-root> <container> <mesh-entry> <mapping.usmap> <out.png> [width]`

use std::error::Error;
use std::time::Instant;

use lumark_render::{GpuRenderer, RenderSettings};
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot,
    apply_animation, extract_key_for_game, parse_package, read_anim_sequence,
    resolve_material_asset, resolve_material_slots, scan_game, scene_material, skeletal_mesh_scene,
    static_mesh_scene,
};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少网格资产路径")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let output = args.next().ok_or("缺少输出 PNG")?;
    let width: u32 = args
        .next()
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(640);
    let material_entry = args.next();
    let animation_entry = args.next();
    let animation_time: f32 = args
        .next()
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(0.0);
    let mut animation_data = None;

    let renderer = GpuRenderer::new()?;
    println!(
        "gpu adapter={} backend={}",
        renderer.adapter_name(),
        renderer.adapter_backend()
    );

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
    let inputs = source.read_package(&entry)?;
    let package = parse_package(inputs.uasset, source.scripts(), None)?;
    let exports = &package.info().exports;

    let mut scene = if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = package.read_static_mesh(index, &mappings)?;
        let slots = mesh.material_slots.clone();
        let resolved = resolve_material_slots(
            &mut source,
            &entry,
            &slots,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        static_mesh_scene(&mesh.name, &mesh, &resolved)
    } else if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::SkeletalMesh)
    {
        let preview =
            package.read_skeletal_mesh_preview(index, &mappings, inputs.ubulk.as_deref())?;
        let slots: Vec<MaterialSlot> = preview
            .material_names
            .iter()
            .map(|name| MaterialSlot {
                name: name.clone(),
                package: None,
            })
            .collect();
        let resolved = resolve_material_slots(
            &mut source,
            &entry,
            &slots,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        if let Some(animation_entry) = &animation_entry {
            let animation_inputs = source.read_package(animation_entry)?;
            let animation_package = parse_package(animation_inputs.uasset, source.scripts(), None)?;
            let animation_index = animation_package
                .info()
                .exports
                .iter()
                .position(|export| export.class_name == "AnimSequence")
                .ok_or("未找到 AnimSequence 导出")?;
            let animation = read_anim_sequence(&animation_package, animation_index, &mappings)?;
            println!(
                "anim={} length={:.3}s frames={} tracks={}",
                animation.name,
                animation.length,
                animation.frames,
                animation.track_to_bone.len()
            );
            animation_data = Some((preview.skeleton.clone(), animation));
        }
        skeletal_mesh_scene(&preview.skeleton.name, &preview, &resolved)
    } else {
        return Err("资产中没有 StaticMesh/SkeletalMesh 导出".into());
    };

    if let Some((skeleton, animation)) = &animation_data {
        let applied = apply_animation(&mut scene, skeleton, animation, animation_time);
        println!("animation time={:.3}s skin_pose={applied}", animation_time);
    }

    // 可选：用一个真实材质覆盖第一个材质槽，验证 GPU 贴图/法线链路。
    if let Some(material_entry) = material_entry {
        let resolved = resolve_material_asset(
            &mut source,
            &material_entry,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        if let Some(appearance) = &resolved.appearance {
            let material =
                scene_material(&resolved.slot.name, appearance, resolved.viewer.as_ref());
            let handle = scene.add_material(material);
            if let Some(node) = scene.nodes.iter_mut().find(|node| node.mesh.is_some()) {
                if node.materials.is_empty() {
                    node.materials.push(Some(handle));
                } else {
                    node.materials[0] = Some(handle);
                }
            }
            println!(
                "material={} textures={} warnings={}",
                resolved.slot.name,
                appearance.textures.len(),
                resolved.warnings.len()
            );
        }
    }

    let settings = RenderSettings {
        width,
        height: width,
        supersample: 2,
        ..RenderSettings::default()
    };
    let started = Instant::now();
    let target = renderer.render(&scene, &settings)?;
    let elapsed = started.elapsed();
    println!(
        "scene={} nodes={} tris={} materials={} render {}x{} ss={} elapsed={:?}",
        scene.name,
        scene.stats().nodes,
        scene.stats().triangles,
        scene.stats().materials,
        target.width,
        target.height,
        settings.supersample,
        elapsed
    );
    let image = image::RgbaImage::from_raw(target.width, target.height, target.rgba)
        .ok_or("渲染尺寸与像素数量不一致")?;
    if let Some(parent) = std::path::Path::new(&output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.save(&output)?;
    println!("saved {}", std::path::Path::new(&output).display());
    Ok(())
}
