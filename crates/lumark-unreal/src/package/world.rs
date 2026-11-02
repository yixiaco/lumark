//! `.umap` World/Level 摘要读取。
//!
//! cooked World 的 `PersistentLevel`、`ExtraReferencedObjects` 和
//! `StreamingLevels` 位于无版本属性之后的原生尾部；PersistentLevel 的
//! `Actors` 数组同样是原生数据。本模块只读取 Actor 清单和类别统计，供世界
//! 浏览器与后续场景预览使用，不展开 Actor 的组件/属性。

use std::collections::{HashMap, HashSet};

use super::{AssetKind, Mappings, PackageError, UnrealPackage, invalid};

/// World 中的一个 Actor。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldActor {
    /// 导出对象名。
    pub name: String,
    /// 类名（如 `StaticMeshActor`）。
    pub class_name: String,
    /// 在包导出表中的索引。
    pub export_index: usize,
}

/// `.umap` World 摘要。
#[derive(Debug, Clone)]
pub struct WorldSummary {
    /// World 对象名。
    pub name: String,
    /// World 导出索引。
    pub world_export: usize,
    /// PersistentLevel 的导出索引。
    pub persistent_level: Option<usize>,
    /// PersistentLevel 的导出名。
    pub persistent_level_name: Option<String>,
    /// Actor 数量。
    pub actor_count: usize,
    /// Actor 清单（按导出表顺序）。
    pub actors: Vec<WorldActor>,
    /// 按类名统计的 Actor 数量（数量降序）。
    pub categories: Vec<(String, usize)>,
    /// StreamingLevels 数量。
    pub streaming_level_count: usize,
    /// 非致命警告。
    pub warnings: Vec<String>,
}

/// 读取 `.umap` 的 World 摘要。
pub fn read_world_summary(
    package: &UnrealPackage,
    mappings: &Mappings,
) -> Result<WorldSummary, PackageError> {
    let exports = &package.info().exports;
    // 优先精确类名 World；`WorldPartition`/`InstanceData` 等类也可能被归为 Map，
    // 不能直接取首个 Map 导出。
    let world_export = exports
        .iter()
        .position(|export| export.class_name == "World")
        .or_else(|| {
            exports.iter().position(|export| {
                export.class_name.contains("World")
                    && !export.class_name.contains("Partition")
                    && !export.class_name.contains("Cell")
                    && !export.class_name.contains("Data")
            })
        })
        .or_else(|| {
            exports
                .iter()
                .position(|export| export.kind == AssetKind::Map && export.outer_export.is_none())
        })
        .ok_or_else(|| invalid("包内没有 World 导出"))?;
    let world = &exports[world_export];
    let data = package
        .export_data(world_export)
        .ok_or_else(|| invalid("World 导出数据越界"))?;
    let mut warnings = Vec::new();
    let mut position = 0usize;
    match package.read_export_properties(world_export, mappings) {
        Ok(properties) => position = properties.bytes_read,
        Err(error) => warnings.push(format!("World 属性解析失败：{error}")),
    }

    // PersistentLevel 与 StreamingLevels 位于属性流之后；World Partition 世界的
    // 原生尾部可能为空/不同，最终以导出表的 PersistentLevel 作为回退。
    let native = read_world_native(data, position, exports, world_export);
    let persistent_level = native
        .as_ref()
        .map(|native| native.persistent_level)
        .or_else(|| {
            exports.iter().position(|export| {
                export.class_name == "Level"
                    && export.name == "PersistentLevel"
                    && export.outer_export == Some(world_export)
            })
        })
        .or_else(|| {
            exports.iter().position(|export| {
                export.class_name == "Level" && export.outer_export == Some(world_export)
            })
        })
        .or_else(|| {
            exports
                .iter()
                .position(|export| export.class_name == "Level")
        });
    let streaming_level_count = native.map(|native| native.streaming_levels).unwrap_or(0);

    let mut actors = Vec::new();
    if let Some(level_index) = persistent_level {
        let level = exports.get(level_index);
        if level.is_some_and(|level| level.class_name == "Level") {
            actors = read_level_actors(package, level_index, mappings, &mut warnings);
            if actors.is_empty() {
                warnings.push("PersistentLevel 的 Actors 数组为空，改用导出表回退".to_string());
                actors = actors_from_outer(exports, level_index);
            }
        } else {
            warnings.push(format!(
                "PersistentLevel 索引 {level_index} 不是 Level 导出"
            ));
        }
    }
    if actors.is_empty() {
        // 最后回退：取包内所有 Actor 类导出。
        actors = exports
            .iter()
            .enumerate()
            .filter(|(_, export)| is_actor_candidate(&export.class_name))
            .map(|(index, export)| WorldActor {
                name: export.name.clone(),
                class_name: actor_class_label(&export.name, &export.class_name),
                export_index: index,
            })
            .collect();
        if !actors.is_empty() {
            warnings.push("使用包内 Actor 类导出的全局回退清单".to_string());
        }
    }
    let categories = categorize(&actors);
    Ok(WorldSummary {
        name: world.name.clone(),
        world_export,
        persistent_level,
        persistent_level_name: persistent_level
            .and_then(|index| exports.get(index))
            .map(|export| export.name.clone()),
        actor_count: actors.len(),
        actors,
        categories,
        streaming_level_count,
        warnings,
    })
}

