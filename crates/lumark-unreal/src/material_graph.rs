//! Zen（UE5 IoStore）材质图还原。
//!
//! 材质实例的参数覆盖（`TextureParameterValues` 等）、父材质链和对象引用都
//! 通过包 ImportMap 解析到目标包 ID + 公共导出哈希，再按包 ID 读取目标包，
//! 因此不需要依赖“同目录同名”启发式。这里实现三条链路：
//!
//! 1. `TextureParameterValues` → 贴图对象 → 目标包/导出 → 贴图引用（带包 ID）；
//! 2. `Parent` → 父材质包/导出 → 递归读取外观并按“子覆盖父”合并；
//! 3. `ReferencedTextures`（属性或 `CachedExpressionData` 结构内）→ 兜底贴图分类。
//!
//! 贴图用途选择对齐 FModel `CMaterialParams2`：
//! - 先按参数名精确命中各通道的触发名列表（`Color`/`Diffuse`/`Normal`/`ORM` 等）；
//! - 再按参数名/贴图名正则兜底（`_D`、`_N`、`_SM` 等后缀）；
//! - 基础色最后回退到材质链上的第一张贴图（FModel 的 first-texture 规则）。
//!
//! 旧式（UE4 PAK）材质继续走导入表路径，不经过本模块。

use std::collections::HashMap;

use crate::imports::{find_export_by_public_hash, resolve_package_import};
use crate::material_params::{
    AO_BOOST_TRIGGERS, AO_TRIGGERS, DIFFUSE_COLOR_TRIGGERS, DIFFUSE_TRIGGERS,
    EMISSIVE_COLOR_TRIGGERS, EMISSIVE_MULT_COLOR_TRIGGERS, EMISSIVE_MULT_TRIGGERS,
    EMISSIVE_TRIGGERS, NORMAL_TRIGGERS, ROUGHNESS_MAX_TRIGGERS, ROUGHNESS_MIN_TRIGGERS,
    ROUGHNESS_TRIGGERS, SPECULAR_TRIGGERS, matches_diffuse, matches_emissive, matches_normals,
    matches_specular_masks,
};
use crate::package::{
    Mappings, MaterialAppearance, MaterialTextureKind, MaterialTextureRef, MaterialViewerImage,
    PackageInfo, PropValue, Texture2D, UnrealPackage, blend_mode_name, blend_mode_property,
    is_base_color_name, is_legacy_package, is_overlay_name, parameter_array,
    parameter_entry_color4, parameter_entry_name, parameter_entry_scalar, parse_package,
    shading_model_name, two_sided_property,
};
use crate::source::GameAssetSource;

/// 一个导出对象的类名与名称。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportMeta {
    /// 导出名称。
    pub name: String,
    /// 类名。
    pub class_name: String,
}

/// 跨包材质图缓存：已解析包、导入包表、导出元数据和材质外观。
#[derive(Default)]
pub(crate) struct MaterialGraph {
    packages: HashMap<u64, UnrealPackage>,
    imported_ids: HashMap<u64, Vec<u64>>,
    exports: HashMap<(u64, u64), Option<ExportMeta>>,
    appearances: HashMap<(u64, u64), Option<MaterialAppearance>>,
}

impl MaterialGraph {
    /// 创建空缓存。
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// 一条已解析的材质槽对象引用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaterialObjectRef {
    /// 目标包 ID。
    pub package_id: u64,
    /// 目标公共导出哈希。
    pub export_hash: u64,
}

/// 材质链上的一条贴图参数（按父材质→子材质顺序收集）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChainTexture {
    /// 参数原名，例如 `Color`、`Fabric_D`；`ReferencedTextures` 为空串。
    parameter: String,
    /// 贴图对象名，例如 `NewNana_D`。
    name: String,
    /// 贴图目标包 ID。
    package_id: u64,
}

/// 材质链参数集合：父材质参数在前、子材质在后。
#[derive(Debug, Default)]
struct ChainParams {
    /// 贴图参数（父→子顺序，子材质同名参数靠后）。
    textures: Vec<ChainTexture>,
    /// 颜色参数（父→子顺序，含四分量）。
    colors: Vec<(String, [f32; 4])>,
    /// 标量参数（父→子顺序）。
    scalars: Vec<(String, f32)>,
}

