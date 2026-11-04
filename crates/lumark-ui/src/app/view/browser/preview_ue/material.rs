//! 旧式材质/材质实例的外观预览：提取近似 PBR 参数并渲染材质球。

use std::path::Path;

use lumark_unreal::{
    LegacyPackage, Mappings, MaterialAppearance, MaterialPreviewTexture, MaterialTextureKind,
    PropValue, ZenScriptObjects, is_legacy_package, material_parent_package, parse_package,
    read_legacy_material_appearance, read_legacy_texture, read_zen_material_appearance,
    render_material_ball,
};

use crate::i18n::tr;

use super::{format_prop_value_with, parse_legacy_asset, read_ue_asset_inputs};

/// 贴图解码结果。
struct PreviewTexture {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

/// 读取旧式材质的引用资源、可解析属性和外观近似参数。
pub(super) fn legacy_material_rows(
    asset: &[u8],
    uexp: &[u8],
    mappings_path: Option<&std::path::Path>,
    class_name: &str,
    name: &str,
) -> Vec<(String, String)> {
    let mut rows = vec![
        ("材质类".to_string(), class_name.to_string()),
        ("对象".to_string(), name.to_string()),
    ];
    let note = tr(
        "材质着色器/参数为引擎自定义布局，完整 shader 无法还原；外观预览是根据可解析参数与贴图渲染的近似材质球",
        "Material shaders use an engine-specific layout; the preview is an approximate PBR ball from parsed parameters/textures",
    )
    .to_string();
    let Some((package, mappings)) = parse_legacy_asset(asset, uexp, mappings_path) else {
        rows.push(("说明".to_string(), note));
        return rows;
    };
    // 材质依赖：父材质/材质实例/纹理引用。
    let mut parents: Vec<String> = Vec::new();
    let mut textures: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    for (class_package, import_class, object_name) in package.imports() {
        if import_class == "Package" {
            if object_name.starts_with("/Game/") {
                packages.push(object_name.clone());
            }
            continue;
        }
        if object_name.starts_with("Default__") || object_name.starts_with('/') {
            continue;
        }
        if import_class.contains("Material") {
            parents.push(format!("{object_name} ({import_class})"));
        } else if import_class.ends_with("Texture2D") {
            textures.push(object_name.clone());
        }
        let _ = class_package;
    }
    parents.sort();
    parents.dedup();
    textures.sort();
    textures.dedup();
    packages.sort();
    packages.dedup();
    if !parents.is_empty() {
        rows.push(("材质依赖".to_string(), parents.len().to_string()));
        rows.extend(
            parents
                .iter()
                .take(8)
                .map(|value| ("依赖".to_string(), value.clone())),
        );
    }
    if !textures.is_empty() {
        rows.push(("纹理引用".to_string(), textures.len().to_string()));
        rows.extend(
            textures
                .iter()
                .take(12)
                .map(|value| ("纹理".to_string(), value.clone())),
        );
    }
    if !packages.is_empty() {
        rows.push(("资源引用".to_string(), packages.len().to_string()));
        rows.extend(packages.iter().take(12).map(|value| {
            let short = value.rsplit('/').next().unwrap_or(value).to_string();
            ("资源".to_string(), short)
        }));
    }
    match package.read_export_properties(0, &mappings) {
        Ok(properties) if !properties.properties.is_empty() => {
            let resolve = |index: i32| package.resolve_object_name(index);
            // FF7R 旧式材质的映射槽位可能与实际类布局不一致，过滤掉明显
            // 无效的对象索引，避免展示 Object(101154816) 之类的垃圾字段。
            let fields: Vec<&(String, PropValue)> = properties
                .properties
                .iter()
                .filter(|(_, value)| plausible_material_value(&package, value))
                .collect();
            if let Some(parent) = fields
                .iter()
                .find_map(|(_, value)| material_parent_name(&package, value))
            {
                rows.push(("父材质".to_string(), parent));
            }
            if fields.is_empty() {
                rows.push((
                    "可解析字段".to_string(),
                    tr(
                        "材质参数布局与映射不一致，暂未能可靠解析属性值",
                        "The material parameter layout does not match the mapping; properties were not decoded",
                    )
                    .to_string(),
                ));
            } else {
                rows.push(("可解析字段".to_string(), format!("{} 项", fields.len())));
                rows.extend(
                    fields.iter().take(24).map(|(name, value)| {
                        (name.clone(), format_prop_value_with(value, &resolve))
                    }),
                );
            }
        }
        Ok(_) => rows.push((
            "可解析字段".to_string(),
            tr(
                "属性表为空或全部为默认值",
                "Properties are empty or all defaults",
            )
            .to_string(),
        )),
        Err(_) => rows.push((
            "可解析字段".to_string(),
            tr(
                "该材质布局与映射不一致，暂未能解析属性值",
                "This material layout does not match the mapping; properties could not be parsed",
            )
            .to_string(),
        )),
    }
    let _ = &package;
    rows.push(("说明".to_string(), note));
    rows
}

/// 判断属性值里的对象引用是否都能解析（避免展示错位后的垃圾值）。
fn plausible_material_value(package: &LegacyPackage, value: &PropValue) -> bool {
    match value {
        PropValue::Object(index) => *index == 0 || package.resolve_object_name(*index).is_some(),
        PropValue::Array(values) | PropValue::Set(values) => values
            .iter()
            .all(|value| plausible_material_value(package, value)),
        PropValue::Map(values) => values.iter().all(|(key, value)| {
            plausible_material_value(package, key) && plausible_material_value(package, value)
        }),
        PropValue::Struct(fields) => fields
            .iter()
            .all(|(_, value)| plausible_material_value(package, value)),
        _ => true,
    }
}

/// 从对象引用里找出父材质名称。
fn material_parent_name(package: &LegacyPackage, value: &PropValue) -> Option<String> {
    let PropValue::Object(index) = value else {
        return None;
    };
    if *index >= 0 {
        return None;
    }
    let import_index = (-*index - 1) as usize;
    let (_, class, object) = package.imports().get(import_index)?;
    (class == "Material" || class.contains("MaterialInstanceConstant")).then(|| object.clone())
}
/// 已渲染的材质球结果。
pub(super) struct RenderedMaterialPreview {
    /// 材质球图片路径。
    pub path: std::path::PathBuf,
    /// 实际使用的近似外观参数（已合并父材质）。
    pub appearance: MaterialAppearance,
    /// 已加载的贴图用途与名称。
    pub textures: Vec<(String, String)>,
    /// 交互式材质查看器数据。
    pub viewer: lumark_unreal::MaterialViewerData,
}

/// 构建交互式材质查看器数据（贴图最大 512 像素）。
fn viewer_data(
    appearance: &MaterialAppearance,
    base: Option<&PreviewTexture>,
    normal: Option<&PreviewTexture>,
    orm: Option<&PreviewTexture>,
) -> lumark_unreal::MaterialViewerData {
    let image = |texture: Option<&PreviewTexture>| {
        texture.and_then(|texture| {
            lumark_unreal::MaterialViewerImage::new(
                texture.rgba.clone(),
                texture.width,
                texture.height,
                512,
            )
        })
    };
    lumark_unreal::MaterialViewerData {
        appearance: appearance.clone(),
        base_color: image(base),
        normal: image(normal),
        metallic_roughness: image(orm),
        occlusion: None,
    }
}

/// 读取旧式材质并渲染近似 PBR 材质球到 `output`。
///
/// 需要从同一个容器里读取父材质和贴图；失败时返回 `None`，由调用方回退到
/// 纯信息预览。
#[allow(clippy::too_many_arguments)]
pub(super) fn load_legacy_material_preview(
    archive: &Path,
    global: &Path,
    relative: &str,
    asset: &[u8],
    uexp: &[u8],
    mappings_path: Option<&Path>,
    aes_key: Option<&str>,
    output: &Path,
) -> Option<RenderedMaterialPreview> {
    let (package, mappings) = parse_legacy_asset(asset, uexp, mappings_path)?;
    let mut appearance = read_legacy_material_appearance(&package, &mappings);
    // 材质实例的参数通常在父材质里。
    if let Some(parent_package) = material_parent_package(&package)
        && let Some(parent_appearance) = load_parent_appearance(
            archive,
            global,
            relative,
            &parent_package,
            aes_key,
            mappings_path,
        )
    {
        appearance.merge_parent(&parent_appearance);
    }
    let base_texture = appearance
        .texture(MaterialTextureKind::BaseColor)
        .and_then(|texture| {
            load_texture(archive, global, relative, texture, aes_key, mappings_path)
        });
    let normal_texture = appearance
        .texture(MaterialTextureKind::Normal)
        .and_then(|texture| {
            load_texture(archive, global, relative, texture, aes_key, mappings_path)
        });
    let orm_texture = appearance
        .texture(MaterialTextureKind::MetallicRoughness)
        .and_then(|texture| {
            load_texture(archive, global, relative, texture, aes_key, mappings_path)
        });
    let size = 512u32;
    let rgba = render_material_ball(
        &appearance,
        base_texture.as_ref().map(|texture| {
            MaterialPreviewTexture::new(&texture.rgba, texture.width, texture.height)
        }),
        normal_texture.as_ref().map(|texture| {
            MaterialPreviewTexture::new(&texture.rgba, texture.width, texture.height)
        }),
        orm_texture.as_ref().map(|texture| {
            MaterialPreviewTexture::new(&texture.rgba, texture.width, texture.height)
        }),
        None,
        lumark_unreal::MaterialBallSettings::default(),
        size,
    );
    let image = image::RgbaImage::from_raw(size, size, rgba)?;
    if let Some(parent) = output.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    image.save(output).ok()?;
    let mut textures = Vec::new();
    for (kind, label) in [
        (MaterialTextureKind::BaseColor, "基础色贴图"),
        (MaterialTextureKind::Normal, "法线贴图"),
        (MaterialTextureKind::MetallicRoughness, "金属/粗糙度贴图"),
    ] {
        if let Some(texture) = appearance.texture(kind) {
            let loaded = match kind {
                MaterialTextureKind::BaseColor => base_texture.is_some(),
                MaterialTextureKind::Normal => normal_texture.is_some(),
                MaterialTextureKind::MetallicRoughness => orm_texture.is_some(),
                _ => false,
            };
            if loaded {
                textures.push((label.to_string(), texture.name.clone()));
            }
        }
    }
    let viewer = viewer_data(
        &appearance,
        base_texture.as_ref(),
        normal_texture.as_ref(),
        orm_texture.as_ref(),
    );
    Some(RenderedMaterialPreview {
        path: output.to_path_buf(),
        appearance,
        textures,
        viewer,
    })
}

/// 颜色格式化成十六进制，便于复制对照。
pub(super) fn format_color(color: [f32; 3]) -> String {
    let encode = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        encode(color[0]),
        encode(color[1]),
        encode(color[2])
    )
}

