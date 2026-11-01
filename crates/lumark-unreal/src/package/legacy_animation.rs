//! FF7R / UE4 旧式 `AnimSequence` 的 ConstantKeyLerp 解压。
//!
//! 旧式包在 UObject 属性之后序列化 `FCompressedAnimSequence`：
//! `FStripDataFlags` + `bSerializeCompressedData` + 压缩格式/偏移表 +
//! 轨道映射 + 曲线数据 + 原始大小 + 压缩字节流。FF7R 的属性读取会少读
//! 若干字节，因此压缩头用结构特征定位，而不是依赖属性流的结束位置。

use super::animation::{AnimSequenceData, TrackPose};
use super::legacy::LegacyPackage;
use super::{Mappings, PackageError, PropValue};

/// 旧式 ConstantKeyLerp 压缩动画数据。
#[derive(Clone)]
pub(crate) struct LegacyKeyLerpData {
    /// 每轨道 4 个 int32：[平移偏移, 平移关键帧数, 旋转偏移, 旋转关键帧数]。
    track_offsets: Vec<i32>,
    /// 缩放偏移表，每轨道 [偏移, 关键帧数]；空表示没有缩放数据。
    scale_offsets: Vec<i32>,
    /// 压缩字节流；平移/旋转/缩放数据都指向这段流。
    stream: Vec<u8>,
    /// 平移压缩格式。
    translation_format: u8,
    /// 旋转压缩格式。
    rotation_format: u8,
    /// 缩放压缩格式。
    scale_format: u8,
}

/// 解码一个 UE4 旧式 `AnimSequence`。
pub fn read_legacy_anim_sequence(
    package: &LegacyPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<AnimSequenceData, PackageError> {
    let export = package
        .exports()
        .get(export_index)
        .ok_or_else(|| invalid("动画导出索引越界"))?;
    if export.class_name != "AnimSequence" {
        return Err(PackageError::Unsupported(format!(
            "{} 不是 AnimSequence",
            export.name
        )));
    }
    let properties = package.read_export_properties(export_index, mappings)?;
    let frames = property_i32(&properties.properties, "NumFrames")
        .unwrap_or(0)
        .max(0) as u32;
    let length = property_f32(&properties.properties, "SequenceLength").unwrap_or(0.0);
    let track_count = match properties
        .properties
        .iter()
        .find(|(name, _)| name == "TrackToSkeletonMapTable")
        .map(|(_, value)| value)
    {
        Some(PropValue::Array(items)) => items.len(),
        _ => 0,
    };
    if track_count == 0 {
        return Err(PackageError::Unsupported(
            "AnimSequence 没有可播放的轨道".to_string(),
        ));
    }
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("动画导出数据越界"))?;
    let layout = locate_native(data, properties.bytes_read, track_count)
        .ok_or_else(|| PackageError::Unsupported("无法定位旧式动画压缩数据".to_string()))?;
    let track_to_bone = layout.track_to_bone.clone();
    let legacy = LegacyKeyLerpData {
        track_offsets: layout.track_offsets,
        scale_offsets: layout.scale_offsets,
        stream: layout.stream,
        translation_format: layout.translation_format,
        rotation_format: layout.rotation_format,
        scale_format: layout.scale_format,
    };
    Ok(AnimSequenceData::from_legacy_key_lerp(
        export.name.clone(),
        length,
        frames,
        track_to_bone,
        legacy,
    ))
}

impl LegacyKeyLerpData {
    /// 返回轨道数量。
    fn track_count(&self) -> usize {
        self.track_offsets.len() / 4
    }

