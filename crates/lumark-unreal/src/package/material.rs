//! 材质外观近似：从旧式材质/材质实例中提取可用的 PBR 参数和贴图引用。
//!
//! cooked 材质不包含可直接还原的 shader，但旧式包仍保留材质表达式对象和
//! 导入表：这里从中提取矢量/标量参数默认值、父材质和贴图引用，供 CPU 渲染
//! 一个近似材质球。完整材质图求值不在本模块范围内。

use super::legacy::LegacyPackage;
use super::{Mappings, PropValue};

/// 贴图在材质中的用途分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialTextureKind {
    /// 基础色/反照率贴图。
    BaseColor,
    /// 法线贴图。
    Normal,
    /// 金属/粗糙度（ORM）贴图。
    MetallicRoughness,
    /// 自发光贴图。
    Emissive,
    /// 遮罩贴图。
    Mask,
    /// 其他贴图。
    Other,
}

/// 材质引用的一张贴图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialTextureRef {
    /// 对象名，例如 `T_Character_D`。
    pub name: String,
    /// 贴图包路径（`/Game/...`），能解析到时填入。
    pub package: Option<String>,
    /// 用途分类。
    pub kind: MaterialTextureKind,
}

/// 近似材质外观参数。
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialAppearance {
    /// 基础色（线性颜色）。
    pub base_color: [f32; 3],
    /// 自发光颜色（线性颜色）。
    pub emissive: [f32; 3],
    /// 金属度。
    pub metallic: f32,
    /// 粗糙度。
    pub roughness: f32,
    /// 引用的贴图。
    pub textures: Vec<MaterialTextureRef>,
    /// 参数是否来自父材质（材质实例未解析到自身参数时）。
    pub inherited: bool,
}

impl Default for MaterialAppearance {
    fn default() -> Self {
        Self {
            base_color: [0.72, 0.72, 0.72],
            emissive: [0.0; 3],
            metallic: 0.0,
            roughness: 0.55,
            textures: Vec::new(),
            inherited: false,
        }
    }
}

impl MaterialAppearance {
    /// 返回指定用途的第一张贴图引用。
    pub fn texture(&self, kind: MaterialTextureKind) -> Option<&MaterialTextureRef> {
        self.textures.iter().find(|texture| texture.kind == kind)
    }

    /// 用父材质补齐当前材质缺失的外观参数。
    pub fn merge_parent(&mut self, parent: &MaterialAppearance) {
        if self.base_color == MaterialAppearance::default().base_color {
            self.base_color = parent.base_color;
            self.inherited = true;
        }
        if self.emissive == [0.0; 3] {
            self.emissive = parent.emissive;
        }
        if self.metallic == 0.0 {
            self.metallic = parent.metallic;
        }
        if self.roughness == MaterialAppearance::default().roughness {
            self.roughness = parent.roughness;
        }
        for texture in &parent.textures {
            if !self
                .textures
                .iter()
                .any(|existing| existing.kind == texture.kind)
            {
                self.textures.push(texture.clone());
            }
        }
    }
}

