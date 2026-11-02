//! UE 资产 → 引擎无关场景的统一 Provider（V-10/V-11）。
//!
//! 负责：读取包、识别 StaticMesh/SkeletalMesh、按 `LodPolicy` 选择 LOD、
//! 解析材质（带可选贴图缓存与 warning）、构建 `Scene` 并返回加载报告。
//! UI / 批量导出 / 未来 Unity Provider 都通过这一入口获取场景。

use lumark_scene::Scene;

use crate::materials::{MaterialResolveOptions, TextureCache};
use crate::scene::{skeletal_mesh_scene, static_mesh_scene};
use crate::{
    AssetKind, GameAssetSource, Mappings, SkeletalMeshPreview, parse_package,
    resolve_material_slots,
};

/// LOD 选择策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LodPolicy {
    /// 使用第一个 LOD。
    #[default]
    First,
    /// 使用指定 LOD（越界时钳制到最后一个）。
    Index(usize),
}

/// 场景加载请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRequest {
    /// 资产路径（容器内）。
    pub asset_path: String,
    /// LOD 策略。
    pub lod: LodPolicy,
}

impl SceneRequest {
    /// 以默认 LOD 策略创建请求。
    pub fn new(asset_path: impl Into<String>) -> Self {
        Self {
            asset_path: asset_path.into(),
            lod: LodPolicy::First,
        }
    }

    /// 指定 LOD。
    pub fn with_lod(mut self, lod: LodPolicy) -> Self {
        self.lod = lod;
        self
    }
}

/// 场景加载选项。
#[derive(Debug, Clone)]
pub struct SceneLoadOptions {
    /// 是否解析并加载材质贴图。
    pub load_materials: bool,
    /// 贴图最大边长。
    pub max_texture_size: u32,
    /// 跨资产贴图缓存。
    pub texture_cache: Option<TextureCache>,
}

impl Default for SceneLoadOptions {
    fn default() -> Self {
        Self {
            load_materials: true,
            max_texture_size: 512,
            texture_cache: None,
        }
    }
}

/// 加载进度事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneLoadStage {
    /// 读取资产字节。
    Reading,
    /// 解析包结构。
    Parsing,
    /// 解析材质与贴图。
    Materials,
    /// 构建场景。
    Building,
    /// 完成。
    Finished {
        /// 顶点数。
        vertices: usize,
        /// 三角形数。
        triangles: usize,
    },
}

/// 加载报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneLoadReport {
    /// 资产路径。
    pub asset_path: String,
    /// 资产类型（`StaticMesh`/`SkeletalMesh`）。
    pub asset_class: String,
    /// 原始 LOD 数量。
    pub lod_count: usize,
    /// 实际使用的 LOD。
    pub lod_used: usize,
    /// 顶点数。
    pub vertices: usize,
    /// 三角形数。
    pub triangles: usize,
    /// 材质槽数量。
    pub materials: usize,
    /// 非致命警告（材质缺失/贴图缺失等）。
    pub warnings: Vec<String>,
}

/// 加载完成的场景。
#[derive(Debug)]
pub struct LoadedScene {
    /// 引擎无关场景。
    pub scene: Scene,
    /// 加载报告。
    pub report: SceneLoadReport,
}