/// 读取父材质的外观参数。
fn load_parent_appearance(
    archive: &Path,
    global: &Path,
    relative: &str,
    parent_package: &str,
    aes_key: Option<&str>,
    mappings_path: Option<&Path>,
) -> Option<MaterialAppearance> {
    let mappings = Mappings::parse_file(mappings_path?).ok()?;
    let relative = package_to_relative(relative, parent_package)?;
    let (data, _ubulk, uexp, _scripts) =
        read_ue_asset_inputs(archive, global, &relative, aes_key).ok()?;
    let uexp = uexp?;
    if !is_legacy_package(&data) {
        return None;
    }
    let package = LegacyPackage::parse(&data, &uexp).ok()?;
    Some(read_legacy_material_appearance(&package, &mappings))
}

/// 材质球贴图目标边长：4K 贴图只解码对应 mip，避免全尺寸临时 RGBA。
const PREVIEW_TEXTURE_LIMIT: u32 = 512;

/// 按材质球尺寸选择 mip 并解码一张贴图。
fn decode_preview_texture(texture: &lumark_unreal::Texture2D) -> Option<PreviewTexture> {
    let level = lumark_unreal::mip_level_for_limit(
        texture.width,
        texture.height,
        texture.mips.len(),
        PREVIEW_TEXTURE_LIMIT,
    );
    let (rgba, width, height) = texture.decode_mip_rgba_with_size(level).ok()?;
    Some(PreviewTexture {
        rgba,
        width,
        height,
    })
}