/// 读取包内 `import_index` 指向的对象引用；非包导入返回 `None`。
pub(crate) fn resolve_material_object(
    info: &PackageInfo,
    imported_ids: &[u64],
    import_index: usize,
) -> Option<MaterialObjectRef> {
    let (package_id, export_hash) =
        resolve_package_import(info, imported_ids, import_index).ok()?;
    Some(MaterialObjectRef {
        package_id,
        export_hash,
    })
}

/// 从 `PropValue` 的 `Object` 字段提取导入索引（负值）。
pub(crate) fn object_import_index(value: &PropValue) -> Option<usize> {
    match value {
        PropValue::Object(index) if *index < 0 => Some((-*index - 1) as usize),
        _ => None,
    }
}

/// 读取目标包中指定导出的类名与名称（只解析包表，不读属性）。
pub(crate) fn export_meta(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    object: MaterialObjectRef,
) -> Option<ExportMeta> {
    let key = (object.package_id, object.export_hash);
    if let Some(cached) = graph.exports.get(&key) {
        return cached.clone();
    }
    let meta = read_export_meta_inner(source, graph, object);
    graph.exports.insert(key, meta.clone());
    meta
}

/// 实际读取包并查找导出。
fn read_export_meta_inner(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    object: MaterialObjectRef,
) -> Option<ExportMeta> {
    ensure_package(source, graph, object.package_id)?;
    let package = graph.packages.get(&object.package_id)?;
    let exports = &package.info().exports;
    let index = if object.export_hash == 0 {
        exports
            .iter()
            .position(|export| export.class_name.to_ascii_lowercase().contains("material"))?
    } else {
        find_export_by_public_hash(exports, object.export_hash)?
    };
    let export = exports.get(index)?;
    Some(ExportMeta {
        name: export.name.clone(),
        class_name: export.class_name.clone(),
    })
}

/// 确保包已读取并解析进缓存。
pub(crate) fn ensure_package(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    package_id: u64,
) -> Option<()> {
    if graph.packages.contains_key(&package_id) {
        return Some(());
    }
    let (uasset, _ubulk) = source.read_package_by_id(package_id).ok()?;
    let package = parse_package(uasset, source.scripts(), None).ok()?;
    graph.packages.insert(package_id, package);
    Some(())
}

/// 返回包的导入包 ID 表（缓存）。
pub(crate) fn imported_package_ids(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    package_id: u64,
) -> Vec<u64> {
    if let Some(ids) = graph.imported_ids.get(&package_id) {
        return ids.clone();
    }
    let ids = source.imported_packages(package_id).unwrap_or_default();
    graph.imported_ids.insert(package_id, ids.clone());
    ids
}