/// 注意：FF7R 旧式材质实例（MIC）的属性流只有少量字段，标量/矢量/贴图
/// 参数数组在烘焙时没有作为属性序列化（被写入内联 shader map），因此这里
/// 只能读取到父材质与表达式默认值，实例覆盖值由调用方按父材质近似。
/// 从旧式材质/材质实例导出中读取近似外观。
///
/// 会遍历所有导出对象（材质主体和 `MaterialExpression*`），优先使用
/// 矢量/标量参数默认值，其次从导入表推断贴图用途。
pub fn read_legacy_material_appearance(
    package: &LegacyPackage,
    mappings: &Mappings,
) -> MaterialAppearance {
    let mut appearance = MaterialAppearance::default();
    let mut base_color: Option<[f32; 3]> = None;
    let mut emissive: Option<[f32; 3]> = None;
    let mut metallic: Option<f32> = None;
    let mut roughness: Option<f32> = None;
    for index in 0..package.exports().len() {
        let Ok(properties) = package.read_export_properties(index, mappings) else {
            continue;
        };
        let class = package
            .exports()
            .get(index)
            .map(|export| export.class_name.as_str())
            .unwrap_or_default();
        if class.ends_with("MaterialExpressionVectorParameter") {
            let Some(default) = struct_color(&properties.properties, "DefaultValue") else {
                continue;
            };
            let name = parameter_name(&properties.properties);
            if is_emissive_name(name.as_deref()) {
                emissive = Some(default);
            } else if is_base_color_name(name.as_deref()) || base_color.is_none() {
                base_color = Some(default);
            }
        } else if class.ends_with("MaterialExpressionScalarParameter") {
            let Some(value) = scalar_value(&properties.properties, "DefaultValue") else {
                continue;
            };
            let name = parameter_name(&properties.properties);
            if is_metallic_name(name.as_deref()) {
                metallic = Some(value);
            } else if is_roughness_name(name.as_deref()) {
                roughness = Some(value);
            }
        }
    }
    if let Some(color) = base_color {
        appearance.base_color = color;
    }
    if let Some(color) = emissive {
        appearance.emissive = color;
    }
    if let Some(value) = metallic {
        appearance.metallic = value;
    }
    if let Some(value) = roughness {
        appearance.roughness = value;
    }
    appearance.textures = texture_references(package);
    if appearance
        .texture(MaterialTextureKind::MetallicRoughness)
        .is_some()
        && roughness.is_none()
    {
        appearance.roughness = 0.35;
    }
    appearance
}

/// 从导入表推断贴图引用和父材质包路径。
pub fn texture_references(package: &LegacyPackage) -> Vec<MaterialTextureRef> {
    let mut references = Vec::new();
    for (_, class, object) in package.imports() {
        let Some(kind) = texture_class_kind(class, object) else {
            continue;
        };
        if references
            .iter()
            .any(|existing: &MaterialTextureRef| existing.name == *object)
        {
            continue;
        }
        let package_path = package
            .imports()
            .iter()
            .find(|(import_class, _, path)| {
                import_class == "/Script/CoreUObject"
                    && path.starts_with('/')
                    && path.rsplit('/').next() == Some(object.as_str())
            })
            .map(|(_, _, path)| path.clone());
        references.push(MaterialTextureRef {
            name: object.clone(),
            package: package_path,
            kind,
        });
    }
    references
}

/// 返回材质实例的父材质包路径（`/Game/...`）。
pub fn material_parent_package(package: &LegacyPackage) -> Option<String> {
    let parent_name = package
        .imports()
        .iter()
        .find(|(_, class, object)| {
            (class == "Material" || class == "MaterialInstanceConstant")
                && !object.starts_with("Default__")
                && !object.starts_with('/')
        })
        .map(|(_, _, object)| object.clone())?;
    package
        .imports()
        .iter()
        .find(|(class_package, _, path)| {
            class_package == "/Script/CoreUObject"
                && path.starts_with('/')
                && path.rsplit('/').next() == Some(parent_name.as_str())
        })
        .map(|(_, _, path)| path.clone())
}