/// 读取材质引用的贴图并解码基础 mip。
fn load_texture(
    archive: &Path,
    global: &Path,
    relative: &str,
    texture: &lumark_unreal::MaterialTextureRef,
    aes_key: Option<&str>,
    mappings_path: Option<&Path>,
) -> Option<PreviewTexture> {
    let package = texture.package.as_deref()?;
    let texture_relative = package_to_relative(relative, package)?;
    let (data, ubulk, uexp, _scripts) =
        read_ue_asset_inputs(archive, global, &texture_relative, aes_key).ok()?;
    if is_legacy_package(&data) {
        let uexp = uexp?;
        let texture = read_legacy_texture(&data, &uexp, ubulk.as_deref()).ok()?;
        return decode_preview_texture(&texture);
    }
    let mappings = Mappings::parse_file(mappings_path?).ok()?;
    let package = parse_package(data.clone(), None, None).ok()?;
    let index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "Texture2D")?;
    let texture = package
        .read_texture2d(index, &mappings, ubulk.as_deref())
        .ok()?;
    decode_preview_texture(&texture)
}

/// 读取 Zen（UE5 IoStore）材质并渲染近似材质球。
///
/// Zen 包不暴露贴图包路径，按材质资产名与同目录 `Textures/` 命名约定
/// 尝试定位基础色/法线/ORM 贴图；找不到时退回参数颜色。
#[allow(clippy::too_many_arguments)]
pub(super) fn load_zen_material_preview(
    archive: &Path,
    global: &Path,
    relative: &str,
    package: &lumark_unreal::UnrealPackage,
    mappings_path: Option<&Path>,
    aes_key: Option<&str>,
    output: &Path,
) -> Option<RenderedMaterialPreview> {
    let mappings = Mappings::parse_file(mappings_path?).ok()?;
    let scripts = ZenScriptObjects::from_container(global, aes_key).ok();
    let mut appearance = read_zen_material_appearance(package, &mappings);
    let mut textures = Vec::new();
    let base_texture = load_guessed_texture(
        archive,
        global,
        relative,
        &appearance,
        MaterialTextureKind::BaseColor,
        aes_key,
        scripts.as_ref(),
        Some(&mappings),
    );
    let normal_texture = load_guessed_texture(
        archive,
        global,
        relative,
        &appearance,
        MaterialTextureKind::Normal,
        aes_key,
        scripts.as_ref(),
        Some(&mappings),
    );
    let orm_texture = load_guessed_texture(
        archive,
        global,
        relative,
        &appearance,
        MaterialTextureKind::MetallicRoughness,
        aes_key,
        scripts.as_ref(),
        Some(&mappings),
    );
    if let Some(texture) = &base_texture {
        appearance.base_color = [1.0, 1.0, 1.0];
        textures.push(("基础色贴图".to_string(), texture.name.clone()));
    }
    if let Some(texture) = &normal_texture {
        textures.push(("法线贴图".to_string(), texture.name.clone()));
    }
    if let Some(texture) = &orm_texture {
        textures.push(("金属/粗糙度贴图".to_string(), texture.name.clone()));
    }
    let size = 512u32;
    let rgba = render_material_ball(
        &appearance,
        base_texture.as_ref().map(|texture| {
            MaterialPreviewTexture::new(
                &texture.texture.rgba,
                texture.texture.width,
                texture.texture.height,
            )
        }),
        normal_texture.as_ref().map(|texture| {
            MaterialPreviewTexture::new(
                &texture.texture.rgba,
                texture.texture.width,
                texture.texture.height,
            )
        }),
        orm_texture.as_ref().map(|texture| {
            MaterialPreviewTexture::new(
                &texture.texture.rgba,
                texture.texture.width,
                texture.texture.height,
            )
        }),
        None,
        lumark_unreal::MaterialBallSettings::default(),
        size,
    );
    let image = image::RgbaImage::from_raw(size, size, rgba)?;
    if let Some(parent) = output.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    image.save(output).ok()?;
    let viewer = viewer_data(
        &appearance,
        base_texture.as_ref().map(|texture| &texture.texture),
        normal_texture.as_ref().map(|texture| &texture.texture),
        orm_texture.as_ref().map(|texture| &texture.texture),
    );
    Some(RenderedMaterialPreview {
        path: output.to_path_buf(),
        appearance,
        textures,
        viewer,
    })
}

