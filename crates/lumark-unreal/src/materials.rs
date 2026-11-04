//! 网格材质槽解析：槽位 → 材质包 → 父材质链 → 贴图。
//!
//! 查看器和后续导出都需要把 `StaticMesh`/`SkeletalMesh` 的材质槽对应到具体
//! 材质资产，并尽量加载基础色/法线/ORM 贴图。Zen 包优先使用 `SkeletalMaterials`
//! 里的对象引用（ImportMap → 目标包/导出）精确定位材质，再由 `material_graph`
//! 还原父链与贴图；没有对象引用时才退回材质槽名/目录/命名约定。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lumark_scene::OrmLayout;

use crate::LegacyPackage;
use crate::material_graph::{self, MaterialGraph, MaterialObjectRef, resolve_material_object};
use crate::package::{
    Mappings, MaterialAppearance, MaterialSlot, MaterialTextureKind, MaterialViewerData,
    MaterialViewerImage, PackageInfo, material_parent_package, read_legacy_material_appearance,
    read_zen_material_appearance, texture_references,
};
use crate::source::GameAssetSource;
use crate::{is_legacy_package, parse_package, read_legacy_texture};

/// 根据金属/粗糙度贴图的命名推断通道布局。
///
/// UE 资产常见两类：`_ORM`/`_OcclusionRoughnessMetallic` 用 R=AO、G=粗糙度、
/// B=金属度；`_SpecularMask`/`_SM` 这类高光遮罩把金属度放在 G、粗糙度放在 B。
pub fn detect_orm_layout(name: &str) -> OrmLayout {
    let lower = name.to_ascii_lowercase();
    let looks_like_specular_mask = lower.contains("specular")
        || lower.contains("specmask")
        || lower.contains("_sm")
        || lower.ends_with("srm");
    if looks_like_specular_mask {
        OrmLayout::SpecularMask
    } else {
        OrmLayout::AoRoughMetal
    }
}

/// 解码后的贴图缓存（跨资产复用）。
pub type TextureCache = Arc<Mutex<HashMap<String, Option<MaterialViewerImage>>>>;

/// 创建一个空贴图缓存。
pub fn texture_cache() -> TextureCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// 材质解析选项。
#[derive(Debug, Clone)]
pub struct MaterialResolveOptions {
    /// 是否加载并解码贴图。
    pub load_textures: bool,
    /// 贴图最大边长；0 表示不缩放。
    pub max_texture_size: u32,
    /// 父材质链最大深度。
    pub max_parent_depth: usize,
    /// 可选贴图解码缓存（跨资产复用）。
    pub texture_cache: Option<TextureCache>,
}

impl Default for MaterialResolveOptions {
    fn default() -> Self {
        Self {
            load_textures: true,
            max_texture_size: 512,
            max_parent_depth: 4,
            texture_cache: None,
        }
    }
}

/// 一个材质槽的解析结果。
#[derive(Debug, Clone)]
pub struct ResolvedMaterial {
    /// 原始槽位信息。
    pub slot: MaterialSlot,
    /// 容器内材质包路径（`.uasset`）。
    pub package_path: Option<String>,
    /// 合并父材质后的近似外观；未找到材质时为 `None`。
    pub appearance: Option<MaterialAppearance>,
    /// 交互式查看器数据（已加载的贴图）。
    pub viewer: Option<MaterialViewerData>,
    /// 实际加载过的材质/贴图路径。
    pub loaded_assets: Vec<String>,
    /// 非致命警告。
    pub warnings: Vec<String>,
}

/// 为网格的全部材质槽解析材质与贴图。
pub fn resolve_material_slots(
    source: &mut GameAssetSource,
    mesh_path: &str,
    slots: &[MaterialSlot],
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
) -> Vec<ResolvedMaterial> {
    resolve_material_slots_impl(source, mesh_path, slots, &[], mappings, options)
}

/// 使用网格材质槽的对象引用精确解析材质（Zen `SkeletalMaterials`）。
///
/// `imports` 与 `slots` 等长：`Some(导入索引)` 时按 ImportMap 精确定位材质包，
/// `None` 或解析失败时回退到 [`resolve_material_slots`] 的命名启发式。
pub fn resolve_material_slots_exact(
    source: &mut GameAssetSource,
    mesh_path: &str,
    slots: &[MaterialSlot],
    imports: &[Option<usize>],
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
) -> Vec<ResolvedMaterial> {
    resolve_material_slots_impl(source, mesh_path, slots, imports, mappings, options)
}

