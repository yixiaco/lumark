//! 手工诊断：解析 FF7R 旧式 AnimSequence 的 ConstantKeyLerp 数据并采样。
//!
//! 用法：`legacy_anim_decode <asset.uasset> <asset.uexp> <mappings.usmap>`

use std::error::Error;
use std::path::Path;

use lumark_unreal::{LegacyPackage, Mappings, PropValue};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("missing .uasset")?;
    let uexp = args.next().ok_or("missing .uexp")?;
    let mapping = args.next().ok_or("missing .usmap")?;
    let asset_data = std::fs::read(asset)?;
    let uexp_data = std::fs::read(uexp)?;
    let mappings = Mappings::parse_file(Path::new(&mapping))?;
    let package = LegacyPackage::parse(&asset_data, &uexp_data)?;
    let properties = package.read_export_properties(0, &mappings)?;
    let track_count = match properties
        .properties
        .iter()
        .find(|(name, _)| name == "TrackToSkeletonMapTable")
        .map(|(_, value)| value.clone())
    {
        Some(PropValue::Array(items)) => items.len(),
        _ => 0,
    };
    let frames = match properties
        .properties
        .iter()
        .find(|(name, _)| name == "NumFrames")
        .map(|(_, value)| value.clone())
    {
        Some(PropValue::Int(value)) => value,
        _ => 0,
    };
    let sequence_length = properties
        .properties
        .iter()
        .find(|(name, _)| name == "SequenceLength")
        .and_then(|(_, value)| match value {
            PropValue::Float(value) => Some(*value as f32),
            _ => None,
        })
        .unwrap_or(0.0);
    let data = package.export_data(0).ok_or("export data")?;
    println!(
        "export_len={} bytes_read={} tracks={track_count} frames={frames} len={sequence_length}",
        data.len(),
        properties.bytes_read
    );

    // 定位 FStripDataFlags + bSerializeCompressedData + 压缩头。
    let mut base = None;
    for candidate in properties.bytes_read..data.len().saturating_sub(64) {
        if data[candidate] & 1 == 0 {
            continue;
        }
        if read_i32(data, candidate + 2) == Some(0) {
            continue;
        }
        if read_i32(data, candidate + 10) != Some((track_count * 4) as i32) {
            continue;
        }
        base = Some(candidate);
        break;
    }
    let Some(base) = base else {
        println!("未找到压缩头");
        return Ok(());
    };
    println!("base={base}");
    let key_format = data[base + 6];
    let translation_format = data[base + 7];
    let rotation_format = data[base + 8];
    let scale_format = data[base + 9];
    let offsets_len = read_i32(data, base + 10).unwrap_or(0) as usize;
    let offsets_start = base + 14;
    let mut cursor = offsets_start + offsets_len * 4;
    let scale_len = read_i32(data, cursor).unwrap_or(0) as usize;
    cursor += 4;
    cursor += scale_len * 4;
    let strip_size = read_i32(data, cursor).unwrap_or(0);
    cursor += 4;
    let map_count = read_i32(data, cursor).unwrap_or(0) as usize;
    cursor += 4;
    cursor += map_count * 4;
    println!(
        "key={key_format} fmt=({translation_format},{rotation_format},{scale_format}) \
         offsets={offsets_len} scale={scale_len} strip={strip_size} map={map_count} curve_start={cursor}"
    );

    // 曲线数据长度未知：枚举候选，利用“压缩字节流结束于导出末尾”和轨道偏移
    // 必须落在流内来筛选唯一解。
    let mut candidates = Vec::new();
    for curve_len in 0..=4096usize {
        let p = cursor + curve_len;
        let raw_size = read_i32(data, p).unwrap_or(-1);
        let stream_len = read_i32(data, p + 4).unwrap_or(-1);
        if raw_size <= 0 || stream_len <= 0 {
            continue;
        }
        let expected = p + 8 + stream_len as usize;
        if expected > data.len() || data.len() - expected > 64 {
            continue;
        }
        let stream = &data[p + 8..data.len()];
        let mut valid = true;
        for track in 0..track_count {
            let entry = offsets_start + track * 16;
            let trans_offset = read_i32(data, entry).unwrap_or(-1);
            let trans_keys = read_i32(data, entry + 4).unwrap_or(-1);
            let rot_offset = read_i32(data, entry + 8).unwrap_or(-1);
            let rot_keys = read_i32(data, entry + 12).unwrap_or(-1);
            let rot_bytes = if rot_keys == 1 {
                12
            } else {
                24 + rot_keys as usize * 12
            };
            if trans_offset < 0
                || rot_offset < 0
                || trans_keys <= 0
                || rot_keys <= 0
                || trans_offset as usize + trans_keys as usize * 12 > stream.len()
                || rot_offset as usize + rot_bytes > stream.len()
            {
                valid = false;
                break;
            }
        }
        if valid {
            candidates.push((curve_len, raw_size, stream_len));
        }
    }
    println!("candidates={candidates:?}");
    let Some(&(curve_len, raw_size, stream_len)) = candidates.first() else {
        return Ok(());
    };
    let stream_start = cursor + curve_len + 8;
    println!(
        "curve_len={curve_len} raw_size={raw_size} stream_len={stream_len} stream_start={stream_start}"
    );
    let stream = &data[stream_start..stream_start + stream_len as usize];

    // 采样第 0 帧、中间帧和末帧的 track0..2 姿态。
    for ratio in [0.0f32, 0.5, 1.0] {
        println!("--- ratio={ratio}");
        for track in 0..track_count.min(3) {
            let entry = offsets_start + track * 16;
            let trans_offset = read_i32(data, entry).unwrap_or(-1);
            let trans_keys = read_i32(data, entry + 4).unwrap_or(-1);
            let rot_offset = read_i32(data, entry + 8).unwrap_or(-1);
            let rot_keys = read_i32(data, entry + 12).unwrap_or(-1);
            let translation = if trans_offset >= 0 && trans_keys > 0 {
                sample_vector(stream, trans_offset as usize, trans_keys as usize, ratio)
            } else {
                [0.0; 3]
            };
            let rotation = if rot_offset >= 0 && rot_keys > 0 {
                sample_rotation(
                    stream,
                    rot_offset as usize,
                    rot_keys as usize,
                    ratio,
                    rotation_format,
                )
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            println!(
                "track{track} keys=({trans_keys},{rot_keys}) t={translation:?} r={rotation:?}"
            );
        }
    }
    Ok(())
}

