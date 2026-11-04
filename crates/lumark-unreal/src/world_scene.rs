//! 世界 `.umap` 场景加载（V-14）：Actor → StaticMeshComponent → 跨包 StaticMesh。
//!
//! 参考 CUE4Parse `IoPackage.ResolveObjectIndex` 与 `FFilePackageStoreEntry`
//! 的包导入解析；本模块负责把 World 中可渲染的 StaticMeshActor 组装为
//! 引擎无关 `Scene`，供查看器/导出复用。

use std::collections::HashMap;

use lumark_scene::{NodeKind, Scene, SceneNode, Transform};

use crate::materials::MaterialResolveOptions;
use crate::scene::{UE_UNIT_SCALE, add_material, convert_mesh};
use crate::source::GameAssetSource;
use crate::{
    Mappings, MaterialSlot, PackageError, PackageExport, PackageInfo, PropValue, UnrealPackage,
    parse_package, resolve_material_slots,
};

/// 世界场景加载选项。
#[derive(Debug, Clone)]
pub struct WorldSceneOptions {
    /// 最多处理的 Actor 数量。
    pub max_actors: usize,
    /// 最多加载的 StaticMesh 数量。
    pub max_meshes: usize,
    /// 是否把无法解析的 Actor 写入 warnings。
    pub warn_skipped: bool,
    /// 网格材质解析选项；`None` 表示世界网格使用默认材质。
    pub material_options: Option<MaterialResolveOptions>,
}

impl Default for WorldSceneOptions {
    fn default() -> Self {
        Self {
            max_actors: 10_000,
            max_meshes: 10_000,
            warn_skipped: true,
            material_options: Some(MaterialResolveOptions::default()),
        }
    }
}

/// 世界场景加载报告。
#[derive(Debug, Clone, Default)]
pub struct WorldSceneReport {
    /// World Actor 总数。
    pub actor_count: usize,
    /// 写入场景的网格节点数。
    pub mesh_count: usize,
    /// 三角形总数。
    pub triangle_count: usize,
    /// 跳过/无法解析的 Actor 数。
    pub skipped: usize,
    /// 可读告警。
    pub warnings: Vec<String>,
}

/// 加载结果。
#[derive(Debug)]
pub struct WorldSceneLoad {
    /// 引擎无关场景。
    pub scene: Scene,
    /// 加载报告。
    pub report: WorldSceneReport,
}

/// 加载 `.umap` 世界场景。
pub fn load_world_scene(
    source: &mut GameAssetSource,
    world_path: &str,
    mappings: &Mappings,
    options: &WorldSceneOptions,
) -> Result<WorldSceneLoad, PackageError> {
    let mut inputs = source
        .read_package(world_path)
        .map_err(|error| PackageError::Unsupported(format!("读取 World 失败：{error}")))?;
    // 世界包通常没有 uexp；保留来源输入结构以兼容后续扩展。
    let world = parse_package(std::mem::take(&mut inputs.uasset), source.scripts(), None)?;
    let summary = world.read_world_summary(mappings)?;
    let world_id = source
        .package_id_for_path(world_path)
        .ok_or_else(|| PackageError::Unsupported("无法确定 World 包 ID".to_string()))?;
    let imported_ids = source
        .imported_packages(world_id)
        .map_err(|error| PackageError::Unsupported(format!("读取导入包表失败：{error}")))?;
    let mut report = WorldSceneReport {
        actor_count: summary.actor_count,
        ..WorldSceneReport::default()
    };
    let mut scene = Scene::new(summary.name.clone());
    scene.source = world_path.to_string();
    scene.up_axis = lumark_scene::Axis::Z;
    scene.unit_scale = UE_UNIT_SCALE;
    let mut package_cache: HashMap<u64, UnrealPackage> = HashMap::new();
    let mut mesh_count = 0usize;
    for actor in summary.actors.iter().take(options.max_actors) {
        if !is_static_mesh_actor(&actor.class_name) {
            report.skipped += 1;
            continue;
        }
        if mesh_count >= options.max_meshes {
            break;
        }
        match actor_instance(
            source,
            &world,
            actor.export_index,
            &imported_ids,
            mappings,
            &mut package_cache,
        ) {
            Ok(Some(instance)) => {
                let mesh_handle = scene.add_mesh(convert_mesh(&instance.mesh, &instance.name));
                // 世界网格的材质按网格包路径 + 槽名解析；解析失败时回退默认材质。
                let mut material_handles = Vec::new();
                if let Some(material_options) = &options.material_options {
                    let resolved = match &instance.mesh_path {
                        Some(mesh_path) => resolve_material_slots(
                            source,
                            mesh_path,
                            &instance.material_slots,
                            Some(mappings),
                            material_options,
                        ),
                        None => Vec::new(),
                    };
                    for material in &resolved {
                        if material.appearance.is_none() {
                            report.warnings.push(format!(
                                "世界网格 {} 的材质槽 {} 未解析到材质",
                                instance.name, material.slot.name
                            ));
                        }
                        for warning in &material.warnings {
                            report.warnings.push(warning.clone());
                        }
                        material_handles.push(add_material(&mut scene, material));
                    }
                }
                report.triangle_count += instance.mesh.indices.len() / 3;
                scene.add_node(SceneNode {
                    name: actor.name.clone(),
                    parent: None,
                    transform: instance.transform,
                    mesh: Some(mesh_handle),
                    materials: material_handles,
                    visible: true,
                    kind: NodeKind::StaticMesh,
                    metadata: vec![("class".to_string(), actor.class_name.clone())],
                });
                mesh_count += 1;
            }
            Ok(None) => {
                report.skipped += 1;
                if options.warn_skipped {
                    report.warnings.push(format!(
                        "Actor {} 没有可解析的 StaticMeshComponent/StaticMesh",
                        actor.name
                    ));
                }
            }
            Err(error) => {
                report.skipped += 1;
                if options.warn_skipped {
                    report
                        .warnings
                        .push(format!("Actor {} 加载失败：{error}", actor.name));
                }
            }
        }
    }
    report.mesh_count = mesh_count;
    Ok(WorldSceneLoad { scene, report })
}