/// 网格包的 ImportMap 上下文，用于解析材质槽对象引用。
struct MeshImportContext {
    info: Option<PackageInfo>,
    imported_ids: Vec<u64>,
}

impl MeshImportContext {
    /// 读取网格包元数据；非 IoStore 或读取失败时返回空上下文。
    fn new(source: &mut GameAssetSource, mesh_path: &str) -> Self {
        let Some(package_id) = source.package_id_for_path(mesh_path) else {
            return Self {
                info: None,
                imported_ids: Vec::new(),
            };
        };
        let imported_ids = source.imported_packages(package_id).unwrap_or_default();
        let info = source
            .read_package(mesh_path)
            .ok()
            .and_then(|inputs| parse_package(inputs.uasset, source.scripts(), None).ok())
            .map(|package| package.info().clone());
        Self { info, imported_ids }
    }
}

/// 材质槽解析实现；`imports` 为空时全部走命名启发式。
fn resolve_material_slots_impl(
    source: &mut GameAssetSource,
    mesh_path: &str,
    slots: &[MaterialSlot],
    imports: &[Option<usize>],
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
) -> Vec<ResolvedMaterial> {
    let packages = package_index(source);
    let context = if imports.is_empty() {
        None
    } else {
        Some(MeshImportContext::new(source, mesh_path))
    };
    let mut graph = MaterialGraph::new();
    slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            if let Some(object) = imports.get(index).copied().flatten().and_then(|import| {
                context
                    .as_ref()
                    .and_then(|context| context.info_import(import))
            }) && let Some(resolved) = resolve_exact_slot(
                source, &mut graph, object, slot, mappings, options, &packages,
            ) {
                return resolved;
            }
            resolve_one(source, mesh_path, slot, mappings, options, &packages)
        })
        .collect()
}

/// `MeshImportContext` 的导入解析辅助。
impl MeshImportContext {
    /// 把导入索引解析为目标包/导出。
    fn info_import(&self, import: usize) -> Option<MaterialObjectRef> {
        let info = self.info.as_ref()?;
        resolve_material_object(info, &self.imported_ids, import)
    }
}

/// 用精确对象引用解析一个材质槽。
fn resolve_exact_slot(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    object: MaterialObjectRef,
    slot: &MaterialSlot,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
    packages: &[String],
) -> Option<ResolvedMaterial> {
    let mappings = mappings?;
    let mut resolved = ResolvedMaterial {
        slot: slot.clone(),
        package_path: Some(format!("package:0x{:016X}", object.package_id)),
        appearance: None,
        viewer: None,
        loaded_assets: Vec::new(),
        warnings: Vec::new(),
    };
    let appearance = material_graph::load_material_by_id(
        source,
        graph,
        object,
        mappings,
        0,
        &mut resolved.loaded_assets,
        &mut resolved.warnings,
    )?;
    fill_viewer_from_appearance(
        source,
        "",
        appearance,
        Some(mappings),
        options,
        packages,
        &mut resolved,
    );
    Some(resolved)
}

/// 返回容器内全部 `.uasset` 路径。
fn package_index(source: &GameAssetSource) -> Vec<String> {
    source
        .list()
        .into_iter()
        .map(|entry| entry.path)
        .filter(|path| path.to_ascii_lowercase().ends_with(".uasset"))
        .collect()
}

/// 直接解析一个材质资产（材质/材质实例），供材质查看器和导出使用。
pub fn resolve_material_asset(
    source: &mut GameAssetSource,
    material_path: &str,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
) -> ResolvedMaterial {
    let packages = package_index(source);
    let name = material_path
        .rsplit('/')
        .next()
        .and_then(|file| file.rsplit_once('.').map(|(stem, _)| stem))
        .unwrap_or(material_path)
        .to_string();
    let mut resolved = ResolvedMaterial {
        slot: MaterialSlot {
            name,
            package: None,
        },
        package_path: Some(material_path.to_string()),
        appearance: None,
        viewer: None,
        loaded_assets: vec![material_path.to_string()],
        warnings: Vec::new(),
    };
    fill_resolved(
        source,
        material_path,
        &mut resolved,
        mappings,
        options,
        &packages,
    );
    resolved
}

