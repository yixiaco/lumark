//! 网格材质槽解析：槽位 → 材质包 → 父材质链 → 贴图。
//!
//! 查看器和后续导出都需要把 `StaticMesh`/`SkeletalMesh` 的材质槽对应到具体
//! 材质资产，并尽量加载基础色/法线/ORM 贴图。Zen 包不暴露对象引用的包路径，
//! 因此这里以材质槽名、资产目录和贴图命名约定为线索；旧式包则沿导入表读取
//! 父材质和贴图包。

use crate::LegacyPackage;
use crate::package::{
    Mappings, MaterialAppearance, MaterialSlot, MaterialTextureKind, MaterialViewerData,
    MaterialViewerImage, material_parent_package, read_legacy_material_appearance,
    read_zen_material_appearance, texture_references,
};
use crate::source::GameAssetSource;
use crate::{is_legacy_package, parse_package, read_legacy_texture};

/// 材质解析选项。
#[derive(Debug, Clone)]
pub struct MaterialResolveOptions {
    /// 是否加载并解码贴图。
    pub load_textures: bool,
    /// 贴图最大边长；0 表示不缩放。
    pub max_texture_size: u32,
    /// 父材质链最大深度。
    pub max_parent_depth: usize,
}

impl Default for MaterialResolveOptions {
    fn default() -> Self {
        Self {
            load_textures: true,
            max_texture_size: 512,
            max_parent_depth: 4,
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
    let packages = package_index(source);
    slots
        .iter()
        .map(|slot| resolve_one(source, mesh_path, slot, mappings, options, &packages))
        .collect()
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
    let mut appearance = match load_material(
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
    if options.load_textures {
        let base = resolve_texture(
            source,
            path,
            &appearance,
            MaterialTextureKind::BaseColor,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        let normal = resolve_texture(
            source,
            path,
            &appearance,
            MaterialTextureKind::Normal,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        let orm = resolve_texture(
            source,
            path,
            &appearance,
            MaterialTextureKind::MetallicRoughness,
            mappings,
            options,
            packages,
            &mut resolved.loaded_assets,
        );
        if base.is_some() {
            // 已加载基础色贴图时让材质球使用白色基色，避免与贴图重复着色。
            appearance.base_color = [1.0, 1.0, 1.0];
        }
        resolved.viewer = Some(MaterialViewerData {
            appearance: appearance.clone(),
            base_color: base,
            normal,
            metallic_roughness: orm,
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
    Some(read_zen_material_appearance(&package, mappings))
}

/// 加载一张贴图为查看器图像。
fn load_texture_image(
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

/// 解析一张贴图：优先精确包路径，其次按用途与命名约定搜索。
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
        if let Some(package) = &reference.package
            && let Some(relative) = package_to_relative(material_path, package)
            && let Some(image) =
                load_texture_image(source, &relative, mappings, options.max_texture_size)
        {
            loaded.push(relative);
            return Some(image);
        }
        if reference.name.to_ascii_lowercase().starts_with("t_")
            && let Some(path) = find_texture_by_filename(packages, &reference.name)
            && let Some(image) =
                load_texture_image(source, &path, mappings, options.max_texture_size)
        {
            loaded.push(path);
            return Some(image);
        }
    }
    for candidate in texture_candidates(material_path, appearance, kind, packages) {
        if let Some(image) =
            load_texture_image(source, &candidate, mappings, options.max_texture_size)
        {
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
    candidates.first().map(|(_, _, path)| (*path).clone())
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
}