    /// 采样指定相对时间（0–1）的全部轨道。
    pub(crate) fn sample(&self, ratio: f32) -> Vec<TrackPose> {
        let ratio = ratio.clamp(0.0, 1.0);
        let track_count = self.track_count();
        let mut poses = Vec::with_capacity(track_count);
        for track in 0..track_count {
            let entry = track * 4;
            let trans_offset = self.track_offsets[entry];
            let trans_keys = self.track_offsets[entry + 1].max(0) as usize;
            let rot_offset = self.track_offsets[entry + 2];
            let rot_keys = self.track_offsets[entry + 3].max(0) as usize;
            let translation = if trans_offset >= 0 && trans_keys > 0 {
                // 单关键帧轨道总是按无损 float3 存储。
                let format = effective_format(self.translation_format, trans_keys, false);
                self.sample_channel(trans_offset as usize, trans_keys, ratio, format, false)
                    .map(|value| [value[0], value[1], value[2]])
                    .unwrap_or([0.0; 3])
            } else {
                [0.0; 3]
            };
            let rotation = if rot_offset >= 0 && rot_keys > 0 {
                self.sample_rotation(rot_offset as usize, rot_keys, ratio)
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            let scale = self
                .scale_for_track(track)
                .map(|(offset, keys)| {
                    if offset >= 0 && keys > 0 {
                        let format = effective_format(self.scale_format, keys, false);
                        self.sample_channel(offset as usize, keys, ratio, format, false)
                            .map(|value| [value[0], value[1], value[2]])
                            .unwrap_or([1.0; 3])
                    } else {
                        [1.0; 3]
                    }
                })
                .unwrap_or([1.0; 3]);
            poses.push(TrackPose {
                translation,
                rotation,
                scale,
            });
        }
        poses
    }

    /// 返回指定轨道的缩放偏移与关键帧数。
    fn scale_for_track(&self, track: usize) -> Option<(i32, usize)> {
        let offset = self.scale_offsets.get(track * 2).copied()?;
        let keys = self.scale_offsets.get(track * 2 + 1).copied()?;
        Some((offset, keys.max(0) as usize))
    }

    /// 采样平移/缩放通道（数值格式按压缩格式解码）。
    fn sample_channel(
        &self,
        offset: usize,
        keys: usize,
        ratio: f32,
        format: u8,
        rotation: bool,
    ) -> Option<[f32; 3]> {
        let (index0, index1, alpha) = time_to_index(keys, ratio);
        let (bounds, key_bytes) = channel_layout(format, keys, rotation)?;
        let top = self.stream.get(offset..)?;
        let first = offset + bounds + index0 * key_bytes;
        let second = offset + bounds + index1 * key_bytes;
        let a = decode_vector(&self.stream, first, format, top)?;
        let b = decode_vector(&self.stream, second, format, top)?;
        Some([
            a[0] + (b[0] - a[0]) * alpha,
            a[1] + (b[1] - a[1]) * alpha,
            a[2] + (b[2] - a[2]) * alpha,
        ])
    }

    /// 采样旋转通道。
    fn sample_rotation(&self, offset: usize, keys: usize, ratio: f32) -> [f32; 4] {
        let (index0, index1, alpha) = time_to_index(keys, ratio);
        let format = effective_format(self.rotation_format, keys, true);
        let (bounds, key_bytes) = match channel_layout(format, keys, true) {
            Some(layout) => layout,
            None => return [0.0, 0.0, 0.0, 1.0],
        };
        let a = decode_rotation(
            &self.stream,
            offset + bounds + index0 * key_bytes,
            format,
            &self.stream[offset..],
        )
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let b = decode_rotation(
            &self.stream,
            offset + bounds + index1 * key_bytes,
            format,
            &self.stream[offset..],
        )
        .unwrap_or(a);
        // UE4 使用 FastLerp 后归一化，而不是球面插值。
        let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        let sign = if dot < 0.0 { -1.0 } else { 1.0 };
        let mut result = [
            a[0] + (b[0] * sign - a[0]) * alpha,
            a[1] + (b[1] * sign - a[1]) * alpha,
            a[2] + (b[2] * sign - a[2]) * alpha,
            a[3] + (b[3] * sign - a[3]) * alpha,
        ];
        let length = result.iter().map(|value| value * value).sum::<f32>().sqrt();
        if length > 0.0 {
            for value in &mut result {
                *value /= length;
            }
        }
        result
    }
}

/// 定位到的旧式动画原生布局。
struct NativeLayout {
    track_offsets: Vec<i32>,
    scale_offsets: Vec<i32>,
    track_to_bone: Vec<i32>,
    stream: Vec<u8>,
    translation_format: u8,
    rotation_format: u8,
    scale_format: u8,
}

/// 从导出数据里定位并按 UE4.18 布局解析压缩动画数据。
fn locate_native(data: &[u8], start: usize, track_count: usize) -> Option<NativeLayout> {
    if track_count == 0 {
        return None;
    }
    let end = data.len().min(start + 512);
    for base in start..end {
        let Some(global_strip) = data.get(base) else {
            break;
        };
        // 只有 editor data 被剥离（cooked 包）时才没有 RawAnimationData。
        if global_strip & 1 == 0 {
            continue;
        }
        if read_i32(data, base + 2) == Some(0) {
            continue;
        }
        let key_format = *data.get(base + 6)?;
        let translation_format = *data.get(base + 7)?;
        let rotation_format = *data.get(base + 8)?;
        let scale_format = *data.get(base + 9)?;
        if key_format > 2 || translation_format > 6 || rotation_format > 6 || scale_format > 6 {
            continue;
        }
        // Constant/VariableKeyLerp 每轨道 4 个偏移，PerTrack 为 2 个。
        let expected_offsets = if key_format == 2 {
            track_count * 2
        } else {
            track_count * 4
        };
        if read_i32(data, base + 10) != Some(expected_offsets as i32) {
            continue;
        }
        let offsets_start = base + 14;
        let mut cursor = offsets_start + expected_offsets * 4;
        let scale_count = read_i32(data, cursor)?;
        if !(0..=65536).contains(&scale_count) {
            continue;
        }
        cursor += 4;
        let scale_start = cursor;
        cursor += scale_count as usize * 4;
        let strip_size = read_i32(data, cursor)?;
        if !(0..=16).contains(&strip_size) {
            continue;
        }
        cursor += 4;
        let map_count = read_i32(data, cursor)?;
        if !(1..=65536).contains(&map_count) || map_count as usize != track_count {
            continue;
        }
        cursor += 4;
        let map_start = cursor;
        let track_to_bone = (0..map_count)
            .map(|index| read_i32(data, map_start + index as usize * 4))
            .collect::<Option<Vec<_>>>()?;
        if track_to_bone
            .iter()
            .any(|bone| *bone < 0 || *bone > 1_000_000)
        {
            continue;
        }
        cursor += map_count as usize * 4;

        let track_offsets = (0..expected_offsets)
            .map(|index| read_i32(data, offsets_start + index * 4))
            .collect::<Option<Vec<_>>>()?;
        let scale_offsets = (0..scale_count as usize)
            .map(|index| read_i32(data, scale_start + index * 4))
            .collect::<Option<Vec<_>>>()?;
        if let Some(layout) = try_stream(
            data,
            cursor,
            track_count,
            track_offsets,
            scale_offsets,
            track_to_bone,
            translation_format,
            rotation_format,
            scale_format,
        ) {
            return Some(layout);
        }
    }
    None
}

/// 在候选曲线长度中寻找能容纳全部轨道并紧贴字节流末尾的布局。
#[allow(clippy::too_many_arguments)]
fn try_stream(
    data: &[u8],
    curve_start: usize,
    track_count: usize,
    track_offsets: Vec<i32>,
    scale_offsets: Vec<i32>,
    track_to_bone: Vec<i32>,
    translation_format: u8,
    rotation_format: u8,
    scale_format: u8,
) -> Option<NativeLayout> {
    let mut best: Option<(usize, usize, Vec<u8>)> = None;
    for curve_len in 0..=8192usize {
        let p = curve_start + curve_len;
        if p + 8 > data.len() {
            break;
        }
        let Some(raw_size) = read_i32(data, p) else {
            break;
        };
        let Some(stream_len) = read_i32(data, p + 4) else {
            break;
        };
        if raw_size <= 0 || stream_len <= 0 {
            continue;
        }
        let stream_start = p + 8;
        let Some(stream_end) = stream_start.checked_add(stream_len as usize) else {
            continue;
        };
        if stream_end > data.len() || data.len() - stream_end > 16 {
            continue;
        }
        let stream = &data[stream_start..stream_end];
        let mut max_end = 0usize;
        let mut valid = true;
        for track in 0..track_count {
            let entry = track * 4;
            let trans_offset = track_offsets[entry];
            let trans_keys = track_offsets[entry + 1];
            let rot_offset = track_offsets[entry + 2];
            let rot_keys = track_offsets[entry + 3];
            if trans_offset < 0 || rot_offset < 0 || trans_keys <= 0 || rot_keys <= 0 {
                valid = false;
                break;
            }
            let Some(trans_end) = channel_end(
                trans_offset as usize,
                trans_keys as usize,
                translation_format,
                false,
            ) else {
                valid = false;
                break;
            };
            let Some(rot_end) = channel_end(
                rot_offset as usize,
                rot_keys as usize,
                rotation_format,
                true,
            ) else {
                valid = false;
                break;
            };
            max_end = max_end.max(trans_end).max(rot_end);
            if max_end > stream.len() {
                valid = false;
                break;
            }
        }
        if valid && max_end == stream.len() {
            // 平移/旋转数值必须有限；旋转四元数三分量模长不应明显超过 1。
            if !stream_plausible(
                stream,
                &track_offsets,
                track_count,
                translation_format,
                rotation_format,
            ) {
                continue;
            }
            let better = best
                .as_ref()
                .is_none_or(|(_, best_len, _)| stream_len as usize > *best_len);
            if better {
                best = Some((raw_size as usize, stream_len as usize, stream.to_vec()));
            }
        }
    }
    let (_, _, stream) = best?;
    Some(NativeLayout {
        track_offsets,
        scale_offsets,
        track_to_bone,
        stream,
        translation_format,
        rotation_format,
        scale_format,
    })
}

/// 校验字节流里前几条轨道的旋转/平移数值是否合理。
fn stream_plausible(
    stream: &[u8],
    track_offsets: &[i32],
    track_count: usize,
    translation_format: u8,
    rotation_format: u8,
) -> bool {
    let mut checked = 0;
    for track in 0..track_count.min(8) {
        let entry = track * 4;
        let trans_offset = track_offsets[entry];
        let trans_keys = track_offsets[entry + 1];
        let rot_offset = track_offsets[entry + 2];
        let rot_keys = track_offsets[entry + 3];
        if trans_offset >= 0
            && trans_keys > 0
            && let Some((bounds, _)) =
                channel_layout(translation_format, trans_keys as usize, false)
            && let Some(top) = stream.get(trans_offset as usize..)
            && let Some(vector) = decode_vector(
                stream,
                trans_offset as usize + bounds,
                translation_format,
                top,
            )
            && vector
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 1_000_000.0)
        {
            return false;
        }
        if rot_offset >= 0
            && rot_keys > 0
            && let Some((bounds, _)) = channel_layout(rotation_format, rot_keys as usize, true)
            && let Some(top) = stream.get(rot_offset as usize..)
            && let Some(rotation) =
                decode_rotation(stream, rot_offset as usize + bounds, rotation_format, top)
        {
            let magnitude =
                rotation[0] * rotation[0] + rotation[1] * rotation[1] + rotation[2] * rotation[2];
            if !magnitude.is_finite() || magnitude > 1.05 {
                return false;
            }
        }
        checked += 1;
    }
    checked > 0
}