/// 判断类名是否为 StaticMeshActor（含蓝图子类）。
fn is_static_mesh_actor(class_name: &str) -> bool {
    class_name == "StaticMeshActor" || class_name.ends_with("_C") && class_name.contains("Static")
}

/// 从 World Actor 解析出的静态网格实例。
struct WorldMeshInstance {
    /// 网格资产名。
    name: String,
    /// 首个 LOD 几何。
    mesh: crate::MeshLod,
    /// 材质槽（顺序与 section 对应）。
    material_slots: Vec<MaterialSlot>,
    /// 网格所在的容器内路径（用于定位材质包）。
    mesh_path: Option<String>,
    /// 世界变换。
    transform: Transform,
}

/// 读取 Actor 的组件、网格与变换。
fn actor_instance(
    source: &GameAssetSource,
    world: &UnrealPackage,
    actor_index: usize,
    imported_ids: &[u64],
    mappings: &Mappings,
    package_cache: &mut HashMap<u64, UnrealPackage>,
) -> Result<Option<WorldMeshInstance>, PackageError> {
    let actor_class = world
        .info()
        .exports
        .get(actor_index)
        .map(|export| export.class_name.clone())
        .unwrap_or_default();
    if actor_class != "StaticMeshActor" {
        return Ok(None);
    }
    let properties = world.read_export_properties(actor_index, mappings)?;
    let component_index = object_export_index(&properties.properties, "StaticMeshComponent")
        .ok_or_else(|| PackageError::Unsupported("Actor 没有 StaticMeshComponent".to_string()))?;
    let component = world.read_export_properties(component_index, mappings)?;
    let mesh_import = object_import_index(&component.properties, "StaticMesh")
        .ok_or_else(|| PackageError::Unsupported("组件没有 StaticMesh".to_string()))?;
    let transform = transform_from_properties(&component.properties);
    let (target_package, target_hash) =
        resolve_package_import(world.info(), imported_ids, mesh_import)?;
    if let std::collections::hash_map::Entry::Vacant(entry) = package_cache.entry(target_package) {
        let (uasset, _ubulk) = source.read_package_by_id(target_package).map_err(|error| {
            PackageError::Unsupported(format!("读取包 0x{target_package:016X} 失败：{error}"))
        })?;
        entry.insert(parse_package(uasset, source.scripts(), None)?);
    }
    let target = package_cache
        .get(&target_package)
        .ok_or_else(|| PackageError::Unsupported("导入包缓存缺失".to_string()))?;
    let export_index = find_export_by_public_hash(target.info().exports.as_slice(), target_hash)
        .ok_or_else(|| {
            PackageError::Unsupported(format!(
                "导入包 0x{target_package:016X} 中未找到 public hash 0x{target_hash:016X}"
            ))
        })?;
    let export = &target.info().exports[export_index];
    if export.class_name != "StaticMesh" {
        return Err(PackageError::Unsupported(format!(
            "目标导出 {} 不是 StaticMesh（{}）",
            export.name, export.class_name
        )));
    }
    let static_mesh = target.read_static_mesh(export_index, mappings)?;
    Ok(Some(WorldMeshInstance {
        name: static_mesh.name,
        mesh: static_mesh.lod,
        material_slots: static_mesh.material_slots,
        mesh_path: source.package_path_for_id(target_package),
        transform,
    }))
}