/// 从包 ID + 导出哈希递归读取材质外观（含父链与对象引用贴图）。
///
/// `depth` 为父材质层级；超过 [`MAX_PARENT_DEPTH`] 时停止递归并记录 warning。
pub(crate) fn load_material_by_id(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    object: MaterialObjectRef,
    mappings: &Mappings,
    depth: usize,
    loaded: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<MaterialAppearance> {
    let key = (object.package_id, object.export_hash);
    if let Some(cached) = graph.appearances.get(&key) {
        return cached.clone();
    }
    let mut chain = ChainParams::default();
    let appearance = load_material_recursive(
        source, graph, object, mappings, depth, loaded, warnings, &mut chain,
    )
    .map(|mut appearance| {
        apply_chain_selection(&mut appearance, &chain);
        appearance
    });
    graph.appearances.insert(key, appearance.clone());
    appearance
}

/// 父材质最大递归深度。
const MAX_PARENT_DEPTH: usize = 8;

/// 先递归父材质（父参数先入链、外观用于子覆盖父），再读取本节点参数。
#[allow(clippy::too_many_arguments)]
fn load_material_recursive(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    object: MaterialObjectRef,
    mappings: &Mappings,
    depth: usize,
    loaded: &mut Vec<String>,
    warnings: &mut Vec<String>,
    chain: &mut ChainParams,
) -> Option<MaterialAppearance> {
    if depth > MAX_PARENT_DEPTH {
        warnings.push("材质父链超过最大深度，已停止".to_string());
        return None;
    }
    ensure_package(source, graph, object.package_id)?;
    let imported_ids = imported_package_ids(source, graph, object.package_id);
    let (index, info) = {
        let package = graph.packages.get(&object.package_id)?;
        let info = package.info();
        let index = if object.export_hash == 0 {
            info.exports
                .iter()
                .position(|export| is_material_export(&export.class_name))?
        } else {
            find_export_by_public_hash(&info.exports, object.export_hash)?
        };
        (index, info.clone())
    };
    let properties = {
        let package = graph.packages.get(&object.package_id)?;
        package.read_export_properties(index, mappings).ok()?
    };
    let export = info.exports.get(index)?;
    loaded.push(format!(
        "package:0x{:016X}/{}",
        object.package_id, export.name
    ));

    // 父材质：先递归，保证参数链顺序为父→子。
    let mut parent_appearance = None;
    if let Some(parent_value) = properties
        .properties
        .iter()
        .find(|(key, _)| key == "Parent")
        .map(|(_, value)| value)
        && let Some(import_index) = object_import_index(parent_value)
        && let Some(parent_object) = resolve_material_object(&info, &imported_ids, import_index)
    {
        match load_material_recursive(
            source,
            graph,
            parent_object,
            mappings,
            depth + 1,
            loaded,
            warnings,
            chain,
        ) {
            Some(parent) => parent_appearance = Some(parent),
            None => warnings.push(format!(
                "父材质 0x{:016X} 无法解析",
                parent_object.package_id
            )),
        }
    }

    let (mut appearance, node_chain) = appearance_from_properties(
        source,
        graph,
        &info,
        &imported_ids,
        &properties.properties,
        loaded,
        warnings,
    );
    chain.textures.extend(node_chain.textures);
    chain.colors.extend(node_chain.colors);
    chain.scalars.extend(node_chain.scalars);
    if let Some(parent) = parent_appearance {
        appearance.merge_parent(&parent);
    }
    Some(appearance)
}

/// 判断类名是否材质/材质实例。
fn is_material_export(class_name: &str) -> bool {
    class_name.to_ascii_lowercase().contains("material")
}

/// 应用材质链选择：贴图用途、颜色 tint、粗糙度区间、发光倍率与 AO 增强。
fn apply_chain_selection(appearance: &mut MaterialAppearance, chain: &ChainParams) {
    let textures = selected_textures(chain);
    let has_base = textures
        .iter()
        .any(|texture| texture.kind == MaterialTextureKind::BaseColor);
    let has_emissive = textures
        .iter()
        .any(|texture| texture.kind == MaterialTextureKind::Emissive);
    if has_base {
        // FModel 只在存在基础色贴图时应用 DiffuseColor；默认白色由贴图提供颜色。
        if let Some(tint) = select_color3(chain, DIFFUSE_COLOR_TRIGGERS) {
            appearance.base_color = tint;
        } else {
            appearance.base_color = [1.0, 1.0, 1.0];
        }
    }
    if has_emissive && let Some(emissive) = select_color3(chain, EMISSIVE_COLOR_TRIGGERS) {
        appearance.emissive = emissive;
    }
    // FModel：RoughnessMin/Max 优先；否则单值 Roughness 生成 ±value/2 区间。
    let single = select_scalar(chain, ROUGHNESS_TRIGGERS);
    let mut min = select_scalar(chain, ROUGHNESS_MIN_TRIGGERS)
        .or_else(|| single.map(|value| value - value / 2.0))
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let mut max = select_scalar(chain, ROUGHNESS_MAX_TRIGGERS)
        .or_else(|| single.map(|value| value + value / 2.0))
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }
    appearance.roughness_min = min;
    appearance.roughness_max = max;
    if let Some(multiplier) = select_scalar(chain, EMISSIVE_MULT_TRIGGERS)
        .or_else(|| select_color3(chain, EMISSIVE_MULT_COLOR_TRIGGERS).map(|color| color[0]))
    {
        appearance.emissive_multiplier = multiplier.max(0.0);
    }
    if let Some(boost) = select_color4(chain, AO_BOOST_TRIGGERS) {
        appearance.ao_boost_color = [boost[0], boost[1], boost[2]];
        appearance.ao_boost_exponent = boost[3];
        appearance.has_ao_boost = true;
    }
    appearance.textures = textures;
}