/// 带名称的贴图。
struct GuessedTexture {
    name: String,
    texture: PreviewTexture,
}

/// 按同目录命名约定定位并解码一张贴图。
#[allow(clippy::too_many_arguments)]
fn load_guessed_texture(
    archive: &Path,
    global: &Path,
    relative: &str,
    appearance: &MaterialAppearance,
    kind: MaterialTextureKind,
    aes_key: Option<&str>,
    scripts: Option<&ZenScriptObjects>,
    mappings: Option<&Mappings>,
) -> Option<GuessedTexture> {
    for (name, path) in guess_texture_paths(relative, appearance, kind) {
        let Ok((data, ubulk, _uexp, _texture_scripts)) =
            read_ue_asset_inputs(archive, global, &path, aes_key)
        else {
            continue;
        };
        if is_legacy_package(&data) {
            continue;
        }
        let Ok(package) = parse_package(data, scripts, None) else {
            continue;
        };
        let Some(index) = package
            .info()
            .exports
            .iter()
            .position(|export| export.class_name == "Texture2D")
        else {
            continue;
        };
        let Some(mappings) = mappings else {
            continue;
        };
        let Some(texture) = package
            .read_texture2d(index, mappings, ubulk.as_deref())
            .ok()
        else {
            continue;
        };
        let Some(texture) = decode_preview_texture(&texture) else {
            continue;
        };
        return Some(GuessedTexture { name, texture });
    }
    None
}

