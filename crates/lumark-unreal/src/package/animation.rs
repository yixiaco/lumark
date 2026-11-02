//! UE5 `AnimSequence` 的 PerTrackCompression 解码，以及旧式 ConstantKeyLerp 后端。
//!
//! UE5 Zen 包走 `PerTrackCompression`：FFloat96NoW / FFixed48NoW /
//! FIntervalFixed32NoW 关键帧格式；FF7R 等 UE4 旧式包走
//! `AnimEncoding_ConstantKeyLerp` 的逐轨道偏移表，见 `legacy_animation`。

use super::legacy_animation::LegacyKeyLerpData;
use super::{Mappings, PackageError, UnrealPackage};

/// 一条骨骼动画轨道的采样姿态。
#[derive(Debug, Clone, Copy)]
pub struct TrackPose {
    /// 局部平移。
    pub translation: [f32; 3],
    /// 局部旋转（四元数 xyzw）。
    pub rotation: [f32; 4],
    /// 局部缩放。
    pub scale: [f32; 3],
}

/// 已解析的 AnimSequence 压缩数据。
#[derive(Clone)]
pub struct AnimSequenceData {
    /// 资产名。
    pub name: String,
    /// 时长（秒）。
    pub length: f32,
    /// 压缩帧数。
    pub frames: u32,
    /// track → 骨架骨骼索引。
    pub track_to_bone: Vec<i32>,
    /// 每条 track 的 translation/rotation 偏移对。
    offsets: Vec<i32>,
    /// 每条 track 的 scale 偏移。
    scale_offsets: Vec<i32>,
    /// 关键帧字节流。
    key_data: Vec<u8>,
    /// 旧式 ConstantKeyLerp 压缩数据；存在时优先使用该解码路径。
    legacy: Option<LegacyKeyLerpData>,
}

impl AnimSequenceData {
    /// 构造旧式（UE4 ConstantKeyLerp）动画数据。
    pub(crate) fn from_legacy_key_lerp(
        name: String,
        length: f32,
        frames: u32,
        track_to_bone: Vec<i32>,
        legacy: LegacyKeyLerpData,
    ) -> Self {
        Self {
            name,
            length,
            frames,
            track_to_bone,
            offsets: Vec::new(),
            scale_offsets: Vec::new(),
            key_data: Vec::new(),
            legacy: Some(legacy),
        }
    }
}

/// 解码一个动画在指定相对时间的骨骼轨迹。
pub fn read_anim_sequence(
    package: &UnrealPackage,
    export_index: usize,
    mappings: &Mappings,
) -> Result<AnimSequenceData, PackageError> {
    let export = package
        .info()
        .exports
        .get(export_index)
        .ok_or_else(|| invalid("动画导出索引越界"))?;
    if export.class_name != "AnimSequence" {
        return Err(PackageError::Unsupported(format!(
            "{} 不是 AnimSequence",
            export.name
        )));
    }
    let properties = package.read_export_properties(export_index, mappings)?;
    let data = package
        .export_data(export_index)
        .ok_or_else(|| invalid("动画导出数据越界"))?;
    let native = data
        .get(properties.bytes_read..)
        .ok_or_else(|| invalid("动画属性长度越界"))?;
    let header = parse_compressed_header(native)?;
    Ok(AnimSequenceData {
        name: export.name.clone(),
        length: property_f32(&properties.properties, "SequenceLength").unwrap_or(0.0),
        frames: header.frames,
        track_to_bone: header.track_to_bone,
        offsets: header.offsets,
        scale_offsets: header.scale_offsets,
        key_data: header.key_data,
        legacy: None,
    })
}