/// 从材质链选择各通道的贴图引用（每个用途最多一张）。
fn selected_textures(chain: &ChainParams) -> Vec<MaterialTextureRef> {
    let mut textures = Vec::new();
    if let Some(texture) = select_texture(
        chain,
        DIFFUSE_TRIGGERS,
        matches_diffuse,
        // FModel 的 first-texture 兜底仅用于基础色。
        true,
    ) {
        textures.push(texture_ref(texture, MaterialTextureKind::BaseColor));
    }
    if let Some(texture) = select_texture(chain, NORMAL_TRIGGERS, matches_normals, false) {
        textures.push(texture_ref(texture, MaterialTextureKind::Normal));
    }
    if let Some(texture) = select_texture(chain, SPECULAR_TRIGGERS, matches_specular_masks, false) {
        textures.push(texture_ref(texture, MaterialTextureKind::MetallicRoughness));
    }
    if let Some(texture) = select_texture(chain, EMISSIVE_TRIGGERS, matches_emissive, false) {
        textures.push(texture_ref(texture, MaterialTextureKind::Emissive));
    }
    if let Some(texture) = select_texture(chain, AO_TRIGGERS, matches_none, false) {
        textures.push(texture_ref(texture, MaterialTextureKind::Occlusion));
    }
    textures
}

/// 该通道只做参数名精确匹配，不做正则兜底。
fn matches_none(_: &str) -> bool {
    false
}

/// 构造贴图引用。
fn texture_ref(texture: &ChainTexture, kind: MaterialTextureKind) -> MaterialTextureRef {
    MaterialTextureRef {
        name: texture.name.clone(),
        package: None,
        package_id: Some(texture.package_id),
        kind,
    }
}

/// 选择某个通道的贴图：触发名优先 → 正则兜底 → （基础色）第一张贴图。
fn select_texture<'a>(
    chain: &'a ChainParams,
    triggers: &[&str],
    matches: fn(&str) -> bool,
    first_fallback: bool,
) -> Option<&'a ChainTexture> {
    for trigger in triggers {
        if let Some(texture) = chain
            .textures
            .iter()
            .rev()
            .find(|texture| texture.parameter.eq_ignore_ascii_case(trigger))
        {
            return Some(texture);
        }
    }
    if let Some(texture) = chain
        .textures
        .iter()
        .rev()
        .find(|texture| matches(&texture.parameter) || matches(&texture.name))
    {
        return Some(texture);
    }
    if first_fallback {
        return chain.textures.first();
    }
    None
}

/// 选择颜色参数（触发名优先，子材质覆盖父材质），返回四分量。
fn select_color4(chain: &ChainParams, triggers: &[&str]) -> Option<[f32; 4]> {
    for trigger in triggers {
        if let Some((_, color)) = chain
            .colors
            .iter()
            .rev()
            .find(|(name, _)| name.eq_ignore_ascii_case(trigger))
        {
            return Some(*color);
        }
    }
    None
}

/// 选择颜色参数并取 RGB。
fn select_color3(chain: &ChainParams, triggers: &[&str]) -> Option<[f32; 3]> {
    select_color4(chain, triggers).map(|color| [color[0], color[1], color[2]])
}

/// 选择标量参数（触发名优先，子材质覆盖父材质）。
fn select_scalar(chain: &ChainParams, triggers: &[&str]) -> Option<f32> {
    for trigger in triggers {
        if let Some((_, value)) = chain
            .scalars
            .iter()
            .rev()
            .find(|(name, _)| name.eq_ignore_ascii_case(trigger))
        {
            return Some(*value);
        }
    }
    None
}

