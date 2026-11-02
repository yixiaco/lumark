//! 端到端验收：扫描游戏、提取密钥、读取映射并预览一个资产。
//!
//! 用法：`accept_game <game-root> <mapping.usmap> <oodle-dll|-> <container> <entry> <out.png>`
//! Oodle 参数传 `-` 表示使用内置纯 Rust 解码器（不加载 DLL）。不打印完整 AES 密钥。

use std::error::Error;
use std::path::Path;

use lumark_archive::{
    ArchiveService, IoStoreContainer, configure_oodle_library, oodle_backend_setting,
};
use lumark_unreal::{
    AssetKind, Mappings, ZenScriptObjects, extract_key_for_game, parse_package, scan_game,
};

/// 运行验收。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let oodle = args
        .next()
        .ok_or("缺少 Oodle DLL（使用 - 表示内置 Rust 后端）")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少资产路径")?;
    let output = args.next().ok_or("缺少输出 PNG")?;

    println!("oodle_backend={}", oodle_backend_setting());
    if oodle != "-" {
        configure_oodle_library(Some(oodle.into())).map_err(|error| format!("Oodle：{error}"))?;
    }
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
    if container_path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pak"))
    {
        let info = ArchiveService::list(&container_path, Some(key.as_str()))?;
        println!(
            "pak={} entries={} encrypted={}",
            container_path.display(),
            info.entries.len(),
            info.encrypted
        );
        for entry in info.entries.iter().take(5) {
            println!("  {}", entry.path);
        }
        for entry in info
            .entries
            .iter()
            .filter(|entry| entry.path.ends_with(".uasset") && entry.path.contains("/Textures/"))
            .take(10)
        {
            println!("  uasset: {}", entry.path);
        }
        if entry.ends_with(".uasset") {
            let output_dir = Path::new("target/ff7-asset");
            let selected = vec![
                entry.clone(),
                entry.replace(".uasset", ".uexp"),
                entry.replace(".uasset", ".ubulk"),
            ];
            let options = lumark_archive::ExtractionOptions {
                output_dir: output_dir.to_path_buf(),
                selected_paths: selected,
                overwrite: true,
                password: Some(key.clone()),
                ..Default::default()
            };
            ArchiveService::extract(&container_path, &options, |_| {})?;
            println!("extracted asset to {}", output_dir.display());
        }
        return Ok(());
    }
    let container = IoStoreContainer::open(&container_path, Some(&key))?;
    let global = container_path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let scripts = global
        .as_deref()
        .map(|path| ZenScriptObjects::from_container(path, Some(&key)))
        .transpose()?;
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;
    let data = container.read_file(&entry)?;
    let package = parse_package(data, scripts.as_ref(), None)?;
    println!(
        "package={} exports={} structs={}",
        package.info().package_name,
        package.info().exports.len(),
        mappings.structs.len()
    );
    let texture_index = package
        .info()
        .exports
        .iter()
        .position(|export| export.kind == AssetKind::Texture);
    if let Some(index) = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "AnimSequence")
    {
        let metadata = package.read_asset_metadata(index, &mappings)?;
        println!(
            "animation={} rows={:?}",
            package
                .info()
                .exports
                .get(index)
                .map(|export| export.name.as_str())
                .unwrap_or("?"),
            metadata.rows()
        );
        return Ok(());
    }
    let mesh_index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "StaticMesh");
    let rgba = if let Some(index) = texture_index {
        let ubulk = entry
            .strip_suffix(".uasset")
            .map(|stem| format!("{stem}.ubulk"))
            .and_then(|path| container.read_file(&path).ok());
        let texture = package.read_texture2d(index, &mappings, ubulk.as_deref())?;
        println!(
            "texture={} {}x{} format={} mips={}",
            texture.name,
            texture.width,
            texture.height,
            texture.pixel_format,
            texture.mips.len()
        );
        (
            texture.width as u32,
            texture.height as u32,
            texture.decode_mip_rgba(0)?,
        )
    } else if let Some(index) = mesh_index {
        let mesh = package.read_static_mesh(index, &mappings)?;
        println!(
            "mesh={} lods={} parsed_lods={} verts={} tris={}",
            mesh.name,
            mesh.lod_count,
            mesh.lods.len(),
            mesh.lod.positions.len(),
            mesh.lod.triangle_count()
        );
        let rgba = lumark_unreal::render_static_mesh(&mesh.lod, 480, 360);
        (480, 360, rgba)
    } else if let Some((index, is_skeletal)) =
        package
            .info()
            .exports
            .iter()
            .enumerate()
            .find_map(|(index, export)| match export.class_name.as_str() {
                "Skeleton" => Some((index, false)),
                "SkeletalMesh" => Some((index, true)),
                _ => None,
            })
    {
        if is_skeletal {
            let ubulk = entry
                .strip_suffix(".uasset")
                .map(|stem| format!("{stem}.ubulk"))
                .and_then(|path| container.read_file(&path).ok());
            let preview = package.read_skeletal_mesh_preview(index, &mappings, ubulk.as_deref())?;
            println!(
                "skeletal_mesh={} bones={} lods={} parsed_lods={} sections={} verts={} tris={} skin={:?}",
                preview.skeleton.name,
                preview.skeleton.bones.len(),
                preview.lod_count,
                preview.lods.len(),
                preview.lod_sections,
                preview.lod_vertices,
                preview.lod_triangles,
                preview.skin.as_ref().map(|skin| (
                    skin.vertices,
                    skin.max_influences,
                    skin.variable,
                    skin.influences.first().copied(),
                    skin.weights.first().copied()
                ))
            );
            println!("material_names={:?}", preview.material_names);
            for (lod, mesh) in preview.lods.iter().enumerate() {
                println!(
                    "lod{lod} verts={} tris={} sections={} skin_vertices={:?}",
                    mesh.positions.len(),
                    mesh.triangle_count(),
                    mesh.sections.len(),
                    preview
                        .lod_skins
                        .get(lod)
                        .and_then(|skin| skin.as_ref())
                        .map(|skin| skin.vertices)
                );
            }
            if let Some(mesh) = preview.geometry {
                if let Some(path) = std::env::var_os("LUMARK_SKIN_POSE")
                    && let Some(skin) = &preview.skin
                    && let Some((bone, name)) =
                        lumark_unreal::skinning::dominant_skinned_bone(&preview.skeleton, skin)
                {
                    let angle = std::f64::consts::PI / 12.0;
                    let rotation = lumark_unreal::skinning::BoneRotation {
                        bone,
                        rotation: [angle.sin(), 0.0, 0.0, angle.cos()],
                    };
                    let posed = lumark_unreal::skinning::skin_mesh(
                        &mesh,
                        &preview.skeleton,
                        skin,
                        &[rotation],
                    );
                    let rgba =
                        lumark_unreal::render_static_mesh_orbit(&posed, 480, 360, 35.0, 25.0, 1.0);
                    if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
                        image.save(&path)?;
                        println!("skin_pose bone={bone} name={name} saved={}", path.display());
                    }
                    let rgba =
                        lumark_unreal::render_static_mesh_orbit(&posed, 480, 360, 35.0, 25.0, 1.0);
                    if let Some(image) = image::RgbaImage::from_raw(480, 360, rgba) {
                        image.save(&path)?;
                        println!("skin_pose bone={bone} name={name} saved={}", path.display());
                    }
                }
                let rgba = lumark_unreal::render_skeletal_mesh_orbit(
                    &mesh,
                    &preview.skeleton,
                    480,
                    360,
                    35.0,
                    25.0,
                    1.0,
                );
                (480, 360, rgba)
            } else {
                let rgba = lumark_unreal::render_skeleton_orbit(
                    &preview.skeleton,
                    480,
                    360,
                    35.0,
                    25.0,
                    1.0,
                );
                (480, 360, rgba)
            }
        } else {
            let skeleton = package.read_skeleton(index, &mappings)?;
            println!("skeleton={} bones={}", skeleton.name, skeleton.bones.len());
            let rgba = lumark_unreal::render_skeleton_orbit(&skeleton, 480, 360, 35.0, 25.0, 1.0);
            (480, 360, rgba)
        }
    } else {
        return Err("包中没有可预览的纹理、网格或骨骼".into());
    };
    let image = image::RgbaImage::from_raw(rgba.0, rgba.1, rgba.2).ok_or("图像尺寸不匹配")?;
    image.save(&output)?;
    println!("saved {output}");
    Ok(())
}