/// 按材质资产名生成贴图候选路径。
fn guess_texture_paths(
    current_relative: &str,
    appearance: &MaterialAppearance,
    kind: MaterialTextureKind,
) -> Vec<(String, String)> {
    let Some(content_index) = current_relative.find("/Content/") else {
        return Vec::new();
    };
    let content_prefix = &current_relative[..content_index + "/Content/".len()];
    let after = &current_relative[content_index + "/Content/".len()..];
    let Some((directory, file)) = after.rsplit_once('/') else {
        return Vec::new();
    };
    let asset = file.trim_end_matches(".uasset");
    let stem = asset
        .trim_start_matches("MAT-")
        .trim_start_matches("MAT_")
        .trim_start_matches("MI-")
        .trim_start_matches("MI_")
        .trim_start_matches("M-")
        .trim_start_matches("M_");
    let short = stem.trim_end_matches(|value: char| value.is_ascii_uppercase());
    let short = if short.is_empty() { stem } else { short };
    let parent = directory
        .rsplit_once('/')
        .map(|(path, _)| path)
        .unwrap_or(directory);
    let texture_dirs = [
        format!("{parent}/Textures"),
        format!("{parent}/Texture"),
        parent.to_string(),
        directory.to_string(),
    ];
    // 材质里的贴图参数名也能提供命名线索。
    let parameter_hint = appearance
        .textures
        .iter()
        .find(|texture| texture.kind == kind)
        .map(|texture| texture.name.clone());
    let suffixes: &[&str] = match kind {
        MaterialTextureKind::BaseColor => &["BaseColor", "Albedo", "BC", "Diffuse", "Color", "D"],
        MaterialTextureKind::Normal => &["Nml", "Normal", "Normals", "Nrm", "N"],
        MaterialTextureKind::MetallicRoughness => &[
            "OcclusionRoughnessMetallic",
            "ORM",
            "Roughness",
            "MetallicRoughness",
            "R",
        ],
        _ => return Vec::new(),
    };
    let mut candidates = Vec::new();
    for suffix in suffixes {
        for name_stem in [stem, short] {
            for candidate_name in [
                format!("T_{name_stem}_{suffix}"),
                format!("T_{name_stem}{suffix}"),
                format!("{name_stem}_{suffix}"),
                format!("{name_stem}{suffix}"),
            ] {
                for directory in &texture_dirs {
                    let path = format!("{content_prefix}{directory}/{candidate_name}.uasset");
                    if !candidates.iter().any(|(_, existing)| existing == &path) {
                        candidates.push((candidate_name.clone(), path));
                    }
                }
            }
        }
    }
    if let Some(hint) = parameter_hint {
        for name_stem in [stem, short] {
            for directory in &texture_dirs {
                candidates.push((
                    hint.clone(),
                    format!("{content_prefix}{directory}/T_{name_stem}_{hint}.uasset"),
                ));
            }
        }
    }
    candidates
}
/// 把 `/Game/...` 包路径映射成当前容器的相对路径。
///
/// 当前资产路径形如 `Project/Content/Game/.../Asset.uasset`，据此推断
/// `/Game/` 在容器里的前缀。
fn package_to_relative(current_relative: &str, package: &str) -> Option<String> {
    let rest = package.strip_prefix("/Game/")?;
    let content = current_relative.find("/Content/")?;
    let prefix = &current_relative[..content + "/Content/".len()];
    Some(format!("{prefix}{rest}.uasset"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 当前容器路径能映射回 `/Game/` 包路径。
    #[test]
    fn maps_game_packages_to_container_relative_paths() {
        let current = "End/Content/GameContents/BodyDriver/Character/Materials/M_Body.uasset";
        let relative = package_to_relative(
            current,
            "/Game/GameContents/BodyDriver/Character/Textures/T_Body_D",
        );
        assert_eq!(
            relative.as_deref(),
            Some("End/Content/GameContents/BodyDriver/Character/Textures/T_Body_D.uasset")
        );
        assert!(package_to_relative(current, "/Engine/Functions/Foo").is_none());
    }

    /// 颜色十六进制格式。
    #[test]
    fn formats_colors_as_hex() {
        assert_eq!(format_color([1.0, 0.0, 0.5]), "#FF0080");
    }
}
