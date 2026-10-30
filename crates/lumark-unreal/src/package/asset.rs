//! 资产级元数据：动画时长、骨骼树概况和静态网格材质/插槽。
//!
//! 3D 几何解析仍在 M5 后续里程碑；本模块只从无版本属性中提取可在详情
//! 面板展示的字段，不依赖引擎原生渲染数据。

use super::{Mappings, PackageError, PropValue, UnrealPackage, invalid};

/// 资产元数据。
#[derive(Debug, Clone)]
pub enum AssetMetadata {
    /// 动画序列。
    Animation {
        /// 序列时长（秒）。
        length: f32,
        /// 播放速率缩放。
        rate_scale: f32,
        /// 骨架对象索引（包内负数表示导入）。
        skeleton: Option<i32>,
    },
    /// 骨骼资源。
    Skeleton {
        /// 骨骼数量。
        bone_count: usize,
        /// 根骨骼名称。
        root: Option<String>,
    },
    /// 静态网格（属性级信息）。
    StaticMesh {
        /// 材质槽数量。
        material_slots: usize,
        /// 插槽数量。
        sockets: usize,
    },
    /// 骨骼网格（属性级信息；几何/蒙皮仍待实现）。
    SkeletalMesh {
        /// 骨架对象索引。
        skeleton: Option<i32>,
        /// LOD 数量。
        lods: usize,
        /// 插槽数量。
        sockets: usize,
    },
    /// 其它资产。
    Other,
}

impl AssetMetadata {
    /// 返回详情面板展示行。
    pub fn rows(&self) -> Vec<(String, String)> {
        match self {
            Self::Animation {
                length,
                rate_scale,
                skeleton,
            } => vec![
                ("时长".to_string(), format!("{length:.3} s")),
                ("RateScale".to_string(), format!("{rate_scale:.3}")),
                (
                    "骨架".to_string(),
                    skeleton
                        .map(|index| format!("Object({index})"))
                        .unwrap_or_else(|| "无".to_string()),
                ),
            ],
            Self::Skeleton { bone_count, root } => vec![
                ("骨骼数".to_string(), bone_count.to_string()),
                (
                    "根骨骼".to_string(),
                    root.clone().unwrap_or_else(|| "未知".to_string()),
                ),
            ],
            Self::StaticMesh {
                material_slots,
                sockets,
            } => vec![
                ("材质槽".to_string(), material_slots.to_string()),
                ("插槽".to_string(), sockets.to_string()),
            ],
            Self::SkeletalMesh {
                skeleton,
                lods,
                sockets,
            } => vec![
                (
                    "骨架".to_string(),
                    skeleton
                        .map(|index| format!("Object({index})"))
                        .unwrap_or_else(|| "无".to_string()),
                ),
                ("LOD".to_string(), lods.to_string()),
                ("插槽".to_string(), sockets.to_string()),
            ],
            Self::Other => Vec::new(),
        }
    }
}

/// 读取资产元数据；未知/不支持的资产返回 [`AssetMetadata::Other`]。
pub fn read_asset_metadata(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<AssetMetadata, PackageError> {
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or_else(|| invalid("资产导出索引越界"))?;
    match export.class_name.as_str() {
        "AnimSequence" | "AnimMontage" => {
            let properties = package.read_export_properties(export_index, mappings)?;
            let length = number_property(&properties.properties, "SequenceLength")
                .map(|value| value as f32)
                .unwrap_or(0.0);
            let rate_scale = number_property(&properties.properties, "RateScale")
                .map(|value| value as f32)
                .unwrap_or(1.0);
            let skeleton = object_property(&properties.properties, "Skeleton");
            Ok(AssetMetadata::Animation {
                length,
                rate_scale,
                skeleton,
            })
        }
        "Skeleton" => {
            let skeleton = package.read_skeleton(export_index, mappings)?;
            Ok(AssetMetadata::Skeleton {
                bone_count: skeleton.bones.len(),
                root: skeleton.bones.first().map(|bone| bone.name.clone()),
            })
        }
        "StaticMesh" => {
            let properties = package.read_export_properties(export_index, mappings)?;
            Ok(AssetMetadata::StaticMesh {
                material_slots: array_property(&properties.properties, "StaticMaterials"),
                sockets: array_property(&properties.properties, "Sockets"),
            })
        }
        "SkeletalMesh" => {
            let properties = package.read_export_properties(export_index, mappings)?;
            Ok(AssetMetadata::SkeletalMesh {
                skeleton: object_property(&properties.properties, "Skeleton"),
                lods: array_property(&properties.properties, "LODInfo"),
                sockets: array_property(&properties.properties, "Sockets"),
            })
        }
        _ => Ok(AssetMetadata::Other),
    }
}

/// 按属性名查找数值。
fn number_property(properties: &[(String, PropValue)], name: &str) -> Option<f64> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Float(value) => Some(*value),
            PropValue::Int(value) => Some(*value as f64),
            PropValue::UInt(value) => Some(*value as f64),
            _ => None,
        })
}

/// 按属性名查找对象索引。
fn object_property(properties: &[(String, PropValue)], name: &str) -> Option<i32> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Object(index) => Some(*index),
            _ => None,
        })
}

/// 返回数组属性长度。
fn array_property(properties: &[(String, PropValue)], name: &str) -> usize {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| match value {
            PropValue::Array(values) => values.len(),
            _ => 0,
        })
        .unwrap_or(0)
}