/// 从属性构造本节点外观与参数链。
#[allow(clippy::too_many_arguments)]
fn appearance_from_properties(
    source: &mut GameAssetSource,
    graph: &mut MaterialGraph,
    info: &PackageInfo,
    imported_ids: &[u64],
    properties: &[(String, PropValue)],
    loaded: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> (MaterialAppearance, ChainParams) {
    let mut appearance = MaterialAppearance::default();
    let mut chain = ChainParams::default();
    if let Some(two_sided) = two_sided_property(properties) {
        appearance.two_sided = two_sided;
    }
    let (masked, translucent) = blend_mode_property(properties);
    appearance.masked |= masked;
    appearance.translucent |= translucent;
    if let Some(blend) = blend_mode_name(properties) {
        appearance.blend_mode = blend;
    }
    if let Some(shading) = shading_model_name(properties) {
        appearance.shading_model = shading;
    }
    let mut base_color = None;
    let mut emissive = None;
    for entry in parameter_array(properties, "VectorParameterValues") {
        let Some(name) = parameter_entry_name(entry) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if is_overlay_name(&lower) {
            continue;
        }
        let Some(color) = parameter_entry_color4(entry) else {
            continue;
        };
        chain.colors.push((name.clone(), color));
        let rgb = [color[0], color[1], color[2]];
        if lower.contains("emiss") || lower.contains("glow") {
            emissive = Some(rgb);
        } else if is_base_color_name(Some(&name)) {
            base_color = Some(rgb);
        }
    }
    let mut metallic = None;
    let mut roughness = None;
    for entry in parameter_array(properties, "ScalarParameterValues") {
        let Some(name) = parameter_entry_name(entry) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if is_overlay_name(&lower) {
            continue;
        }
        let Some(value) = parameter_entry_scalar(entry) else {
            continue;
        };
        chain.scalars.push((name.clone(), value));
        if lower.contains("metallic") && !lower.contains("overwrite") {
            metallic = Some(value);
        } else if lower.contains("roughness") && !lower.contains("min") && !lower.contains("max") {
            roughness = Some(value);
        }
    }
    if let Some(color) = base_color {
        appearance.base_color = color;
    }
    if let Some(color) = emissive {
        appearance.emissive = color;
    }
    if let Some(value) = metallic {
        appearance.metallic = value.clamp(0.0, 1.0);
    }
    if let Some(value) = roughness {
        appearance.roughness = value.clamp(0.04, 1.0);
    }

    // TextureParameterValues：参数名 → 贴图对象 → 目标包/导出（只收集，不分类）。
    for entry in parameter_array(properties, "TextureParameterValues") {
        let Some(parameter_name) = parameter_entry_name(entry) else {
            continue;
        };
        let Some(value) = parameter_entry_object(entry) else {
            continue;
        };
        let Some(import_index) = object_import_index(value) else {
            continue;
        };
        let Some(object) = resolve_material_object(info, imported_ids, import_index) else {
            warnings.push(format!("贴图参数 {parameter_name} 的导入无法解析"));
            continue;
        };
        let Some(meta) = export_meta(source, graph, object) else {
            continue;
        };
        if !meta.class_name.to_ascii_lowercase().contains("texture") {
            warnings.push(format!(
                "贴图参数 {parameter_name} 指向非贴图对象 {}",
                meta.class_name
            ));
            continue;
        }
        chain.textures.push(ChainTexture {
            parameter: parameter_name,
            name: meta.name.clone(),
            package_id: object.package_id,
        });
        loaded.push(format!("texture:{}", meta.name));
    }

    // ReferencedTextures（部分游戏把 CachedExpressionData 作为属性序列化）。
    let referenced = property_array(properties, "ReferencedTextures")
        .or_else(|| cached_referenced_textures(properties));
    if let Some(values) = referenced {
        for value in values {
            let Some(import_index) = object_import_index(value) else {
                continue;
            };
            let Some(object) = resolve_material_object(info, imported_ids, import_index) else {
                continue;
            };
            let Some(meta) = export_meta(source, graph, object) else {
                continue;
            };
            if !meta.class_name.to_ascii_lowercase().contains("texture") {
                continue;
            }
            chain.textures.push(ChainTexture {
                parameter: String::new(),
                name: meta.name.clone(),
                package_id: object.package_id,
            });
        }
    }
    (appearance, chain)
}

/// 读取 `ReferencedTextures` 数组属性。
fn property_array<'a>(
    properties: &'a [(String, PropValue)],
    name: &str,
) -> Option<&'a [PropValue]> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Array(values) => Some(values.as_slice()),
            _ => None,
        })
}