/// World 原生尾部解析结果。
struct WorldNative {
    persistent_level: usize,
    streaming_levels: usize,
}

/// 解析 World 原生尾部：PersistentLevel + ExtraReferencedObjects + StreamingLevels。
fn read_world_native(
    data: &[u8],
    start: usize,
    exports: &[super::PackageExport],
    world_export: usize,
) -> Option<WorldNative> {
    // 属性流长度可能与原生尾部有少量对齐差异，在有限窗口内寻找合法索引。
    for offset in 0..=16 {
        let Some(position) = start.checked_add(offset) else {
            break;
        };
        let Some(raw) = read_i32(data, position) else {
            break;
        };
        // FPackageIndex：正数 = 导出索引 + 1；负数/0 不是导出引用。
        let Ok(level) = usize::try_from(raw) else {
            continue;
        };
        let Some(level) = level.checked_sub(1) else {
            continue;
        };
        let Some(export) = exports.get(level) else {
            continue;
        };
        // 必须是 World 的子 Level，避免把组件/其它对象的索引误认为 PersistentLevel。
        if export.class_name != "Level" || export.outer_export != Some(world_export) {
            continue;
        }
        let mut cursor = position + 4;
        let Some((_extra, next)) = read_index_array(data, cursor) else {
            continue;
        };
        cursor = next;
        let Some((streaming, _next)) = read_index_array(data, cursor) else {
            continue;
        };
        return Some(WorldNative {
            persistent_level: level,
            streaming_levels: streaming.len(),
        });
    }
    None
}

/// 读取 PersistentLevel 的 Actors 数组。
fn read_level_actors(
    package: &UnrealPackage,
    level_index: usize,
    mappings: &Mappings,
    warnings: &mut Vec<String>,
) -> Vec<WorldActor> {
    let exports = &package.info().exports;
    let Some(data) = package.export_data(level_index) else {
        warnings.push("PersistentLevel 导出数据越界".to_string());
        return Vec::new();
    };
    let mut position = 0usize;
    match package.read_export_properties(level_index, mappings) {
        Ok(properties) => position = properties.bytes_read,
        Err(error) => warnings.push(format!("Level 属性解析失败：{error}")),
    }
    // Actors 数组位于属性之后；在有限窗口内寻找合法索引数组。
    for offset in 0..=16 {
        let Some(candidate) = position.checked_add(offset) else {
            break;
        };
        let Some((indices, _next)) = read_index_array(data, candidate) else {
            continue;
        };
        if indices.is_empty() {
            // 偏移不可靠时 count=0 可能只是错位；继续尝试后续偏移。
            continue;
        }
        let raw_count = indices.len();
        let actors = indices_to_actors(&indices, exports);
        if actors.len() * 2 >= raw_count {
            return actors;
        }
    }
    // 属性流可能因映射缺项/SoftObjectPath 等无法定位，退化为全数据扫描。
    let actors = scan_actor_array(data, exports);
    if !actors.is_empty() {
        warnings.push("Actors 数组通过全数据扫描定位".to_string());
    }
    actors
}