/// 从 UE5 Zen 材质/材质实例属性中读取近似外观。
///
/// Zen 包的对象引用不暴露包路径，因此贴图只记录参数名和用途，由 UI 按
/// 同目录命名约定尝试定位贴图资产。
pub fn read_zen_material_appearance(
    package: &super::UnrealPackage,
    mappings: &Mappings,
) -> MaterialAppearance {
    let mut appearance = MaterialAppearance::default();
    let Some(index) = package.info().exports.iter().position(|export| {
        let class = export.class_name.to_ascii_lowercase();
        class.contains("material")
    }) else {
        return appearance;
    };
    let Ok(properties) = package.read_export_properties(index, mappings) else {
        return appearance;
    };
    let properties = properties.properties;
    let mut base_color: Option<[f32; 3]> = None;
    let mut emissive: Option<[f32; 3]> = None;
    for entry in parameter_array(&properties, "VectorParameterValues") {
        let Some(name) = parameter_entry_name(entry) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let Some(color) = parameter_entry_color(entry) else {
            continue;
        };
        if lower.contains("emiss") || lower.contains("glow") {
            emissive = Some(color);
        } else if is_base_color_name(Some(&name)) && !is_overlay_name(&lower) {
            base_color = Some(color);
        }
    }
    let mut roughness: Option<f32> = None;
    let mut metallic: Option<f32> = None;
    for entry in parameter_array(&properties, "ScalarParameterValues") {
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
        if is_metallic_name(Some(&name)) {
            metallic = Some(value);
        } else if is_roughness_name(Some(&name)) {
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
    for entry in parameter_array(&properties, "TextureParameterValues") {
        let Some(name) = parameter_entry_name(entry) else {
            continue;
        };
        let kind = classify_parameter_texture(&name);
        appearance.textures.push(MaterialTextureRef {
            name,
            package: None,
            kind,
        });
    }
    appearance
}

/// 读取属性数组中的结构项。
fn parameter_array<'a>(properties: &'a [(String, PropValue)], name: &str) -> &'a [PropValue] {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Array(values) => Some(values.as_slice()),
            _ => None,
        })
        .unwrap_or_default()
}

/// 读取参数项里的 ParameterInfo.Name。
fn parameter_entry_name(entry: &PropValue) -> Option<String> {
    let PropValue::Struct(fields) = entry else {
        return None;
    };
    let info = fields
        .iter()
        .find(|(key, _)| key == "ParameterInfo")
        .map(|(_, value)| value)?;
    let PropValue::Struct(info_fields) = info else {
        return None;
    };
    info_fields
        .iter()
        .find(|(key, _)| key == "Name")
        .and_then(|(_, value)| match value {
            PropValue::Name(name) | PropValue::String(name) => Some(name.clone()),
            _ => None,
        })
}

/// 读取参数项里的颜色值。
fn parameter_entry_color(entry: &PropValue) -> Option<[f32; 3]> {
    let PropValue::Struct(fields) = entry else {
        return None;
    };
    let value = fields
        .iter()
        .find(|(key, _)| key == "ParameterValue")
        .map(|(_, value)| value)?;
    struct_color(
        &[("DefaultValue".to_string(), value.clone())],
        "DefaultValue",
    )
}

/// 读取参数项里的标量值。
fn parameter_entry_scalar(entry: &PropValue) -> Option<f32> {
    let PropValue::Struct(fields) = entry else {
        return None;
    };
    fields
        .iter()
        .find(|(key, _)| key == "ParameterValue")
        .and_then(|(_, value)| match value {
            PropValue::Float(value) => Some(*value as f32),
            PropValue::Int(value) => Some(*value as f32),
            _ => None,
        })
}

/// 按参数名猜测贴图用途。
fn classify_parameter_texture(name: &str) -> MaterialTextureKind {
    let lower = name.to_ascii_lowercase();
    if lower.contains("albedo") || lower.contains("basecolor") || lower.contains("diffuse") {
        MaterialTextureKind::BaseColor
    } else if lower.contains("normal") {
        MaterialTextureKind::Normal
    } else if lower.contains("rough")
        || lower.contains("metal")
        || lower.contains("orm")
        || lower.contains("occlusion")
    {
        MaterialTextureKind::MetallicRoughness
    } else if lower.contains("emiss") {
        MaterialTextureKind::Emissive
    } else if lower.contains("mask") || lower.contains("opacity") {
        MaterialTextureKind::Mask
    } else {
        MaterialTextureKind::Other
    }
}

/// 血液/透明度等叠加参数不应作为基础外观。
fn is_overlay_name(lower: &str) -> bool {
    lower.contains("blood") || lower.contains("opacity") || lower.contains("specular")
}
/// 判断导入项是否是贴图并返回用途分类。
fn texture_class_kind(class: &str, object: &str) -> Option<MaterialTextureKind> {
    if !class.ends_with("Texture2D") && !class.ends_with("Texture") {
        return None;
    }
    if object.starts_with('/') || object.starts_with("Default__") {
        return None;
    }
    Some(classify_texture_name(object))
}