/// 计算通道在字节流中的结束位置（含 Interval 格式的 bounds）。
fn channel_end(offset: usize, keys: usize, format: u8, rotation: bool) -> Option<usize> {
    let (bounds, key_bytes) = channel_layout(format, keys, rotation)?;
    offset.checked_add(bounds + keys * key_bytes)
}

/// 返回通道的 bounds 字节数和每关键帧字节数。
///
/// 单关键帧总是无损存储：平移/缩放为 float3（12 字节），旋转为
/// `FQuatFloat96NoW`（12 字节）。`ACF_IntervalFixed32NoW` 在字节流起始处
/// 额外存放 6 个 float 的 mins/ranges。
fn channel_layout(format: u8, keys: usize, rotation: bool) -> Option<(usize, usize)> {
    if keys == 1 {
        return Some((0, 12));
    }
    // 平移/缩放：None、Float96NoW 为 float3；Fixed48NoW 为 3×u16；
    // IntervalFixed32NoW 为 4 字节；Identity 不存数据。
    let translation_strides = [12usize, 12, 6, 4, 0, 0, 0];
    // 旋转：None 为 float4；Float96NoW 3×float；Fixed48NoW 3×u16；
    // Interval/Fixed32/Float32 均为 4 字节；Identity 不存数据。
    let rotation_strides = [16usize, 12, 6, 4, 4, 4, 0];
    let table = if rotation {
        &rotation_strides
    } else {
        &translation_strides
    };
    let stride = *table.get(format as usize)?;
    if stride == 0 && format != 6 {
        return None;
    }
    let bounds = if format == 3 { 24 } else { 0 };
    Some((bounds, stride))
}