/// 采样平移轨道（float3 序列）。
fn sample_vector(stream: &[u8], offset: usize, keys: usize, ratio: f32) -> [f32; 3] {
    let position = ratio.clamp(0.0, 1.0) * keys.saturating_sub(1) as f32;
    let index = position.floor() as usize;
    let next = (index + 1).min(keys - 1);
    let alpha = position - index as f32;
    let a = read_vector(stream, offset + index * 12).unwrap_or([0.0; 3]);
    let b = read_vector(stream, offset + next * 12).unwrap_or(a);
    [
        a[0] + (b[0] - a[0]) * alpha,
        a[1] + (b[1] - a[1]) * alpha,
        a[2] + (b[2] - a[2]) * alpha,
    ]
}

/// 采样旋转轨道（Float96NoW 等）。
fn sample_rotation(stream: &[u8], offset: usize, keys: usize, ratio: f32, format: u8) -> [f32; 4] {
    let position = ratio.clamp(0.0, 1.0) * keys.saturating_sub(1) as f32;
    let index = position.floor() as usize;
    let next = (index + 1).min(keys - 1);
    let alpha = position - index as f32;
    let a = read_rotation_key(stream, offset, keys, index, format);
    let b = if next == index {
        a
    } else {
        read_rotation_key(stream, offset, keys, next, format)
    };
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

/// 读取一个旋转关键帧。
fn read_rotation_key(
    stream: &[u8],
    offset: usize,
    keys: usize,
    index: usize,
    format: u8,
) -> [f32; 4] {
    // keys==1 时头部是一个 FQuatFloat96NoW（3 个 float）；否则前面有 24 字节 bounds。
    let key_start = if keys == 1 { offset } else { offset + 24 };
    let bytes_per_key = match format {
        2 => 6,
        3..=5 => 4,
        _ => 12,
    };
    let key = key_start + index * bytes_per_key;
    match format {
        2 => {
            let raw = [
                read_u16(stream, key).unwrap_or(32767),
                read_u16(stream, key + 2).unwrap_or(32767),
                read_u16(stream, key + 4).unwrap_or(32767),
            ];
            let x = (raw[0] as f32 - 32767.0) / 32767.0;
            let y = (raw[1] as f32 - 32767.0) / 32767.0;
            let z = (raw[2] as f32 - 32767.0) / 32767.0;
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            [x, y, z, w]
        }
        _ => {
            let x = read_f32(stream, key).unwrap_or(0.0);
            let y = read_f32(stream, key + 4).unwrap_or(0.0);
            let z = read_f32(stream, key + 8).unwrap_or(0.0);
            let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
            [x, y, z, w]
        }
    }
}

/// 读取 float3。
fn read_vector(data: &[u8], offset: usize) -> Option<[f32; 3]> {
    Some([
        read_f32(data, offset)?,
        read_f32(data, offset + 4)?,
        read_f32(data, offset + 8)?,
    ])
}

/// 读取 f32。
fn read_f32(data: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// 读取 u16。
fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

/// 读取 i32。
fn read_i32(data: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}