/// 按名称猜测贴图用途。
fn classify_texture_name(name: &str) -> MaterialTextureKind {
    let lower = name.to_ascii_lowercase();
    if lower.contains("normal") || lower.ends_with("_n") || lower.contains("_norm") {
        MaterialTextureKind::Normal
    } else if lower.contains("orm")
        || lower.contains("rough")
        || lower.contains("metal")
        || lower.ends_with("_r")
    {
        MaterialTextureKind::MetallicRoughness
    } else if lower.contains("emiss") || lower.contains("glow") {
        MaterialTextureKind::Emissive
    } else if lower.contains("mask") {
        MaterialTextureKind::Mask
    } else if lower.ends_with("_d")
        || lower.contains("basecolor")
        || lower.contains("albedo")
        || lower.contains("diffuse")
        || lower.contains("_bc")
    {
        MaterialTextureKind::BaseColor
    } else {
        MaterialTextureKind::Other
    }
}

/// 读取 struct 类型的颜色值（X/Y/Z 三个 float）。
fn struct_color(properties: &[(String, PropValue)], name: &str) -> Option<[f32; 3]> {
    let value = properties
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)?;
    let PropValue::Struct(fields) = value else {
        return None;
    };
    let mut color = [0.0f32; 3];
    for (index, component) in ["X", "Y", "Z"].iter().enumerate() {
        color[index] = fields
            .iter()
            .find(|(key, _)| key == component)
            .and_then(|(_, value)| match value {
                PropValue::Float(value) => Some(*value as f32),
                _ => None,
            })?;
    }
    Some(color)
}

/// 读取标量属性值。
fn scalar_value(properties: &[(String, PropValue)], name: &str) -> Option<f32> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Float(value) => Some(*value as f32),
            PropValue::Int(value) => Some(*value as f32),
            _ => None,
        })
}

/// 读取参数名（可能包在 SmartName struct 里）。
fn parameter_name(properties: &[(String, PropValue)]) -> Option<String> {
    let value = properties
        .iter()
        .find(|(key, _)| key == "ParameterName")
        .map(|(_, value)| value)?;
    match value {
        PropValue::Name(name) | PropValue::String(name) => Some(name.clone()),
        PropValue::Struct(fields) => fields.iter().find_map(|(key, value)| {
            if key == "Name" {
                match value {
                    PropValue::Name(name) | PropValue::String(name) => Some(name.clone()),
                    _ => None,
                }
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// 参数名是否表示自发光。
fn is_emissive_name(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        let lower = name.to_ascii_lowercase();
        lower.contains("emiss") || lower.contains("glow")
    })
}

/// 参数名是否表示基础色。
fn is_base_color_name(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        let lower = name.to_ascii_lowercase();
        lower.contains("color")
            || lower.contains("tint")
            || lower.contains("diffuse")
            || lower.contains("skin")
            || lower.contains("base")
    })
}

/// 参数名是否表示金属度。
fn is_metallic_name(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.to_ascii_lowercase().contains("metal"))
}

/// 参数名是否表示粗糙度。
fn is_roughness_name(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        let lower = name.to_ascii_lowercase();
        lower.contains("rough") || lower.contains("gloss")
    })
}

/// 材质查看器使用的一张 RGBA8 贴图（已限制尺寸）。
#[derive(Debug, Clone)]
pub struct MaterialViewerImage {
    /// RGBA8 像素。
    pub rgba: Vec<u8>,
    /// 像素宽度。
    pub width: u32,
    /// 像素高度。
    pub height: u32,
}

impl MaterialViewerImage {
    /// 从 RGBA8 数据创建查看器贴图；超过 `max_size` 时按最近邻缩放。
    pub fn new(rgba: Vec<u8>, width: u32, height: u32, max_size: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let max_size = max_size.max(16);
        if rgba.len() < (width as usize) * (height as usize) * 4 {
            return None;
        }
        let mut image = Self {
            rgba,
            width,
            height,
        };
        if image.width.max(image.height) > max_size {
            image = image.downsample(max_size)?;
        }
        Some(image)
    }