/// 单关键帧轨道总是按无损格式存储：平移/缩放为 float3，旋转为 Float96NoW。
fn effective_format(format: u8, keys: usize, rotation: bool) -> u8 {
    if keys == 1 {
        if rotation { 1 } else { 0 }
    } else {
        format
    }
}
/// TimeToIndex：返回两个关键帧索引和插值系数。
fn time_to_index(keys: usize, ratio: f32) -> (usize, usize, f32) {
    if keys < 2 {
        return (0, 0, 0.0);
    }
    let ratio = ratio.clamp(0.0, 1.0);
    if ratio <= 0.0 {
        return (0, 0, 0.0);
    }
    if ratio >= 1.0 {
        return (keys - 1, keys - 1, 0.0);
    }
    let position = ratio * (keys - 1) as f32;
    let index = position.floor() as usize;
    let next = (index + 1).min(keys - 1);
    (index, next, position - index as f32)
}

/// 解码一个平移/缩放关键帧；`top` 是通道起始（Interval 格式的 bounds 所在处）。
fn decode_vector(stream: &[u8], offset: usize, format: u8, top: &[u8]) -> Option<[f32; 3]> {
    match format {
        0 | 1 => Some([
            read_f32(stream, offset)?,
            read_f32(stream, offset + 4)?,
            read_f32(stream, offset + 8)?,
        ]),
        2 => {
            let raw = [
                read_u16(stream, offset)? as f32,
                read_u16(stream, offset + 2)? as f32,
                read_u16(stream, offset + 4)? as f32,
            ];
            let scale = 128.0 / 32767.0;
            Some([
                (raw[0] - 32767.0) * scale,
                (raw[1] - 32767.0) * scale,
                (raw[2] - 32767.0) * scale,
            ])
        }
        3 => {
            let packed = read_u32(stream, offset)?;
            let mins = [read_f32(top, 0)?, read_f32(top, 4)?, read_f32(top, 8)?];
            let ranges = [read_f32(top, 12)?, read_f32(top, 16)?, read_f32(top, 20)?];
            let unpacked_x = packed & 0x0000_03ff;
            let unpacked_y = (packed & 0x001f_fc00) >> 10;
            let unpacked_z = packed >> 21;
            Some([
                (unpacked_x as f32 - 511.0) / 511.0 * ranges[0] + mins[0],
                (unpacked_y as f32 - 1023.0) / 1023.0 * ranges[1] + mins[1],
                (unpacked_z as f32 - 1023.0) / 1023.0 * ranges[2] + mins[2],
            ])
        }
        6 => Some([1.0, 1.0, 1.0]),
        _ => None,
    }
}