/// 把 FPackageIndex 数组转换为 Actor 清单（按导出索引去重）。
fn indices_to_actors(indices: &[i32], exports: &[super::PackageExport]) -> Vec<WorldActor> {
    let mut actors = Vec::with_capacity(indices.len());
    let mut seen = HashSet::new();
    for index in indices {
        // FPackageIndex：正数 = 导出索引 + 1；导入项跳过。
        let Ok(index) = usize::try_from(*index) else {
            continue;
        };
        let Some(index) = index.checked_sub(1) else {
            continue;
        };
        let Some(export) = exports.get(index) else {
            continue;
        };
        if !is_actor_candidate(&export.class_name) || !seen.insert(index) {
            continue;
        }
        actors.push(WorldActor {
            name: export.name.clone(),
            class_name: actor_class_label(&export.name, &export.class_name),
            export_index: index,
        });
    }
    actors
}

/// 在整段导出数据中扫描最可能的 Actor 索引数组。
///
/// 用于属性流无法解析或布局偏移未知的 World Partition 子关卡。
fn scan_actor_array(data: &[u8], exports: &[super::PackageExport]) -> Vec<WorldActor> {
    let mut best: Option<Vec<WorldActor>> = None;
    let mut offset = 0usize;
    while offset + 8 <= data.len() {
        if let Some((indices, _next)) = read_index_array(data, offset) {
            let count = indices.len();
            if (1..=4096).contains(&count) {
                let actors = indices_to_actors(&indices, exports);
                if actors.len() * 100 >= count * 60
                    && best.as_ref().is_none_or(|best| actors.len() > best.len())
                {
                    best = Some(actors);
                }
            }
        }
        offset += 4;
    }
    best.unwrap_or_default()
}

/// 回退：Level 子对象中类名像 Actor 的导出。
fn actors_from_outer(exports: &[super::PackageExport], level_index: usize) -> Vec<WorldActor> {
    exports
        .iter()
        .enumerate()
        .filter(|(_, export)| export.outer_export == Some(level_index))
        .filter(|(_, export)| is_actor_candidate(&export.class_name))
        .map(|(index, export)| WorldActor {
            name: export.name.clone(),
            class_name: actor_class_label(&export.name, &export.class_name),
            export_index: index,
        })
        .collect()
}

/// 类名是否可能是 Actor（排除 Level/World/组件等导出）。
fn is_actor_candidate(class_name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        "Level",
        "World",
        "Package",
        "Function",
        "Class",
        "Blueprint",
        "LevelScriptActor",
        "WorldSettings",
        "Model",
        "Polys",
        "Brush",
        "BrushBuilder",
    ];
    if class_name.is_empty() || EXCLUDED.contains(&class_name) {
        return false;
    }
    // 资产类（非 Actor）名通常较短且没有 Actor 后缀；只排除确切的资产类名，
    // 避免误伤 StaticMeshActor / SkeletalMeshActor 等 Actor 类。
    const ASSET_CLASSES: &[&str] = &[
        "StaticMesh",
        "SkeletalMesh",
        "Material",
        "MaterialInstance",
        "MaterialInterface",
        "Texture2D",
        "Texture",
        "SoundWave",
        "ParticleSystem",
        "AnimSequence",
    ];
    if ASSET_CLASSES.contains(&class_name) {
        return false;
    }
    !class_name.ends_with("Component")
        && !class_name.ends_with("Data")
        && !class_name.ends_with("Setup")
        && !class_name.ends_with("Settings")
        && !class_name.starts_with("WorldPartition")
        && class_name != "BodySetup"
        && class_name != "BookMark"
}