/// 解析一条包导入：返回 (目标包 ID, 目标公共导出哈希)。
fn resolve_package_import(
    info: &PackageInfo,
    imported_ids: &[u64],
    import_index: usize,
) -> Result<(u64, u64), PackageError> {
    crate::imports::resolve_package_import(info, imported_ids, import_index)
}

/// 按公共导出哈希查找导出。
fn find_export_by_public_hash(exports: &[PackageExport], hash: u64) -> Option<usize> {
    crate::imports::find_export_by_public_hash(exports, hash)
}

/// 属性中的对象 → 导出索引（正值为 package index，导出 = value - 1）。
fn object_export_index(properties: &[(String, PropValue)], name: &str) -> Option<usize> {
    match property(properties, name)? {
        PropValue::Object(index) if *index > 0 => Some((*index - 1) as usize),
        _ => None,
    }
}

/// 属性中的对象 → 导入索引（负值为 import = -value - 1）。
fn object_import_index(properties: &[(String, PropValue)], name: &str) -> Option<usize> {
    match property(properties, name)? {
        PropValue::Object(index) if *index < 0 => Some((-*index - 1) as usize),
        _ => None,
    }
}

/// 从组件属性读取 TRS 变换。
fn transform_from_properties(properties: &[(String, PropValue)]) -> Transform {
    let translation = struct_vector(properties, "RelativeLocation").unwrap_or([0.0; 3]);
    let scale = struct_vector(properties, "RelativeScale3D").unwrap_or([1.0; 3]);
    let rotation =
        struct_quaternion(properties, "RelativeRotation").unwrap_or([0.0, 0.0, 0.0, 1.0]);
    Transform {
        translation,
        rotation,
        scale,
    }
}

fn property<'a>(properties: &'a [(String, PropValue)], name: &str) -> Option<&'a PropValue> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn struct_fields<'a>(
    properties: &'a [(String, PropValue)],
    name: &str,
) -> Option<&'a [(String, PropValue)]> {
    match property(properties, name)? {
        PropValue::Struct(fields) => Some(fields.as_slice()),
        _ => None,
    }
}

fn struct_float(properties: &[(String, PropValue)], name: &str, field: &str) -> Option<f32> {
    let fields = struct_fields(properties, name)?;
    fields.iter().find_map(|(key, value)| {
        if key != field {
            return None;
        }
        match value {
            PropValue::Float(number) => Some(*number as f32),
            _ => None,
        }
    })
}

fn struct_vector(properties: &[(String, PropValue)], name: &str) -> Option<[f32; 3]> {
    Some([
        struct_float(properties, name, "X")?,
        struct_float(properties, name, "Y")?,
        struct_float(properties, name, "Z")?,
    ])
}

fn struct_quaternion(properties: &[(String, PropValue)], name: &str) -> Option<[f32; 4]> {
    let fields = struct_fields(properties, name)?;
    if fields.iter().any(|(key, _)| key == "W") {
        return Some([
            struct_float(properties, name, "X")?,
            struct_float(properties, name, "Y")?,
            struct_float(properties, name, "Z")?,
            struct_float(properties, name, "W")?,
        ]);
    }
    // UE FRotator：Pitch(X)/Yaw(Y)/Roll(Z)，单位为度；q = qz * qy * qx。
    let pitch = struct_float(properties, name, "X")?.to_radians() * 0.5;
    let yaw = struct_float(properties, name, "Y")?.to_radians() * 0.5;
    let roll = struct_float(properties, name, "Z")?.to_radians() * 0.5;
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    let (sr, cr) = roll.sin_cos();
    let qx = [sr, 0.0, 0.0, cr];
    let qy = [0.0, sp, 0.0, cp];
    let qz = [0.0, 0.0, sy, cy];
    Some(quat_mul(qz, quat_mul(qy, qx)))
}

/// 四元数乘法（xyzw）。
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(value: f64) -> PropValue {
        PropValue::Float(value)
    }

    #[test]
    fn reads_component_transform() {
        let properties = vec![
            (
                "RelativeLocation".to_string(),
                PropValue::Struct(vec![
                    ("X".to_string(), float(100.0)),
                    ("Y".to_string(), float(200.0)),
                    ("Z".to_string(), float(300.0)),
                ]),
            ),
            (
                "RelativeScale3D".to_string(),
                PropValue::Struct(vec![
                    ("X".to_string(), float(2.0)),
                    ("Y".to_string(), float(3.0)),
                    ("Z".to_string(), float(4.0)),
                ]),
            ),
        ];
        let transform = transform_from_properties(&properties);
        assert_eq!(transform.translation, [100.0, 200.0, 300.0]);
        assert_eq!(transform.scale, [2.0, 3.0, 4.0]);
        assert_eq!(transform.rotation, [0.0, 0.0, 0.0, 1.0]);
    }
}
