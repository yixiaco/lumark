//! 诊断：把 Unreal 网格转换为场景后用 wgpu GPU 离屏渲染输出 PNG。
//!
//! 用法：`render_scene_gpu <game-root> <container> <mesh-entry> <mapping.usmap> <out.png> [键=值 …]`
//!
//! 可选键：`width`、`height`、`supersample`、`msaa`、`material`（覆盖首个材质槽的材质包路径）、
//! `animation`（AnimSequence 包路径）、`time`（动画时间，秒）、`lighting`（`studio`/`headlight`）、
//! `normal`（`ue`/`fmodel`）、`fov`/`distance`（相机基线）、`frames`（连续渲染帧数，用于观察
//! 目标与回读缓冲复用）。为兼容旧脚本，第 6 个位置参数仍可写成宽度，其后依次是材质、
//! 动画与时间。
//!
//! 输出中的 `elapsed` 是首帧离屏渲染 + 回读的总耗时；`frames>1` 时还会输出逐帧耗时与 `avg`。

use std::error::Error;
use std::time::Instant;

use lumark_render::{GpuRenderer, LightingMode, NormalMapConvention, RenderSettings};
use lumark_unreal::{
    AssetKind, GameAssetOptions, GameAssetSource, Mappings, MaterialResolveOptions, MaterialSlot,
    apply_animation, extract_key_for_game, parse_package, read_anim_sequence,
    resolve_material_asset, resolve_material_slots_exact, resolve_static_materials, scan_game,
    scene_material, skeletal_mesh_scene, static_mesh_scene,
};

/// 解析后的命令行参数。
struct Cli {
    /// 游戏根目录。
    root: String,
    /// 容器文件名（如 `pakchunk0-Windows.utoc`）。
    container: String,
    /// 容器内的网格资产路径。
    entry: String,
    /// 映射文件路径。
    mapping: String,
    /// 输出 PNG 路径。
    output: String,
    /// 输出宽度（像素）。
    width: u32,
    /// 输出高度（像素）；缺省与宽度相同。
    height: Option<u32>,
    /// 超采样倍数（1–4）。
    supersample: u32,
    /// MSAA 采样数（1 或 4）。
    msaa: u32,
    /// 覆盖首个材质槽的材质包路径。
    material_entry: Option<String>,
    /// 动画包路径。
    animation_entry: Option<String>,
    /// 动画时间（秒）。
    animation_time: f32,
    /// 光照模式（studio/headlight）。
    lighting: LightingMode,
    /// 法线贴图约定（ue/fmodel）。
    normal: NormalMapConvention,
    /// 垂直视场角（度）；用于对齐历史基线。
    fov: f32,
    /// 相机距离系数；用于对齐历史基线。
    distance: f32,
    /// 连续渲染帧数（>1 时额外输出逐帧耗时，用于观察目标/回读缓冲复用）。
    frames: u32,
}

impl Cli {
    /// 解析命令行；位置参数与 `键=值` 参数可混用。
    fn parse(arguments: Vec<String>) -> Result<Self, Box<dyn Error>> {
        let mut positional = arguments.into_iter();
        let root = positional.next().ok_or("缺少游戏目录")?;
        let container = positional.next().ok_or("缺少容器文件名")?;
        let entry = positional.next().ok_or("缺少网格资产路径")?;
        let mapping = positional.next().ok_or("缺少 .usmap")?;
        let output = positional.next().ok_or("缺少输出 PNG")?;
        let mut cli = Self {
            root,
            container,
            entry,
            mapping,
            output,
            width: 640,
            height: None,
            supersample: 2,
            msaa: 1,
            material_entry: None,
            animation_entry: None,
            animation_time: 0.0,
            lighting: LightingMode::Studio,
            normal: NormalMapConvention::Ue,
            fov: RenderSettings::default().fov_y_degrees,
            distance: RenderSettings::default().distance_scale,
            frames: 1,
        };
        // 旧式位置参数依次是宽度、材质、动画、时间。
        let mut legacy_slots = 0;
        for argument in positional {
            let Some((key, value)) = argument.split_once('=') else {
                match legacy_slots {
                    0 => {
                        cli.width = parse_number(&argument, "宽度")?;
                        legacy_slots = 1;
                    }
                    1 => {
                        cli.material_entry = Some(argument);
                        legacy_slots = 2;
                    }
                    2 => {
                        cli.animation_entry = Some(argument);
                        legacy_slots = 3;
                    }
                    _ => cli.animation_time = argument.parse()?,
                }
                continue;
            };
            match key {
                "width" => cli.width = parse_number(value, "宽度")?,
                "height" => cli.height = Some(parse_number(value, "高度")?),
                "supersample" => cli.supersample = parse_number(value, "超采样")?,
                "msaa" => cli.msaa = parse_number(value, "MSAA")?,
                "material" => cli.material_entry = Some(value.to_string()),
                "animation" => cli.animation_entry = Some(value.to_string()),
                "time" => cli.animation_time = value.parse()?,
                "lighting" => cli.lighting = parse_lighting(value)?,
                "normal" => cli.normal = parse_normal_convention(value)?,
                "fov" => cli.fov = value.parse()?,
                "distance" => cli.distance = value.parse()?,
                "frames" => cli.frames = parse_number(value, "帧数")?.max(1),
                other => return Err(format!("未知参数 {other}").into()),
            }
        }
        Ok(cli)
    }