/// 解码一个旋转关键帧；`top` 是通道起始（Interval 格式的 bounds 所在处）。
fn decode_rotation(stream: &[u8], offset: usize, format: u8, top: &[u8]) -> Option<[f32; 4]> {
    let mut quat = match format {
        0 => [
            read_f32(stream, offset)?,
            read_f32(stream, offset + 4)?,
            read_f32(stream, offset + 8)?,
            read_f32(stream, offset + 12)?,
        ],
        1 => {
            let x = read_f32(stream, offset)?;
            let y = read_f32(stream, offset + 4)?;
            let z = read_f32(stream, offset + 8)?;
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            [x, y, z, w]
        }
        2 => {
            let scale = 1.0 / 32767.0;
            let x = (read_u16(stream, offset)? as f32 - 32767.0) * scale;
            let y = (read_u16(stream, offset + 2)? as f32 - 32767.0) * scale;
            let z = (read_u16(stream, offset + 4)? as f32 - 32767.0) * scale;
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            [x, y, z, w]
        }
        3 => {
            let packed = read_u32(stream, offset)?;
            let mins = [read_f32(top, 0)?, read_f32(top, 4)?, read_f32(top, 8)?];
            let ranges = [read_f32(top, 12)?, read_f32(top, 16)?, read_f32(top, 20)?];
            let unpacked_x = packed >> 21;
            let unpacked_y = (packed & 0x001f_fc00) >> 10;
            let unpacked_z = packed & 0x0000_03ff;
            let x = (unpacked_x as f32 - 1023.0) / 1023.0 * ranges[0] + mins[0];
            let y = (unpacked_y as f32 - 1023.0) / 1023.0 * ranges[1] + mins[1];
            let z = (unpacked_z as f32 - 511.0) / 511.0 * ranges[2] + mins[2];
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            [x, y, z, w]
        }
        4 => {
            let packed = read_u32(stream, offset)?;
            let unpacked_x = packed >> 21;
            let unpacked_y = (packed & 0x001f_fc00) >> 10;
            let unpacked_z = packed & 0x0000_03ff;
            let x = (unpacked_x as f32 - 1023.0) / 1023.0;
            let y = (unpacked_y as f32 - 1023.0) / 1023.0;
            let z = (unpacked_z as f32 - 511.0) / 511.0;
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            [x, y, z, w]
        }
        6 => [0.0, 0.0, 0.0, 1.0],
        _ => return None,
    };
    let length = quat.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length > 0.0 {
        for value in &mut quat {
            *value /= length;
        }
    }
    Some(quat)
}