/// 从 `CachedExpressionData` 结构属性中取 `ReferencedTextures`。
fn cached_referenced_textures(properties: &[(String, PropValue)]) -> Option<&[PropValue]> {
    let cached = properties
        .iter()
        .find(|(key, _)| key == "CachedExpressionData")
        .map(|(_, value)| value)?;
    let PropValue::Struct(fields) = cached else {
        return None;
    };
    property_array(fields, "ReferencedTextures")
}

/// 读取贴图参数条目中的对象引用值。
fn parameter_entry_object(entry: &PropValue) -> Option<&PropValue> {
    let PropValue::Struct(fields) = entry else {
        return None;
    };
    fields
        .iter()
        .find(|(key, _)| key == "ParameterValue")
        .map(|(_, value)| value)
}

/// 按包 ID 解码一张贴图为查看器图像。
pub(crate) fn decode_texture_by_id(
    source: &mut GameAssetSource,
    package_id: u64,
    mappings: &Mappings,
    max_size: u32,
) -> Option<MaterialViewerImage> {
    let (uasset, ubulk) = source.read_package_by_id(package_id).ok()?;
    if is_legacy_package(&uasset) {
        return None;
    }
    let package = parse_package(uasset, source.scripts(), None).ok()?;
    let index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "Texture2D")?;
    let texture: Texture2D = package
        .read_texture2d(index, mappings, ubulk.as_deref())
        .ok()?;
    let limit = max_size.clamp(16, 4096);
    let level = crate::package::mip_level_for_limit(
        texture.width,
        texture.height,
        texture.mips.len(),
        limit,
    );
    let (rgba, width, height) = texture.decode_mip_rgba_with_size(level).ok()?;
    MaterialViewerImage::new(rgba, width, height, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条贴图参数链项。
    fn texture(parameter: &str, name: &str, package_id: u64) -> ChainTexture {
        ChainTexture {
            parameter: parameter.to_string(),
            name: name.to_string(),
            package_id,
        }
    }

    /// 基础色：参数名命中触发列表优先（`Color` 覆盖遮罩类 `RGB`）。
    #[test]
    fn selects_diffuse_by_parameter_trigger() {
        let chain = ChainParams {
            textures: vec![
                texture("RGB", "NA_RGB", 1),
                texture("Color", "NewNana_D", 2),
                texture("Normal", "FlatN", 3),
            ],
            ..Default::default()
        };
        let selected = selected_textures(&chain);
        assert_eq!(selected[0].kind, MaterialTextureKind::BaseColor);
        assert_eq!(selected[0].name, "NewNana_D");
        assert_eq!(selected[0].package_id, Some(2));
        assert_eq!(selected[1].kind, MaterialTextureKind::Normal);
        assert_eq!(selected[1].name, "FlatN");
    }

    /// AO：FModel `Material.Setup` 的 `M`/`AEM`/`AO` 参数精确匹配。
    #[test]
    fn selects_occlusion_texture_by_parameter() {
        let chain = ChainParams {
            textures: vec![
                texture("AO", "NewNana_AO", 20),
                texture("Color", "NewNana_D", 21),
            ],
            ..Default::default()
        };
        let selected = selected_textures(&chain);
        assert!(
            selected.iter().any(|texture| {
                texture.kind == MaterialTextureKind::Occlusion && texture.name == "NewNana_AO"
            }),
            "AO 参数应映射为环境光遮蔽贴图"
        );
    }

    /// 颜色 tint：按 FModel DiffuseColors 触发名选择，子材质覆盖父材质。
    #[test]
    fn selects_diffuse_color_tint() {
        let chain = ChainParams {
            textures: Vec::new(),
            colors: vec![
                ("BaseColortint".to_string(), [0.2, 0.3, 0.4, 1.0]),
                ("Color".to_string(), [0.9, 0.8, 0.7, 1.0]),
            ],
            ..Default::default()
        };
        assert_eq!(
            select_color3(&chain, DIFFUSE_COLOR_TRIGGERS),
            Some([0.9, 0.8, 0.7])
        );
        // 非触发名颜色不参与基础色 tint。
        let only_tint = ChainParams {
            textures: Vec::new(),
            colors: vec![("BaseColortint".to_string(), [0.2, 0.3, 0.4, 1.0])],
            ..Default::default()
        };
        assert_eq!(select_color3(&only_tint, DIFFUSE_COLOR_TRIGGERS), None);
    }

    /// BNB 的 `AO` 参数既作 AO 又作 FModel specular mask。
    #[test]
    fn ao_parameter_feeds_specular_mask_and_occlusion() {
        let chain = ChainParams {
            textures: vec![
                texture("AO", "NewNana_AO", 7),
                texture("Color", "NewNana_D", 8),
                texture("Normal", "FlatN", 9),
            ],
            ..Default::default()
        };
        let selected = selected_textures(&chain);
        assert!(selected.iter().any(|texture| {
            texture.kind == MaterialTextureKind::MetallicRoughness && texture.name == "NewNana_AO"
        }));
        assert!(
            selected
                .iter()
                .any(|texture| texture.kind == MaterialTextureKind::Occlusion
                    && texture.name == "NewNana_AO")
        );
    }

    /// 粗糙度区间、自发光倍率与 AO 增强按 FModel 触发名选择。
    #[test]
    fn selects_fmodel_scalar_and_boost_parameters() {
        let chain = ChainParams {
            textures: vec![
                texture("Color", "NewNana_D", 1),
                texture("AO", "NewNana_AO", 2),
            ],
            scalars: vec![
                ("RoughnessMin".to_string(), 0.2),
                ("RoughnessMax".to_string(), 0.9),
                ("Emissive_Mult".to_string(), 1.7),
            ],
            colors: vec![(
                "Skin Boost Color And Exponent".to_string(),
                [1.2, 1.1, 1.0, 2.5],
            )],
        };
        let mut appearance = MaterialAppearance::default();
        apply_chain_selection(&mut appearance, &chain);
        assert!((appearance.roughness_min - 0.2).abs() < 1e-6);
        assert!((appearance.roughness_max - 0.9).abs() < 1e-6);
        assert!((appearance.emissive_multiplier - 1.7).abs() < 1e-6);
        assert!(appearance.has_ao_boost);
        assert_eq!(appearance.ao_boost_color, [1.2, 1.1, 1.0]);
        assert!((appearance.ao_boost_exponent - 2.5).abs() < 1e-6);
    }

    /// 单值 Roughness 生成 ±value/2 的区间；没有基础色贴图时不应用 tint。
    #[test]
    fn single_roughness_and_tint_require_base_texture() {
        let chain = ChainParams {
            textures: Vec::new(),
            scalars: vec![("Roughness".to_string(), 0.4)],
            colors: vec![("Color".to_string(), [0.9, 0.2, 0.1, 1.0])],
        };
        let mut appearance = MaterialAppearance::default();
        apply_chain_selection(&mut appearance, &chain);
        assert!((appearance.roughness_min - 0.2).abs() < 1e-6);
        assert!((appearance.roughness_max - 0.6).abs() < 1e-6);
        assert_eq!(
            appearance.base_color,
            MaterialAppearance::default().base_color,
            "没有基础色贴图时 FModel 不应用 DiffuseColor"
        );

        let with_texture = ChainParams {
            textures: vec![texture("Color", "NewNana_D", 1)],
            colors: vec![("Color".to_string(), [0.9, 0.2, 0.1, 1.0])],
            ..Default::default()
        };
        let mut appearance = MaterialAppearance::default();
        apply_chain_selection(&mut appearance, &with_texture);
        assert_eq!(appearance.base_color, [0.9, 0.2, 0.1]);
    }
}