    /// 渲染设置：尺寸、超采样、MSAA、光照模式与法线约定由命令行决定。
    fn settings(&self) -> RenderSettings {
        RenderSettings {
            width: self.width,
            height: self.height.unwrap_or(self.width),
            supersample: self.supersample,
            msaa_samples: self.msaa,
            lighting_mode: self.lighting,
            normal_map_convention: self.normal,
            fov_y_degrees: self.fov,
            distance_scale: self.distance,
            ..RenderSettings::default()
        }
    }
}

/// 解析光照模式参数。
fn parse_lighting(value: &str) -> Result<LightingMode, Box<dyn Error>> {
    match value.to_ascii_lowercase().as_str() {
        "studio" => Ok(LightingMode::Studio),
        "headlight" => Ok(LightingMode::Headlight),
        other => Err(format!("未知光照模式 {other}（可选 studio/headlight）").into()),
    }
}

/// 解析法线贴图约定参数。
fn parse_normal_convention(value: &str) -> Result<NormalMapConvention, Box<dyn Error>> {
    match value.to_ascii_lowercase().as_str() {
        "ue" => Ok(NormalMapConvention::Ue),
        "fmodel" => Ok(NormalMapConvention::Fmodel),
        other => Err(format!("未知法线约定 {other}（可选 ue/fmodel）").into()),
    }
}

/// 解析无符号整数参数并给出中文错误。
fn parse_number(value: &str, label: &str) -> Result<u32, Box<dyn Error>> {
    value
        .parse()
        .map_err(|_| format!("{label}不是合法数字：{value}").into())
}

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse(std::env::args().skip(1).collect())?;
    let mut animation_data = None;

    let renderer = GpuRenderer::new()?;
    println!(
        "gpu adapter={} backend={}",
        renderer.adapter_name(),
        renderer.adapter_backend()
    );

    let game = scan_game(std::path::Path::new(&cli.root))?;
    let container_path = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == cli.container)
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
    let mappings = Mappings::parse_file(std::path::Path::new(&cli.mapping))?;
    let mut source = GameAssetSource::open(
        &container_path,
        GameAssetOptions {
            key: Some(key.as_str()),
            global,
            oodle: None,
            mappings: Some(cli.mapping.as_str().into()),
        },
    )?;
    let inputs = source.read_package(&cli.entry)?;
    let package = parse_package(inputs.uasset, source.scripts(), None)?;
    let exports = &package.info().exports;

    let mut scene = if let Some(index) = exports
        .iter()
        .position(|export| export.kind == AssetKind::StaticMesh)
    {
        let mesh = package.read_static_mesh(index, &mappings)?;
        let resolved = resolve_static_materials(
            &mut source,
            &cli.entry,
            &mesh.material_slots,
            &mesh.material_imports,
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
        let imports: Vec<Option<usize>> = preview
            .material_imports
            .iter()
            .map(|index| (*index < 0).then(|| (-*index - 1) as usize))
            .collect();
        let resolved = resolve_material_slots_exact(
            &mut source,
            &cli.entry,
            &slots,
            &imports,
            Some(&mappings),
            &MaterialResolveOptions::default(),
        );
        if let Some(animation_entry) = &cli.animation_entry {
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
        let applied = apply_animation(&mut scene, skeleton, animation, cli.animation_time);
        println!(
            "animation time={:.3}s skin_pose={applied}",
            cli.animation_time
        );
    }

    // 可选：用一个真实材质覆盖第一个材质槽，验证 GPU 贴图/法线链路。
    if let Some(material_entry) = &cli.material_entry {
        let resolved = resolve_material_asset(
            &mut source,
            material_entry,
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

    let settings = cli.settings();
    let bounds_started = Instant::now();
    let bounds = lumark_render::render_bounds(&scene);
    println!(
        "bounds=({:?}..{:?}) center={:?} extent={:.3} bounds_elapsed={:?}",
        bounds.min,
        bounds.max,
        bounds.center(),
        bounds.extent(),
        bounds_started.elapsed()
    );
    let mut last_target = None;
    let mut elapsed_frames = Vec::with_capacity(cli.frames as usize);
    for frame in 0..cli.frames {
        let started = Instant::now();
        let target = renderer.render(&scene, &settings)?;
        let elapsed = started.elapsed();
        if cli.frames > 1 {
            println!("frame={frame} elapsed={elapsed:?}");
        }
        elapsed_frames.push(elapsed);
        last_target = Some(target);
    }
    let target = last_target.ok_or("没有渲染出任何帧")?;
    let elapsed = elapsed_frames.first().copied().unwrap_or_default();
    let average = elapsed_frames.iter().sum::<std::time::Duration>() / cli.frames.max(1);
    println!(
        "scene={} nodes={} tris={} materials={} render {}x{} ss={} msaa={} lighting={:?} normal={:?} elapsed={:?} avg={:?}",
        scene.name,
        scene.stats().nodes,
        scene.stats().triangles,
        scene.stats().materials,
        target.width,
        target.height,
        settings.supersample,
        settings.msaa_samples,
        settings.lighting_mode,
        settings.normal_map_convention,
        elapsed,
        average
    );
    let image = image::RgbaImage::from_raw(target.width, target.height, target.rgba)
        .ok_or("渲染尺寸与像素数量不一致")?;
    if let Some(parent) = std::path::Path::new(&cli.output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.save(&cli.output)?;
    println!("saved {}", std::path::Path::new(&cli.output).display());
    Ok(())
}