    /// 最近邻缩放，避免贴图在查看器中占用过多内存。
    fn downsample(&self, max_size: u32) -> Option<Self> {
        let scale = max_size as f32 / self.width.max(self.height) as f32;
        let width = ((self.width as f32 * scale).round()).max(1.0) as u32;
        let height = ((self.height as f32 * scale).round()).max(1.0) as u32;
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height {
            let source_y = (y as f32 * self.height as f32 / height as f32) as u32;
            for x in 0..width {
                let source_x = (x as f32 * self.width as f32 / width as f32) as u32;
                let source = ((source_y.min(self.height - 1) * self.width
                    + source_x.min(self.width - 1))
                    * 4) as usize;
                let target = ((y * width + x) * 4) as usize;
                rgba[target..target + 4].copy_from_slice(&self.rgba[source..source + 4]);
            }
        }
        Some(Self {
            rgba,
            width,
            height,
        })
    }
}

/// 交互式材质查看器所需的数据：近似外观参数 + 已抽样的贴图。
#[derive(Debug, Clone)]
pub struct MaterialViewerData {
    /// 近似 PBR 外观参数。
    pub appearance: MaterialAppearance,
    /// 基础色贴图。
    pub base_color: Option<MaterialViewerImage>,
    /// 法线贴图。
    pub normal: Option<MaterialViewerImage>,
    /// 金属/粗糙度贴图。
    pub metallic_roughness: Option<MaterialViewerImage>,
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 贴图名称能猜出基础色/法线/ORM 等用途。
    #[test]
    fn classifies_texture_names() {
        assert_eq!(
            classify_texture_name("T_Hero_BaseColor"),
            MaterialTextureKind::BaseColor
        );
        assert_eq!(
            classify_texture_name("T_Hero_D"),
            MaterialTextureKind::BaseColor
        );
        assert_eq!(
            classify_texture_name("T_Hero_Normals"),
            MaterialTextureKind::Normal
        );
        assert_eq!(
            classify_texture_name("T_Hero_ORM"),
            MaterialTextureKind::MetallicRoughness
        );
        assert_eq!(
            classify_texture_name("T_Hero_Emissive"),
            MaterialTextureKind::Emissive
        );
        assert_eq!(
            classify_texture_name("T_Hero_Mask"),
            MaterialTextureKind::Mask
        );
        assert_eq!(classify_texture_name("T_Sky"), MaterialTextureKind::Other);
    }

    /// 材质实例未解析到自身参数时用父材质补齐颜色和贴图。
    #[test]
    fn merges_parent_appearance() {
        let mut child = MaterialAppearance::default();
        let parent = MaterialAppearance {
            base_color: [0.45, 0.4, 0.36],
            metallic: 0.2,
            roughness: 0.4,
            textures: vec![MaterialTextureRef {
                name: "T_Hero_D".to_string(),
                package: Some("/Game/Textures/T_Hero_D".to_string()),
                kind: MaterialTextureKind::BaseColor,
            }],
            ..Default::default()
        };
        child.merge_parent(&parent);
        assert_eq!(child.base_color, parent.base_color);
        assert!(child.inherited);
        assert_eq!(child.metallic, 0.2);
        assert_eq!(
            child
                .texture(MaterialTextureKind::BaseColor)
                .map(|t| t.name.as_str()),
            Some("T_Hero_D")
        );
    }
}

/// 材质查看器贴图超过上限时按最近邻缩小。
#[test]
fn downsamples_material_viewer_image() {
    let rgba = vec![128u8; 32 * 32 * 4];
    let image = MaterialViewerImage::new(rgba, 32, 32, 16).expect("创建查看器贴图");
    assert_eq!((image.width, image.height), (16, 16));
    assert_eq!(image.rgba.len(), 16 * 16 * 4);
    assert!(MaterialViewerImage::new(vec![0; 3], 8, 8, 16).is_none());
}