/// 加载场景。
pub fn load_scene(
    source: &mut GameAssetSource,
    mappings: &Mappings,
    request: &SceneRequest,
    options: &SceneLoadOptions,
    progress: &mut dyn FnMut(SceneLoadStage),
) -> Result<LoadedScene, String> {
    progress(SceneLoadStage::Reading);
    let inputs = source
        .read_package(&request.asset_path)
        .map_err(|error| format!("读取资产失败：{error}"))?;
    progress(SceneLoadStage::Parsing);
    let package = parse_package(inputs.uasset, source.scripts(), None)
        .map_err(|error| format!("解析包失败：{error}"))?;
    let exports = &package.info().exports;
    let material_options = MaterialResolveOptions {
        load_textures: options.load_materials,
        max_texture_size: options.max_texture_size,
        texture_cache: options.texture_cache.clone(),
        ..MaterialResolveOptions::default()
    };

    if let Some(index) = exports.iter().position(|export| {
        export.kind == AssetKind::StaticMesh && export.class_name == "StaticMesh"
    }) {
        let mut mesh = package
            .read_static_mesh(index, mappings)
            .map_err(|error| format!("读取 StaticMesh 失败：{error}"))?;
        let lod_count = mesh.lods.len().max(1);
        let lod_used = select_lod(lod_count, request.lod);
        if let Some(lod) = mesh.lods.get(lod_used).cloned() {
            mesh.lod = lod;
        }
        let mut warnings = Vec::new();
        progress(SceneLoadStage::Materials);
        let resolved = resolve_materials(
            source,
            &request.asset_path,
            &mesh.material_slots,
            mappings,
            &material_options,
            &mut warnings,
        );
        progress(SceneLoadStage::Building);
        let scene = static_mesh_scene(&mesh.name, &mesh, &resolved);
        let report = build_report(
            &request.asset_path,
            "StaticMesh",
            lod_count,
            lod_used,
            scene.meshes.iter().map(|mesh| mesh.positions.len()).sum(),
            scene.meshes.iter().map(|mesh| mesh.triangle_count()).sum(),
            mesh.material_slots.len(),
            warnings,
        );
        progress(SceneLoadStage::Finished {
            vertices: report.vertices,
            triangles: report.triangles,
        });
        return Ok(LoadedScene { scene, report });
    }

    if let Some(index) = exports.iter().position(|export| {
        export.kind == AssetKind::SkeletalMesh && export.class_name == "SkeletalMesh"
    }) {
        let mut preview: SkeletalMeshPreview = package
            .read_skeletal_mesh_preview(index, mappings, inputs.ubulk.as_deref())
            .map_err(|error| format!("读取 SkeletalMesh 失败：{error}"))?;
        let lod_count = preview.lods.len().max(1);
        let lod_used = select_lod(lod_count, request.lod);
        if let Some(lod) = preview.lods.get(lod_used).cloned() {
            preview.geometry = Some(lod.clone());
            preview.lods = vec![lod];
        }
        let mut warnings = Vec::new();
        progress(SceneLoadStage::Materials);
        let slots: Vec<crate::MaterialSlot> = preview
            .material_names
            .iter()
            .map(|name| crate::MaterialSlot {
                name: name.clone(),
                package: None,
            })
            .collect();
        let resolved = resolve_materials(
            source,
            &request.asset_path,
            &slots,
            mappings,
            &material_options,
            &mut warnings,
        );
        progress(SceneLoadStage::Building);
        let scene = skeletal_mesh_scene(&preview.skeleton.name, &preview, &resolved);
        let report = build_report(
            &request.asset_path,
            "SkeletalMesh",
            lod_count,
            lod_used,
            scene.meshes.iter().map(|mesh| mesh.positions.len()).sum(),
            scene.meshes.iter().map(|mesh| mesh.triangle_count()).sum(),
            slots.len(),
            warnings,
        );
        progress(SceneLoadStage::Finished {
            vertices: report.vertices,
            triangles: report.triangles,
        });
        return Ok(LoadedScene { scene, report });
    }

    Err("资产中没有 StaticMesh/SkeletalMesh 导出".to_string())
}

/// LOD 策略 → 可用索引。
fn select_lod(lod_count: usize, policy: LodPolicy) -> usize {
    let last = lod_count.saturating_sub(1);
    match policy {
        LodPolicy::First => 0,
        LodPolicy::Index(index) => index.min(last),
    }
}

/// 解析材质并收集 warning。
fn resolve_materials(
    source: &mut GameAssetSource,
    asset_path: &str,
    slots: &[crate::MaterialSlot],
    mappings: &Mappings,
    options: &MaterialResolveOptions,
    warnings: &mut Vec<String>,
) -> Vec<crate::ResolvedMaterial> {
    let resolved = resolve_material_slots(source, asset_path, slots, Some(mappings), options);
    for (index, material) in resolved.iter().enumerate() {
        if material.appearance.is_none() {
            warnings.push(format!(
                "材质槽 {index}（{}）未解析到材质",
                material.slot.name
            ));
        }
        for warning in &material.warnings {
            warnings.push(warning.clone());
        }
        if let Some(viewer) = &material.viewer
            && viewer.base_color.is_none()
        {
            warnings.push(format!("材质 {} 缺少基础色贴图", material.slot.name));
        }
    }
    resolved
}

/// 组装报告。
#[allow(clippy::too_many_arguments)]
fn build_report(
    asset_path: &str,
    asset_class: &str,
    lod_count: usize,
    lod_used: usize,
    vertices: usize,
    triangles: usize,
    materials: usize,
    warnings: Vec<String>,
) -> SceneLoadReport {
    SceneLoadReport {
        asset_path: asset_path.to_string(),
        asset_class: asset_class.to_string(),
        lod_count,
        lod_used,
        vertices,
        triangles,
        materials,
        warnings,
    }
}

/// 选择 LOD 的纯函数（供单测）。
pub fn lod_index(lod_count: usize, policy: LodPolicy) -> usize {
    select_lod(lod_count, policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LOD 策略边界。
    #[test]
    fn selects_lod_by_policy() {
        assert_eq!(lod_index(3, LodPolicy::First), 0);
        assert_eq!(lod_index(3, LodPolicy::Index(2)), 2);
        assert_eq!(lod_index(3, LodPolicy::Index(99)), 2);
        assert_eq!(lod_index(0, LodPolicy::Index(1)), 0);
    }

    /// 请求默认 LOD 与链式设置。
    #[test]
    fn request_builder() {
        let request = SceneRequest::new("Game/SM_A.uasset").with_lod(LodPolicy::Index(1));
        assert_eq!(request.asset_path, "Game/SM_A.uasset");
        assert_eq!(request.lod, LodPolicy::Index(1));
    }
}