impl AnimSequenceData {
    /// 采样指定相对时间（0–1）的轨道姿态。
    pub fn sample(&self, ratio: f32) -> Vec<TrackPose> {
        if let Some(legacy) = &self.legacy {
            return legacy.sample(ratio);
        }
        let ratio = ratio.clamp(0.0, 1.0) as f64;
        let mut poses = Vec::with_capacity(self.track_to_bone.len());
        for track in 0..self.track_to_bone.len() {
            let offset = self.offsets.get(track * 2).copied().unwrap_or(-1);
            let rotation_offset = self.offsets.get(track * 2 + 1).copied().unwrap_or(-1);
            let scale_offset = self.scale_offsets.get(track).copied().unwrap_or(-1);
            let translation = if offset >= 0 {
                decode_track(
                    &self.key_data,
                    offset as usize,
                    Channel::Translation,
                    self.frames,
                    ratio,
                )
                .map(|value| [value[0] as f32, value[1] as f32, value[2] as f32])
                .unwrap_or([0.0; 3])
            } else {
                [0.0; 3]
            };
            let rotation = if rotation_offset >= 0 {
                decode_quat(&self.key_data, rotation_offset as usize, self.frames, ratio)
                    .unwrap_or([0.0, 0.0, 0.0, 1.0])
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            let scale = if scale_offset >= 0 {
                decode_track(
                    &self.key_data,
                    scale_offset as usize,
                    Channel::Scale,
                    self.frames,
                    ratio,
                )
                .map(|value| [value[0] as f32, value[1] as f32, value[2] as f32])
                .unwrap_or([1.0; 3])
            } else {
                [1.0; 3]
            };
            poses.push(TrackPose {
                translation,
                rotation,
                scale,
            });
        }
        poses
    }
}

#[cfg(test)]
impl AnimSequenceData {
    /// 构造无关键帧动画，供采样/蒙皮接口测试使用。
    pub(crate) fn empty_for_test(track_to_bone: Vec<i32>, length: f32, frames: u32) -> Self {
        Self {
            name: "A_Test".to_string(),
            length,
            frames,
            track_to_bone,
            offsets: Vec::new(),
            scale_offsets: Vec::new(),
            key_data: Vec::new(),
            legacy: None,
        }
    }
}

/// 解析后的压缩头。
struct CompressedHeader {
    frames: u32,
    track_to_bone: Vec<i32>,
    offsets: Vec<i32>,
    scale_offsets: Vec<i32>,
    key_data: Vec<u8>,
}

/// 解析 AnimSequence 原生压缩数据。
fn parse_compressed_header(data: &[u8]) -> Result<CompressedHeader, PackageError> {
    let offset = find_animation_offset(data).ok_or_else(|| invalid("无法定位动画压缩数据"))?;
    let tracks = read_i32(data, offset + 4).ok_or_else(|| invalid("动画轨道数越界"))?;
    if !(1..=65535).contains(&tracks) {
        return Err(invalid("动画轨道数非法"));
    }
    let tracks = tracks as usize;
    let track_start = offset + 8;
    let track_end = track_start + tracks * 4;
    let mut track_to_bone = Vec::with_capacity(tracks);
    for index in 0..tracks {
        track_to_bone.push(read_i32(data, track_start + index * 4).unwrap_or(0));
    }
    let curves = read_i32(data, track_end).ok_or_else(|| invalid("曲线数量越界"))?;
    if curves != 0 {
        return Err(PackageError::Unsupported(
            "暂不支持带压缩曲线的 AnimSequence".to_string(),
        ));
    }
    let stream_size_offset = track_end + 4;
    let stream_size =
        read_i32(data, stream_size_offset).ok_or_else(|| invalid("动画流大小越界"))?;
    if stream_size < 0 {
        return Err(invalid("动画流大小非法"));
    }
    let stream_start = stream_size_offset + 8; // i32 size + bool32
    let stream_end = stream_start
        .checked_add(stream_size as usize)
        .ok_or_else(|| invalid("动画流长度溢出"))?;
    let stream = data
        .get(stream_start..stream_end)
        .ok_or_else(|| invalid("动画流被截断"))?;
    let mut cursor = stream_end;
    let codec = read_fstring(data, &mut cursor)?;
    let _curve_path = read_fstring(data, &mut cursor)?;
    let curve_bytes = read_i32(data, cursor).ok_or_else(|| invalid("曲线字节数越界"))?;
    if curve_bytes < 0 {
        return Err(invalid("曲线字节数非法"));
    }
    cursor = cursor
        .checked_add(4 + curve_bytes as usize)
        .ok_or_else(|| invalid("曲线数据越界"))?;

    let mut parsed = None;
    for skip_stats in [false, true] {
        let mut position = cursor;
        let frames = read_i32(data, position).unwrap_or(-1);
        position += 4;
        if skip_stats {
            position += 16;
        }
        let formats = data.get(position..position + 4).unwrap_or_default();
        if formats.len() != 4 {
            continue;
        }
        position += 4;
        let key_size = read_i32(data, position).unwrap_or(-1);
        let offset_count = read_i32(data, position + 4).unwrap_or(-1);
        let scale_count = read_i32(data, position + 8).unwrap_or(-1);
        if frames <= 0
            || key_size < 0
            || offset_count != (tracks * 2) as i32
            || scale_count != tracks as i32
            || (key_size as usize + (tracks * 2 + tracks) * 4) != stream_size as usize
        {
            continue;
        }
        parsed = Some((frames as u32, key_size as usize));
        break;
    }
    let Some((frames, key_size)) = parsed else {
        return Err(PackageError::Unsupported(format!(
            "暂不支持动画 codec {codec}"
        )));
    };
    let offset_count = tracks * 2;
    let mut offsets = Vec::with_capacity(offset_count);
    for index in 0..offset_count {
        offsets.push(read_i32(stream, index * 4).unwrap_or(-1));
    }
    let mut scale_offsets = Vec::with_capacity(tracks);
    for index in 0..tracks {
        scale_offsets.push(read_i32(stream, 4 * (offset_count + index)).unwrap_or(-1));
    }
    let key_start = 4 * (offset_count + tracks);
    let key_data = stream
        .get(key_start..key_start + key_size)
        .ok_or_else(|| invalid("动画关键帧数据越界"))?
        .to_vec();
    Ok(CompressedHeader {
        frames,
        track_to_bone,
        offsets,
        scale_offsets,
        key_data,
    })
}

/// 定位 `CompressedRawDataSize/TrackToSkeletonMap` 起点。
fn find_animation_offset(data: &[u8]) -> Option<usize> {
    for offset in 0..data.len().saturating_sub(16) {
        let Some(raw) = read_i32(data, offset) else {
            continue;
        };
        let Some(tracks) = read_i32(data, offset + 4) else {
            continue;
        };
        if !(0..=100_000_000).contains(&raw) || !(1..=65535).contains(&tracks) {
            continue;
        }
        let tracks = tracks as usize;
        let Some(map_end) = offset.checked_add(8 + tracks * 4) else {
            continue;
        };
        let Some(curves) = read_i32(data, map_end) else {
            continue;
        };
        if curves == 0 {
            return Some(offset);
        }
    }
    None
}

/// 读取 i32。
fn read_i32(data: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
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

/// 读取 FString。
fn read_fstring(data: &[u8], cursor: &mut usize) -> Result<String, PackageError> {
    let length = read_i32(data, *cursor).ok_or_else(|| invalid("FString 长度越界"))?;
    if length <= 0 || length > 4096 {
        return Err(invalid("FString 长度非法"));
    }
    let start = *cursor + 4;
    let end = start
        .checked_add(length as usize - 1)
        .ok_or_else(|| invalid("FString 越界"))?;
    let bytes = data
        .get(start..end)
        .ok_or_else(|| invalid("FString 数据越界"))?;
    *cursor = start + length as usize;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// 动画通道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Translation,
    Scale,
}

/// 关键帧格式。
#[derive(Debug, Clone, Copy)]
struct KeyFormat {
    key_format: usize,
    format_flags: usize,
    num_keys: usize,
    bytes_per_key: usize,
    fixed_bytes: usize,
}

/// 解码单个关键帧头。
fn decode_key_format(key_data: &[u8], offset: usize) -> Option<KeyFormat> {
    let header = read_i32(key_data, offset)?;
    let num_keys = (header & 0x00FF_FFFF) as usize;
    let format_flags = ((header >> 24) & 0x0F) as usize;
    let key_format = ((header >> 28) & 0x0F) as usize;
    let component_count = component_count(key_format, format_flags)?;
    let (bytes_per_key, fixed_bytes) = match key_format {
        1 => (component_count * 4, 0),
        2 => (component_count * 2, 0),
        3 => (4, component_count * 4),
        4 => (4, 0),
        5 => (4, 0),
        _ => return None,
    };
    Some(KeyFormat {
        key_format,
        format_flags,
        num_keys,
        bytes_per_key,
        fixed_bytes,
    })
}

/// 每格式/flag 的组件数，取自 UE 的 `PerTrackNumComponentTable`。
fn component_count(key_format: usize, format_flags: usize) -> Option<usize> {
    const TABLE: [[usize; 8]; 7] = [
        [4; 8],
        [3, 1, 1, 2, 1, 2, 2, 3],
        [3, 1, 1, 2, 1, 2, 2, 3],
        [6, 2, 2, 4, 2, 4, 4, 6],
        [1; 8],
        [1; 8],
        [0; 8],
    ];
    TABLE.get(key_format)?.get(format_flags & 0x7).copied()
}

/// 采样向量轨道。
fn decode_track(
    key_data: &[u8],
    offset: usize,
    channel: Channel,
    frames: u32,
    ratio: f64,
) -> Option<[f64; 3]> {
    let format = decode_key_format(key_data, offset)?;
    let track_start = offset + 4;
    let (index0, index1, alpha) = time_to_index(key_data, &format, track_start, frames, ratio)?;
    let key0 = decode_vector_key(key_data, track_start, &format, index0, channel)?;
    if index0 == index1 {
        return Some(key0);
    }
    let key1 = decode_vector_key(key_data, track_start, &format, index1, channel)?;
    Some([
        lerp(key0[0], key1[0], alpha),
        lerp(key0[1], key1[1], alpha),
        lerp(key0[2], key1[2], alpha),
    ])
}

/// 采样旋转轨道。
fn decode_quat(key_data: &[u8], offset: usize, frames: u32, ratio: f64) -> Option<[f32; 4]> {
    let format = decode_key_format(key_data, offset)?;
    let track_start = offset + 4;
    let (index0, index1, alpha) = time_to_index(key_data, &format, track_start, frames, ratio)?;
    let q0 = decode_quat_key(key_data, track_start, &format, index0)?;
    if index0 == index1 {
        return Some(q0);
    }
    let q1 = decode_quat_key(key_data, track_start, &format, index1)?;
    Some(lerp_quat(q0, q1, alpha as f32))
}

/// 计算插值关键帧索引。
fn time_to_index(
    key_data: &[u8],
    format: &KeyFormat,
    track_start: usize,
    frames: u32,
    ratio: f64,
) -> Option<(usize, usize, f64)> {
    if format.num_keys < 2 {
        return Some((0, 0, 0.0));
    }
    if format.format_flags & 0x8 == 0 {
        let last = format.num_keys - 1;
        if ratio <= 0.0 {
            return Some((0, 0, 0.0));
        }
        if ratio >= 1.0 {
            return Some((last, last, 0.0));
        }
        let position = ratio * last as f64;
        let index0 = position.floor() as usize;
        let index1 = (index0 + 1).min(last);
        return Some((index0, index1, position - index0 as f64));
    }
    let frame_table = track_start
        .checked_add(format.fixed_bytes)?
        .checked_add(format.bytes_per_key.checked_mul(format.num_keys)?)?;
    let frame_table = (frame_table + 3) & !3;
    let total_frames = frames.saturating_sub(1) as usize;
    let last_key = format.num_keys - 1;
    if ratio <= 0.0 {
        return Some((0, 0, 0.0));
    }
    if ratio >= 1.0 {
        return Some((last_key, last_key, 0.0));
    }
    let last_frame = total_frames.saturating_sub(1);
    let frame_position = (ratio * total_frames as f64) as usize;
    let frame_position = frame_position.min(last_frame);
    let estimate = ((ratio * last_key as f64) as usize).min(last_key);
    let frame = |index: usize| -> Option<usize> {
        if frames > 0xFF {
            read_u16(key_data, frame_table + index * 2).map(usize::from)
        } else {
            key_data
                .get(frame_table + index)
                .map(|value| usize::from(*value))
        }
    };
    let mut low;
    if frame(estimate)? <= frame_position {
        low = last_key;
        for index in estimate + 1..=last_key {
            if frame(index)? > frame_position {
                low = index - 1;
                break;
            }
        }
    } else {
        low = 0;
        for index in (1..estimate).rev() {
            if frame(index)? <= frame_position {
                low = index;
                break;
            }
        }
    }
    let high = (low + 1).min(last_key);
    let low_frame = frame(low)?;
    let high_frame = frame(high)?;
    let delta = high_frame.saturating_sub(low_frame).max(1);
    Some((
        low,
        high,
        (frame_position as f64 - low_frame as f64) / delta as f64,
    ))
}

/// 解码一个向量关键帧。
fn decode_vector_key(
    key_data: &[u8],
    track_start: usize,
    format: &KeyFormat,
    index: usize,
    channel: Channel,
) -> Option<[f64; 3]> {
    let key = track_start
        .checked_add(format.fixed_bytes)?
        .checked_add(index.checked_mul(format.bytes_per_key)?)?;
    match format.key_format {
        1 => {
            let mut cursor = key;
            let mut values = [0.0f64; 3];
            for (axis, value) in values.iter_mut().enumerate() {
                if format.format_flags & (1 << axis) != 0 || format.format_flags & 7 == 0 {
                    *value = read_f32(key_data, cursor)? as f64;
                    cursor += 4;
                } else {
                    *value = 0.0;
                }
            }
            Some(values)
        }
        2 => {
            let mut cursor = key;
            let mut values = [0.0f64; 3];
            for (axis, value) in values.iter_mut().enumerate() {
                if format.format_flags & (1 << axis) != 0 {
                    let raw = read_u16(key_data, cursor)?;
                    *value = decode_fixed16(raw);
                    cursor += 2;
                } else {
                    *value = 0.0;
                }
            }
            Some(values)
        }
        3 => {
            let (mins, ranges, packed) = read_interval(key_data, track_start, format, key)?;
            Some(interval_vector(&mins, &ranges, packed))
        }
        _ => {
            let _ = channel;
            Some([0.0; 3])
        }
    }
}

/// 解码一个旋转关键帧。
fn decode_quat_key(
    key_data: &[u8],
    track_start: usize,
    format: &KeyFormat,
    index: usize,
) -> Option<[f32; 4]> {
    let key = track_start
        .checked_add(format.fixed_bytes)?
        .checked_add(index.checked_mul(format.bytes_per_key)?)?;
    match format.key_format {
        1 => {
            let x = read_f32(key_data, key)? as f64;
            let y = read_f32(key_data, key + 4)? as f64;
            let z = read_f32(key_data, key + 8)? as f64;
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            Some([x as f32, y as f32, z as f32, w as f32])
        }
        2 => {
            let mut cursor = key;
            let mut values = [32767u16; 3];
            for (axis, value) in values.iter_mut().enumerate() {
                if format.format_flags & (1 << axis) != 0 {
                    *value = read_u16(key_data, cursor)?;
                    cursor += 2;
                }
            }
            let x = decode_fixed16(values[0]);
            let y = decode_fixed16(values[1]);
            let z = decode_fixed16(values[2]);
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            Some([x as f32, y as f32, z as f32, w as f32])
        }
        3 => {
            let (mins, ranges, packed) = read_interval(key_data, track_start, format, key)?;
            let x = ((((packed >> 21) as i32 - 1023) as f64 / 1023.0) * ranges[0] + mins[0]) as f32;
            let y = (((((packed & 0x001f_fc00) >> 10) as i32 - 1023) as f64 / 1023.0) * ranges[1]
                + mins[1]) as f32;
            let z = ((((packed & 0x0000_03ff) as i32 - 511) as f64 / 511.0) * ranges[2] + mins[2])
                as f32;
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            Some([x, y, z, w])
        }
        _ => Some([0.0, 0.0, 0.0, 1.0]),
    }
}

/// 读取区间格式的 bounds 和 packed 值。
fn read_interval(
    key_data: &[u8],
    track_start: usize,
    format: &KeyFormat,
    key: usize,
) -> Option<([f64; 3], [f64; 3], u32)> {
    let mut mins = [0.0f64; 3];
    let mut ranges = [0.0f64; 3];
    let mut cursor = track_start;
    for axis in 0..3 {
        if format.format_flags & (1 << axis) != 0 {
            mins[axis] = read_f32(key_data, cursor)? as f64;
            ranges[axis] = read_f32(key_data, cursor + 4)? as f64;
            cursor += 8;
        }
    }
    let packed = read_i32(key_data, key)? as u32;
    Some((mins, ranges, packed))
}

/// 区间压缩向量解码。
fn interval_vector(mins: &[f64; 3], ranges: &[f64; 3], packed: u32) -> [f64; 3] {
    let z = ((packed >> 21) as i32 - 1023) as f64 / 1023.0 * ranges[2] + mins[2];
    let y = (((packed & 0x001f_fc00) >> 10) as i32 - 1023) as f64 / 1023.0 * ranges[1] + mins[1];
    let x = ((packed & 0x0000_03ff) as i32 - 511) as f64 / 511.0 * ranges[0] + mins[0];
    [x, y, z]
}

/// 16 位定点数解码（LogScale=0）。
fn decode_fixed16(value: u16) -> f64 {
    (i32::from(value) - 32767) as f64 / 32767.0
}

/// 线性插值。
fn lerp(a: f64, b: f64, alpha: f64) -> f64 {
    a + (b - a) * alpha
}

/// 四元数插值并归一化。
fn lerp_quat(a: [f32; 4], b: [f32; 4], alpha: f32) -> [f32; 4] {
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

/// 属性数值。
fn property_f32(properties: &[(String, super::PropValue)], name: &str) -> Option<f32> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            super::PropValue::Float(value) => Some(*value as f32),
            super::PropValue::Int(value) => Some(*value as f32),
            _ => None,
        })
}

/// 构造错误。
fn invalid(message: impl Into<String>) -> PackageError {
    PackageError::Invalid(message.into())
}