/// 导入类在 Zen 包里可能无法解析为真实类名；用对象名的 UAID 前缀做可读标签。
fn actor_class_label(export_name: &str, class_name: &str) -> String {
    if (class_name == "PackageImport" || class_name == "ScriptImport")
        && let Some(stem) = export_name.split("_UAID_").next()
        && !stem.is_empty()
    {
        return stem.to_string();
    }
    class_name.to_string()
}

/// 按类名统计 Actor 数量。
fn categorize(actors: &[WorldActor]) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for actor in actors {
        *counts.entry(actor.class_name.as_str()).or_default() += 1;
    }
    let mut categories: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(class_name, count)| (class_name.to_string(), count))
        .collect();
    categories.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    categories
}

/// 读取 i32。
fn read_i32(data: &[u8], position: usize) -> Option<i32> {
    let bytes = data.get(position..position.checked_add(4)?)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

/// 读取 `TArray<i32>`（count + count×i32），返回元素和数组之后的偏移。
fn read_index_array(data: &[u8], position: usize) -> Option<(Vec<i32>, usize)> {
    let count = read_i32(data, position)?;
    if !(0..=1_000_000).contains(&count) {
        return None;
    }
    let count = count as usize;
    let start = position.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(4)?)?;
    if end > data.len() {
        return None;
    }
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        values.push(read_i32(data, start + index * 4)?);
    }
    Some((values, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原生索引数组读取与越界保护。
    #[test]
    fn reads_index_arrays() {
        let mut data = Vec::new();
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&5i32.to_le_bytes());
        data.extend_from_slice(&(-3i32).to_le_bytes());
        data.extend_from_slice(&99i32.to_le_bytes());
        let (values, next) = read_index_array(&data, 0).expect("读取数组");
        assert_eq!(values, vec![5, -3]);
        assert_eq!(next, 12);
        // 截断的数组返回 None。
        assert!(read_index_array(&data[..10], 0).is_none());
        // 负数量返回 None。
        assert!(read_index_array(&(-1i32).to_le_bytes(), 0).is_none());
    }

    /// Actor 类名过滤与类别统计。
    #[test]
    fn categorizes_actors() {
        let actors = vec![
            WorldActor {
                name: "A".to_string(),
                class_name: "StaticMeshActor".to_string(),
                export_index: 1,
            },
            WorldActor {
                name: "B".to_string(),
                class_name: "StaticMeshActor".to_string(),
                export_index: 2,
            },
            WorldActor {
                name: "C".to_string(),
                class_name: "PointLight".to_string(),
                export_index: 3,
            },
        ];
        let categories = categorize(&actors);
        assert_eq!(
            categories,
            vec![
                ("StaticMeshActor".to_string(), 2),
                ("PointLight".to_string(), 1)
            ]
        );
        assert!(is_actor_candidate("BP_Chair_C"));
        assert!(!is_actor_candidate("Level"));
        assert!(!is_actor_candidate("StaticMesh"));
        assert!(!is_actor_candidate("Material"));
    }

    /// FPackageIndex 按“导出索引 + 1”解析，并去除重复引用。
    #[test]
    fn resolves_and_deduplicates_actor_indices() {
        use crate::package::{AssetKind, PackageExport};
        let export = |name: &str, class_name: &str| PackageExport {
            name: name.to_string(),
            class_name: class_name.to_string(),
            outer_export: None,
            data_offset: 0,
            data_size: 0,
            kind: AssetKind::Other,
        };
        let exports = vec![
            export("Level", "Level"),
            export("Chair", "StaticMeshActor"),
            export("Lamp", "PointLight"),
        ];
        let actors = indices_to_actors(&[2, 2, 3, -1], &exports);
        assert_eq!(actors.len(), 2);
        assert_eq!(actors[0].export_index, 1);
        assert_eq!(actors[1].export_index, 2);
        assert_eq!(actors[0].class_name, "StaticMeshActor");
    }
}