/// 解析单个槽位。
fn resolve_one(
    source: &mut GameAssetSource,
    mesh_path: &str,
    slot: &MaterialSlot,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
    packages: &[String],
) -> ResolvedMaterial {
    let mut resolved = ResolvedMaterial {
        slot: slot.clone(),
        package_path: None,
        appearance: None,
        viewer: None,
        loaded_assets: Vec::new(),
        warnings: Vec::new(),
    };
    let Some(path) = find_material_package(mesh_path, slot, packages) else {
        resolved
            .warnings
            .push(format!("未找到材质槽 {} 对应的材质包", slot.name));
        return resolved;
    };
    resolved.package_path = Some(path.clone());
    resolved.loaded_assets.push(path.clone());
    fill_resolved(source, &path, &mut resolved, mappings, options, packages);
    resolved
}

/// 读取材质外观并按需加载贴图，填充解析结果。
fn fill_resolved(
    source: &mut GameAssetSource,
    path: &str,
    resolved: &mut ResolvedMaterial,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
    packages: &[String],
) {
    let appearance = match load_material(
        source,
        path,
        mappings,
        options,
        0,
        &mut resolved.loaded_assets,
        &mut resolved.warnings,
    ) {
        Some(appearance) => appearance,
        None => {
            resolved
                .warnings
                .push(format!("材质包 {path} 无法解析外观参数"));
            return;
        }
    };
    fill_viewer_from_appearance(
        source, path, appearance, mappings, options, packages, resolved,
    );
}

