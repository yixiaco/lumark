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
        /// 压缩轨道数量。
        tracks: usize,
        /// 压缩动画字节数。
        compressed_bytes: usize,
        /// 骨骼压缩 codec 名称。
        codec: Option<String>,
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
        /// 材质槽名称。
        material_names: Vec<String>,
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
                tracks,
                compressed_bytes,
                codec,
            } => {
                let mut rows = vec![
                    ("时长".to_string(), format!("{length:.3} s")),
                    ("RateScale".to_string(), format!("{rate_scale:.3}")),
                    (
                        "骨架".to_string(),
                        skeleton
                            .map(|index| format!("Object({index})"))
                            .unwrap_or_else(|| "无".to_string()),
                    ),
                    ("轨道".to_string(), tracks.to_string()),
                    ("压缩数据".to_string(), format!("{compressed_bytes} bytes")),
                ];
                if let Some(codec) = codec {
                    rows.push(("编码".to_string(), codec.clone()));
                }
                rows
            }
            Self::Skeleton { bone_count, root } => vec![
                ("骨骼数".to_string(), bone_count.to_string()),
                (
                    "根骨骼".to_string(),
                    root.clone().unwrap_or_else(|| "未知".to_string()),
                ),
            ],
            Self::StaticMesh {
                material_slots,
                material_names,
                sockets,
            } => {
                let mut rows = vec![
                    ("材质槽".to_string(), material_slots.to_string()),
                    ("插槽".to_string(), sockets.to_string()),
                ];
                if !material_names.is_empty() {
                    rows.push(("材质".to_string(), material_names.join(", ")));
                }
                rows
            }
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
            let (tracks, compressed_bytes) = package
                .export_data(export_index)
                .and_then(|data| scan_animation_header(data, properties.bytes_read))
                .unwrap_or((0, 0));
            let codec = package
                .export_data(export_index)
                .and_then(|data| scan_animation_codec(data, properties.bytes_read));
            Ok(AssetMetadata::Animation {
                length,
                rate_scale,
                skeleton,
                tracks,
                compressed_bytes,
                codec,
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
                material_names: array_struct_field_names(
                    &properties.properties,
                    "StaticMaterials",
                    "MaterialSlotName",
                ),
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

/// 从数组结构体字段中提取名称，用于显示材质槽名。
fn array_struct_field_names(
    properties: &[(String, PropValue)],
    array_name: &str,
    field: &str,
) -> Vec<String> {
    let Some(PropValue::Array(values)) = properties
        .iter()
        .find(|(key, _)| key == array_name)
        .map(|(_, value)| value)
    else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| {
            let PropValue::Struct(fields) = value else {
                return None;
            };
            fields.iter().find_map(|(key, value)| {
                if key != field {
                    return None;
                }
                match value {
                    PropValue::Name(name) | PropValue::String(name) => Some(name.clone()),
                    _ => None,
                }
            })
        })
        .filter(|name| !name.is_empty() && name != "None")
        .collect()
}

/// 从 AnimSequence 原生数据中扫描压缩轨道头。
///
/// 不同引擎版本在属性流与压缩数据之间可能有少量对齐/未消费字段；这里
/// 通过 `CompressedRawDataSize → TrackToSkeletonMap → CurveNames → 字节数`
/// 的结构特征定位，返回（轨道数，压缩字节数）。
fn scan_animation_header(data: &[u8], start: usize) -> Option<(usize, usize)> {
    for offset in start..data.len().saturating_sub(16) {
        let raw_size = i32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?);
        if !(0..=100_000_000).contains(&raw_size) {
            continue;
        }
        let tracks = i32::from_le_bytes(data.get(offset + 4..offset + 8)?.try_into().ok()?);
        if !(1..=65535).contains(&tracks) {
            continue;
        }
        let tracks = tracks as usize;
        let map_start = offset + 8;
        let map_end = map_start.checked_add(tracks.checked_mul(4)?)?;
        if map_end + 9 > data.len() {
            continue;
        }
        let mut valid_map = true;
        for index in 0..tracks {
            let position = map_start + index * 4;
            let bone = i32::from_le_bytes(data.get(position..position + 4)?.try_into().ok()?);
            if !(0..=65535).contains(&bone) {
                valid_map = false;
                break;
            }
        }
        if !valid_map {
            continue;
        }
        let curves = i32::from_le_bytes(data.get(map_end..map_end + 4)?.try_into().ok()?);
        if !(0..=4096).contains(&curves) {
            continue;
        }
        // BNB/UE5 常见 case：没有压缩曲线；有曲线时退化为只报告轨道数。
        let byte_offset = if curves == 0 { map_end + 4 } else { continue };
        let compressed_bytes =
            i32::from_le_bytes(data.get(byte_offset..byte_offset + 4)?.try_into().ok()?);
        if !(0..=100_000_000).contains(&compressed_bytes) {
            continue;
        }
        let use_bulk = *data.get(byte_offset + 4)?;
        if use_bulk > 1 {
            continue;
        }
        return Some((tracks, compressed_bytes as usize));
    }
    None
}

/// 扫描压缩动画 codec 名称（FString，如 `AnimCompress_PerTrackCompression_6`）。
fn scan_animation_codec(data: &[u8], start: usize) -> Option<String> {
    const NEEDLE: &[u8] = b"AnimCompress_";
    let begin = start.max(4);
    for offset in begin..data.len().saturating_sub(NEEDLE.len()) {
        if data.get(offset..offset + NEEDLE.len()) != Some(NEEDLE) {
            continue;
        }
        let length = i32::from_le_bytes(data.get(offset - 4..offset)?.try_into().ok()?);
        if !(1..=256).contains(&length) {
            continue;
        }
        let end = offset + (length as usize - 1);
        let bytes = data.get(offset..end)?;
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 静态网格材质槽名称会从 StaticMaterials 数组结构中提取。
    #[test]
    fn extracts_static_material_names() {
        let properties = vec![(
            "StaticMaterials".to_string(),
            PropValue::Array(vec![
                PropValue::Struct(vec![(
                    "MaterialSlotName".to_string(),
                    PropValue::Name("MAT_A".to_string()),
                )]),
                PropValue::Struct(vec![(
                    "MaterialSlotName".to_string(),
                    PropValue::Name("MAT_B".to_string()),
                )]),
            ]),
        )];
        assert_eq!(
            array_struct_field_names(&properties, "StaticMaterials", "MaterialSlotName"),
            vec!["MAT_A".to_string(), "MAT_B".to_string()]
        );
    }

    /// 动画压缩头可通过结构特征定位。
    #[test]
    fn scans_animation_header() {
        let mut data = vec![0u8; 7];
        data.extend_from_slice(&100i32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&64i32.to_le_bytes());
        data.push(0);
        data.extend_from_slice(&[0u8; 8]);
        assert_eq!(scan_animation_header(&data, 0), Some((2, 64)));
    }
}