/// 读取属性中的整数。
fn property_i32(properties: &[(String, PropValue)], name: &str) -> Option<i32> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Int(value) => Some(*value as i32),
            _ => None,
        })
}

/// 读取属性中的浮点数。
fn property_f32(properties: &[(String, PropValue)], name: &str) -> Option<f32> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            PropValue::Float(value) => Some(*value as f32),
            _ => None,
        })
}

/// 读取 i32。
fn read_i32(data: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// 读取 u32。
fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// 读取 u16。
fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

/// 读取 f32。
fn read_f32(data: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// 构造错误。
fn invalid(message: impl Into<String>) -> PackageError {
    PackageError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造单轨道 ConstantKeyLerp 数据：平移 2 个关键帧、旋转 2 个关键帧。
    fn single_track_data(translation_format: u8, rotation_format: u8) -> LegacyKeyLerpData {
        let mut stream = Vec::new();
        // 平移：0 → (10, 0, 0)。
        for value in [0.0f32, 0.0, 0.0, 10.0, 0.0, 0.0] {
            stream.extend_from_slice(&value.to_le_bytes());
        }
        // 旋转：identity → 绕 Z 轴约 73°（Float96NoW）。
        stream.extend_from_slice(&[0.0f32, 0.0, 0.0].map(f32::to_le_bytes).concat());
        stream.extend_from_slice(&[0.0f32, 0.0, 0.6].map(f32::to_le_bytes).concat());
        LegacyKeyLerpData {
            track_offsets: vec![0, 2, 24, 2],
            scale_offsets: Vec::new(),
            stream,
            translation_format,
            rotation_format,
            scale_format: 1,
        }
    }

    /// 验证两个关键帧之间的线性插值与四元数插值。
    #[test]
    fn constant_key_lerp_samples_between_keys() {
        let data = single_track_data(0, 1);
        let start = data.sample(0.0);
        assert!((start[0].translation[0] - 0.0).abs() < 1e-5);
        assert!((start[0].rotation[3] - 1.0).abs() < 1e-5);
        let middle = data.sample(0.5);
        assert!(
            (middle[0].translation[0] - 5.0).abs() < 1e-4,
            "中间帧平移应线性插值：{:?}",
            middle[0].translation
        );
        assert!(middle[0].rotation[2] > 0.3);
        let end = data.sample(1.0);
        assert!((end[0].translation[0] - 10.0).abs() < 1e-4);
        assert!((end[0].rotation[2] - 0.6).abs() < 1e-4);
    }

    /// 验证单关键帧轨道总是按无损格式解码。
    #[test]
    fn single_key_track_uses_lossless_layout() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&[1.0f32, 2.0, 3.0].map(f32::to_le_bytes).concat());
        stream.extend_from_slice(&[0.0f32, 0.0, 0.0].map(f32::to_le_bytes).concat());
        let data = LegacyKeyLerpData {
            track_offsets: vec![0, 1, 12, 1],
            scale_offsets: Vec::new(),
            stream,
            translation_format: 3,
            rotation_format: 2,
            scale_format: 1,
        };
        let poses = data.sample(0.5);
        assert_eq!(poses[0].translation, [1.0, 2.0, 3.0]);
        assert!((poses[0].rotation[3] - 1.0).abs() < 1e-5);
    }

    /// 构造带完整压缩头的合成导出数据。
    fn synthetic_export() -> Vec<u8> {
        let mut data = vec![0u8; 32]; // 属性前缀
        data.extend_from_slice(&[0x01, 0x00]); // FStripDataFlags
        data.extend_from_slice(&1i32.to_le_bytes()); // bSerializeCompressedData
        data.extend_from_slice(&[0, 0, 1, 1]); // 格式：ConstantKeyLerp/None/Float96/Float96
        data.extend_from_slice(&4i32.to_le_bytes()); // 轨道偏移数量
        for value in [0i32, 2, 24, 2] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&0i32.to_le_bytes()); // 缩放偏移数量
        data.extend_from_slice(&2i32.to_le_bytes()); // StripSize
        data.extend_from_slice(&1i32.to_le_bytes()); // 轨道映射数量
        data.extend_from_slice(&0i32.to_le_bytes()); // 骨骼索引
        data.extend_from_slice(&[0u8; 40]); // 曲线数据
        data.extend_from_slice(&41508i32.to_le_bytes()); // 原始大小
        let mut stream = Vec::new();
        for value in [0.0f32, 0.0, 0.0, 10.0, 0.0, 0.0] {
            stream.extend_from_slice(&value.to_le_bytes());
        }
        stream.extend_from_slice(&[0.0f32, 0.0, 0.0].map(f32::to_le_bytes).concat());
        stream.extend_from_slice(&[0.0f32, 0.0, 0.6].map(f32::to_le_bytes).concat());
        data.extend_from_slice(&(stream.len() as i32).to_le_bytes());
        data.extend_from_slice(&stream);
        data.extend_from_slice(&0i32.to_le_bytes()); // bUseRawDataOnly
        data
    }

    /// 验证从导出数据中定位并解析压缩头。
    #[test]
    fn locates_synthetic_compressed_header() {
        let data = synthetic_export();
        let layout = locate_native(&data, 32, 1).expect("应定位到压缩头");
        assert_eq!(layout.stream.len(), 48);
        assert_eq!(layout.translation_format, 0);
        assert_eq!(layout.rotation_format, 1);
        assert_eq!(layout.track_to_bone, vec![0]);
        let legacy = LegacyKeyLerpData {
            track_offsets: layout.track_offsets,
            scale_offsets: layout.scale_offsets,
            stream: layout.stream,
            translation_format: layout.translation_format,
            rotation_format: layout.rotation_format,
            scale_format: layout.scale_format,
        };
        let middle = legacy.sample(0.5);
        assert!((middle[0].translation[0] - 5.0).abs() < 1e-4);
    }
}