/// 按材质外观加载查看器贴图并写入解析结果。
fn fill_viewer_from_appearance(
    source: &mut GameAssetSource,
    material_path: &str,
    appearance: MaterialAppearance,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
    packages: &[String],
    resolved: &mut ResolvedMaterial,
) {
    if options.load_textures {
        let base = resolve_texture(
            source,
            material_path,
            &appearance,
            MaterialTextureKind::BaseColor,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        let normal = resolve_texture(
            source,
            material_path,
            &appearance,
            MaterialTextureKind::Normal,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        let orm = resolve_texture(
            source,
            material_path,
            &appearance,
            MaterialTextureKind::MetallicRoughness,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        let occlusion = resolve_texture(
            source,
            material_path,
            &appearance,
            MaterialTextureKind::Occlusion,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        // 基础色 tint 已由材质图按 FModel DiffuseColor 规则计算，无需再覆盖。
        resolved.viewer = Some(MaterialViewerData {
            appearance: appearance.clone(),
            base_color: base,
            normal,
            metallic_roughness: orm,
            occlusion,
        });
    }
    resolved.appearance = Some(appearance);
}

/// 读取一个材质包的近似外观；旧式包会递归合并父材质。
fn load_material(
    source: &mut GameAssetSource,
    path: &str,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
    depth: usize,
    loaded: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<MaterialAppearance> {
    if depth > options.max_parent_depth {
        return None;
    }
    let inputs = source.read_package(path).ok()?;
    if is_legacy_package(&inputs.uasset) {
        let package = LegacyPackage::parse(&inputs.uasset, inputs.uexp.as_deref()?).ok()?;
        let mappings = mappings?;
        let mut appearance = read_legacy_material_appearance(&package, mappings);
        if depth < options.max_parent_depth
            && let Some(parent) = material_parent_package(&package)
            && let Some(relative) = package_to_relative(path, &parent)
        {
            loaded.push(relative.clone());
            match load_material(
                source,
                &relative,
                Some(mappings),
                options,
                depth + 1,
                loaded,
                warnings,
            ) {
                Some(parent_appearance) => appearance.merge_parent(&parent_appearance),
                None => warnings.push(format!("父材质 {relative} 无法解析")),
            }
        }
        // 旧式导入表可精确给出贴图包路径。
        let mut references = texture_references(&package);
        references.extend(appearance.textures.clone());
        for reference in references {
            if let Some(package) = &reference.package
                && let Some(relative) = package_to_relative(path, package)
                && !appearance
                    .textures
                    .iter()
                    .any(|existing| existing.package.as_deref() == Some(package))
            {
                appearance.textures.push(reference);
                loaded.push(relative);
            }
        }
        return Some(appearance);
    }
    let package = parse_package(inputs.uasset, source.scripts(), None).ok()?;
    let mappings = mappings?;
    // Zen 材质按包 ID 走材质图：对象引用精确定位父材质与贴图包。
    let package_id = source.package_id_for_path(path)?;
    let mut graph = MaterialGraph::new();
    material_graph::load_material_by_id(
        source,
        &mut graph,
        MaterialObjectRef {
            package_id,
            export_hash: 0,
        },
        mappings,
        depth,
        loaded,
        warnings,
    )
    .or_else(|| {
        // 没有可解析的对象引用时保留属性流近似值（至少拿到颜色/双面标记）。
        // 只有确认包里存在材质导出才回退，避免把贴图资产误当材质。
        let is_material_package = package
            .info()
            .exports
            .iter()
            .any(|export| export.class_name.to_ascii_lowercase().contains("material"));
        is_material_package.then(|| read_zen_material_appearance(&package, mappings))
    })
}

/// 加载一张贴图为查看器图像（带可选缓存）。
fn load_texture_image(
    source: &mut GameAssetSource,
    path: &str,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
) -> Option<MaterialViewerImage> {
    let max_size = options.max_texture_size;
    let cache_key = format!("{path}|{max_size}");
    if let Some(cache) = &options.texture_cache
        && let Ok(cache) = cache.lock()
        && let Some(cached) = cache.get(&cache_key)
    {
        return cached.clone();
    }
    let image = decode_texture_image(source, path, mappings, max_size);
    if let Some(cache) = &options.texture_cache
        && let Ok(mut cache) = cache.lock()
    {
        cache.insert(cache_key, image.clone());
    }
    image
}

/// 解码一张贴图（不含缓存）。
fn decode_texture_image(
    source: &mut GameAssetSource,
    path: &str,
    mappings: Option<&Mappings>,
    max_size: u32,
) -> Option<MaterialViewerImage> {
    let inputs = source.read_package(path).ok()?;
    if is_legacy_package(&inputs.uasset) {
        let texture = read_legacy_texture(
            &inputs.uasset,
            inputs.uexp.as_deref()?,
            inputs.ubulk.as_deref(),
        )
        .ok()?;
        let rgba = texture.decode_mip_rgba(0).ok()?;
        return MaterialViewerImage::new(
            rgba,
            texture.width as u32,
            texture.height as u32,
            size_limit(max_size),
        );
    }
    let package = parse_package(inputs.uasset, source.scripts(), None).ok()?;
    let index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "Texture2D")?;
    let texture = package
        .read_texture2d(index, mappings?, inputs.ubulk.as_deref())
        .ok()?;
    let rgba = texture.decode_mip_rgba(0).ok()?;
    MaterialViewerImage::new(
        rgba,
        texture.width as u32,
        texture.height as u32,
        size_limit(max_size),
    )
}

/// 按包 ID 加载一张贴图为查看器图像（带可选缓存）。
fn load_texture_image_by_id(
    source: &mut GameAssetSource,
    package_id: u64,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
) -> Option<MaterialViewerImage> {
    let mappings = mappings?;
    let max_size = options.max_texture_size;
    let cache_key = format!("id:{package_id:016X}|{max_size}");
    if let Some(cache) = &options.texture_cache
        && let Ok(cache) = cache.lock()
        && let Some(cached) = cache.get(&cache_key)
    {
        return cached.clone();
    }
    let image = material_graph::decode_texture_by_id(source, package_id, mappings, max_size);
    if let Some(cache) = &options.texture_cache
        && let Ok(mut cache) = cache.lock()
    {
        cache.insert(cache_key, image.clone());
    }
    image
}

/// 解析一张贴图：优先精确包路径/包 ID，其次按用途与命名约定搜索。
#[allow(clippy::too_many_arguments)]
fn resolve_texture(
    source: &mut GameAssetSource,
    material_path: &str,
    appearance: &MaterialAppearance,
    kind: MaterialTextureKind,
    mappings: Option<&Mappings>,
    options: &MaterialResolveOptions,
    packages: &[String],
    loaded: &mut Vec<String>,
) -> Option<MaterialViewerImage> {
    if let Some(reference) = appearance.texture(kind) {
        if let Some(package_id) = reference.package_id
            && let Some(image) = load_texture_image_by_id(source, package_id, mappings, options)
        {
            loaded.push(format!("package:0x{package_id:016X}"));
            return Some(image);
        }
        if let Some(package) = &reference.package
            && let Some(relative) = package_to_relative(material_path, package)
            && let Some(image) = load_texture_image(source, &relative, mappings, options)
        {
            loaded.push(relative);
            return Some(image);
        }
        if reference.name.to_ascii_lowercase().starts_with("t_")
            && let Some(path) = find_texture_by_filename(packages, &reference.name)
            && let Some(image) = load_texture_image(source, &path, mappings, options)
        {
            loaded.push(path);
            return Some(image);
        }
    }
    for candidate in texture_candidates(material_path, appearance, kind, packages) {
        if let Some(image) = load_texture_image(source, &candidate, mappings, options) {
            loaded.push(candidate);
            return Some(image);
        }
    }
    None
}

/// 在容器索引中按文件名查找贴图。
fn find_texture_by_filename(packages: &[String], name: &str) -> Option<String> {
    let target = format!("{}.uasset", name.to_ascii_lowercase());
    packages
        .iter()
        .find(|path| {
            path.rsplit('/')
                .next()
                .is_some_and(|file| file.eq_ignore_ascii_case(&target))
        })
        .cloned()
}

/// 按材质槽名查找材质包。
///
/// 优先级：槽位显式包路径 → 网格同目录同名资产 → 容器内同名资产（按与网格
/// 目录的公共前缀和路径长度排序）。
fn find_material_package(
    mesh_path: &str,
    slot: &MaterialSlot,
    packages: &[String],
) -> Option<String> {
    if let Some(package) = &slot.package
        && let Some(relative) = package_to_relative(mesh_path, package)
        && packages
            .iter()
            .any(|path| path.eq_ignore_ascii_case(&relative))
    {
        return Some(relative);
    }
    let name = slot.name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("none") {
        return None;
    }
    let mesh_dir = mesh_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let direct = format!("{mesh_dir}/{name}.uasset");
    if let Some(path) = packages
        .iter()
        .find(|path| path.eq_ignore_ascii_case(&direct))
    {
        return Some(path.clone());
    }
    let target = format!("{name}.uasset");
    let mut candidates: Vec<(usize, usize, &String)> = packages
        .iter()
        .filter_map(|path| {
            let file = path.rsplit('/').next()?;
            if !file.eq_ignore_ascii_case(&target) {
                return None;
            }
            Some((shared_directory_len(mesh_dir, path), path.len(), path))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if let Some((_, _, path)) = candidates.first() {
        return Some((*path).clone());
    }
    // 槽名与材质资产名常常不一致（例如槽位 `N_Lip` 对应 `M/N_Lips`，
    // `N_Body` 对应 `N_Body_Inst6`）。游戏资产里材质不会按槽名精确命名，
    // 因此再按「名称前缀/分词前缀 + 同角色目录亲和度」做一次模糊匹配。
    fuzzy_material_package(mesh_dir, name, packages)
}

/// 按槽名前缀和目录亲和度挑选材质资产，作为精确同名的兜底。
///
/// 只接受明显的材质候选：名称以槽名开头、槽名以资产名开头，或两者按
/// `_` 分词后除末段外完全一致且末段互为前缀（`N_Lip` ↔ `N_Lips`）。
/// 跳过网格/贴图/动画前缀，避免把 `SK_`、`T_` 资产当成材质。
fn fuzzy_material_package(mesh_dir: &str, slot_name: &str, packages: &[String]) -> Option<String> {
    let slot = slot_name.trim().to_ascii_lowercase();
    if slot.len() < 3 {
        return None;
    }
    // 网格路径的上一级是角色目录（例如 `.../CH/Nana`），用于目录亲和度。
    let character_root = mesh_dir
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or(mesh_dir);
    let mut best: Option<(i32, usize, usize, &String)> = None;
    for path in packages {
        let Some(file) = path.rsplit('/').next() else {
            continue;
        };
        let Some(stem) = file.strip_suffix(".uasset") else {
            continue;
        };
        let stem_lower = stem.to_ascii_lowercase();
        if stem_lower.len() < 2
            || matches!(
                stem_lower.split('_').next().unwrap_or_default(),
                "sk" | "sm" | "t" | "tx" | "anim" | "abp" | "bp" | "phys" | "ns"
            )
        {
            continue;
        }
        let name_score = if stem_lower == slot {
            5
        } else if stem_lower.starts_with(&slot) {
            4
        } else if slot.starts_with(&stem_lower) {
            3
        } else if tokens_prefix_match(&slot, &stem_lower) {
            2
        } else {
            continue;
        };
        let directory_score = shared_directory_len(character_root, path);
        // 同类材质通常集中放在角色目录下的 `M/` 子目录。
        let material_dir_bonus = i32::from(
            path.rsplit_once('/')
                .map(|(dir, _)| dir.rsplit('/').next() == Some("M") || dir.ends_with("/M"))
                .unwrap_or(false),
        ) * 2;
        let score = name_score * 10 + material_dir_bonus;
        let extra_len = stem_lower.len().saturating_sub(slot.len());
        let entry = (score, directory_score, usize::MAX - extra_len, path);
        let better = match &best {
            None => true,
            Some((best_score, best_dir, best_extra, _)) => {
                score > *best_score
                    || (score == *best_score && directory_score > *best_dir)
                    || (score == *best_score
                        && directory_score == *best_dir
                        && entry.2 > *best_extra)
            }
        };
        if better {
            best = Some(entry);
        }
    }
    best.map(|(_, _, _, path)| path.clone())
}

/// 判断两个下划线分词名称除最后一段外相同，且最后一段互为前缀。
fn tokens_prefix_match(slot: &str, stem: &str) -> bool {
    let slot_tokens: Vec<&str> = slot.split('_').filter(|part| !part.is_empty()).collect();
    let stem_tokens: Vec<&str> = stem.split('_').filter(|part| !part.is_empty()).collect();
    if slot_tokens.len() < 2 || stem_tokens.len() < 2 || slot_tokens.len() != stem_tokens.len() {
        return false;
    }
    let (slot_last, stem_last) = (
        slot_tokens[slot_tokens.len() - 1],
        stem_tokens[stem_tokens.len() - 1],
    );
    slot_tokens[..slot_tokens.len() - 1] == stem_tokens[..stem_tokens.len() - 1]
        && (slot_last.starts_with(stem_last) || stem_last.starts_with(slot_last))
}

/// 按用途、材质资产名和参数提示生成贴图候选路径。
fn texture_candidates(
    material_path: &str,
    appearance: &MaterialAppearance,
    kind: MaterialTextureKind,
    packages: &[String],
) -> Vec<String> {
    let Some((material_dir, file)) = material_path.rsplit_once('/') else {
        return Vec::new();
    };
    let stem = material_stem(file);
    if stem.is_empty() {
        return Vec::new();
    }
    let suffixes = texture_kind_suffixes(kind);
    if suffixes.is_empty() {
        return Vec::new();
    }
    let hints: Vec<String> = appearance
        .texture(kind)
        .map(|reference| vec![reference.name.to_ascii_lowercase()])
        .unwrap_or_default();
    let mut candidates: Vec<(usize, usize, String)> = packages
        .iter()
        .filter_map(|path| {
            let file = path.rsplit('/').next()?;
            let lower = file.to_ascii_lowercase();
            if !lower.ends_with(".uasset") || !lower.starts_with('t') {
                return None;
            }
            let has_suffix = suffixes.iter().any(|suffix| lower.contains(suffix));
            if !has_suffix {
                return None;
            }
            let has_stem = lower.contains(&stem);
            let has_hint = hints
                .iter()
                .any(|hint| hint.len() > 2 && lower.contains(hint));
            if !has_stem && !has_hint {
                return None;
            }
            Some((
                shared_directory_len(material_dir, path),
                path.len(),
                path.clone(),
            ))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().map(|(_, _, path)| path).collect()
}

/// 从材质文件名提取用于匹配贴图的资产名（去掉 `M_`/`MI_`/`MAT_` 前缀）。
fn material_stem(file: &str) -> String {
    let stem = file
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file)
        .to_ascii_lowercase();
    for prefix in ["mat-", "mat_", "mi-", "mi_", "m-", "m_"] {
        if let Some(rest) = stem.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    stem
}

/// 贴图用途对应的命名后缀。
fn texture_kind_suffixes(kind: MaterialTextureKind) -> &'static [&'static str] {
    match kind {
        MaterialTextureKind::BaseColor => &[
            "_basecolor",
            "_base_color",
            "_albedo",
            "_diffuse",
            "_bc",
            "_d",
            "_color",
        ],
        MaterialTextureKind::Normal => &["_normal", "_normals", "_nrm", "_nml", "_n"],
        MaterialTextureKind::MetallicRoughness => &[
            "_orm",
            "_occlusionroughnessmetallic",
            "_roughness",
            "_metallicroughness",
            "_or",
            "_mr",
            "_r",
        ],
        MaterialTextureKind::Occlusion => &["_ao", "_occlusion", "_ambientocclusion"],
        _ => &[],
    }
}

/// 把 `/Game/...` 包路径映射成当前容器内的相对路径。
fn package_to_relative(current_relative: &str, package: &str) -> Option<String> {
    let rest = package.strip_prefix("/Game/")?;
    let content = current_relative.find("/Content/")?;
    let prefix = &current_relative[..content + "/Content/".len()];
    let rest = rest.trim_end_matches(".uasset");
    Some(format!("{prefix}{rest}.uasset"))
}

/// 两个路径共享目录前缀的长度（用于排序候选）。
fn shared_directory_len(left_dir: &str, right: &str) -> usize {
    let right_dir = right.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(right);
    let mut shared = 0;
    for (a, b) in left_dir.chars().zip(right_dir.chars()) {
        if a != b {
            break;
        }
        shared += 1;
    }
    // 回退到目录分隔符，避免把部分目录名当公共前缀。
    left_dir[..shared].rfind('/').unwrap_or(0)
}

/// 贴图尺寸上限；0 表示不缩放（由 [`MaterialViewerImage`] 内部兜底）。
fn size_limit(max_size: u32) -> u32 {
    if max_size == 0 { u32::MAX } else { max_size }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/Game/` 路径映射为当前容器的相对 `.uasset` 路径。
    #[test]
    fn maps_package_paths_to_container_relative_paths() {
        let current = "Proj/Content/Art/Meshes/SM_Box.uasset";
        assert_eq!(
            package_to_relative(current, "/Game/Art/Materials/M_Wood").as_deref(),
            Some("Proj/Content/Art/Materials/M_Wood.uasset")
        );
        assert!(package_to_relative(current, "/Engine/BasicShapes/Cube").is_none());
    }

    /// 槽位名优先匹配网格同目录的同名材质。
    #[test]
    fn prefers_material_next_to_mesh() {
        let packages = vec![
            "Proj/Content/Art/Meshes/M_Wood.uasset".to_string(),
            "Proj/Content/Art/Materials/M_Wood.uasset".to_string(),
            "Proj/Content/Art/Materials/M_Stone.uasset".to_string(),
        ];
        let slot = MaterialSlot {
            name: "M_Wood".to_string(),
            package: None,
        };
        let found =
            find_material_package("Proj/Content/Art/Meshes/SM_Box.uasset", &slot, &packages);
        assert_eq!(
            found.as_deref(),
            Some("Proj/Content/Art/Meshes/M_Wood.uasset")
        );
    }

    /// 没有同目录材质时按“与网格目录公共前缀最长”选择。
    #[test]
    fn ranks_material_candidates_by_shared_directory() {
        let packages = vec![
            "Proj/Content/Other/M_Wood.uasset".to_string(),
            "Proj/Content/Art/Materials/M_Wood.uasset".to_string(),
        ];
        let slot = MaterialSlot {
            name: "M_Wood".to_string(),
            package: None,
        };
        let found =
            find_material_package("Proj/Content/Art/Meshes/SM_Box.uasset", &slot, &packages);
        assert_eq!(
            found.as_deref(),
            Some("Proj/Content/Art/Materials/M_Wood.uasset")
        );
    }

    /// 按材质名与用途后缀挑选贴图。
    #[test]
    fn finds_textures_by_material_stem_and_kind() {
        let packages = vec![
            "Proj/Content/Art/Materials/M_Wood.uasset".to_string(),
            "Proj/Content/Art/Textures/T_Wood_BaseColor.uasset".to_string(),
            "Proj/Content/Art/Textures/T_Wood_Normal.uasset".to_string(),
            "Proj/Content/Art/Textures/T_Wood_ORM.uasset".to_string(),
            "Proj/Content/Art/Textures/T_Stone_BaseColor.uasset".to_string(),
        ];
        let appearance = MaterialAppearance::default();
        let base = texture_candidates(
            "Proj/Content/Art/Materials/M_Wood.uasset",
            &appearance,
            MaterialTextureKind::BaseColor,
            &packages,
        );
        assert_eq!(
            base.first().map(String::as_str),
            Some("Proj/Content/Art/Textures/T_Wood_BaseColor.uasset")
        );
        let normal = texture_candidates(
            "Proj/Content/Art/Materials/M_Wood.uasset",
            &appearance,
            MaterialTextureKind::Normal,
            &packages,
        );
        assert_eq!(
            normal.first().map(String::as_str),
            Some("Proj/Content/Art/Textures/T_Wood_Normal.uasset")
        );
        let orm = texture_candidates(
            "Proj/Content/Art/Materials/M_Wood.uasset",
            &appearance,
            MaterialTextureKind::MetallicRoughness,
            &packages,
        );
        assert_eq!(
            orm.first().map(String::as_str),
            Some("Proj/Content/Art/Textures/T_Wood_ORM.uasset")
        );
    }

    /// 材质资产名去掉常见前缀后再匹配贴图。
    #[test]
    fn strips_material_prefixes_for_texture_matching() {
        assert_eq!(material_stem("M_Wood.uasset"), "wood");
        assert_eq!(material_stem("MI_Wood_Inst.uasset"), "wood_inst");
        assert_eq!(material_stem("MAT-Wood.uasset"), "wood");
    }

    /// 按金属/粗糙度贴图命名识别 ORM 通道布局。
    #[test]
    fn detects_orm_layout_from_texture_name() {
        assert_eq!(
            detect_orm_layout("T_Nana_ORM"),
            lumark_scene::OrmLayout::AoRoughMetal
        );
        assert_eq!(
            detect_orm_layout("T_Nana_OcclusionRoughnessMetallic"),
            lumark_scene::OrmLayout::AoRoughMetal
        );
        assert_eq!(
            detect_orm_layout("T_Nana_SpecularMask"),
            lumark_scene::OrmLayout::SpecularMask
        );
        assert_eq!(
            detect_orm_layout("T_Nana_SM"),
            lumark_scene::OrmLayout::SpecularMask
        );
    }

    /// 槽名与材质资产名不一致时按前缀/分词做模糊匹配，并优先精确同名。
    #[test]
    fn fuzzy_matches_slot_names_to_materials() {
        let packages = vec![
            "Game/Content/CH/Nana/Cloth/MI_C_N01.uasset".to_string(),
            "Game/Content/CH/Nana/M/N_Lips.uasset".to_string(),
            "Game/Content/CH/Nana/M/N_Body_Inst6.uasset".to_string(),
            "Game/Content/CH/Nana/M/N_Nail_Inst1.uasset".to_string(),
            "Game/Content/CH/Nana/N_All/SK_Body.uasset".to_string(),
            "Game/Content/CH/Nana/N_All/T_N_Lip.uasset".to_string(),
        ];
        let mesh = "Game/Content/CH/Nana/N_All/NanaF6_Hold.uasset";
        let pick = |name: &str| {
            find_material_package(
                mesh,
                &crate::MaterialSlot {
                    name: name.to_string(),
                    package: None,
                },
                &packages,
            )
        };
        assert_eq!(
            pick("N_Lip").as_deref(),
            Some("Game/Content/CH/Nana/M/N_Lips.uasset")
        );
        assert_eq!(
            pick("N_Nail").as_deref(),
            Some("Game/Content/CH/Nana/M/N_Nail_Inst1.uasset")
        );
        // 精确同名仍优先于模糊匹配。
        assert_eq!(
            pick("MI_C_N01").as_deref(),
            Some("Game/Content/CH/Nana/Cloth/MI_C_N01.uasset")
        );
        // 没有合理候选时保持未解析，避免随机错配材质。
        assert_eq!(pick("Zzz_Unknown"), None);
    }
}
