//! ActorX PSA 动画导出（版本 20100422）。
//!
//! 布局参考 CUE4Parse `ActorXAnim`：
//! `ANIMHEAD`、`BONENAMES`（120 字节/骨骼）、`ANIMINFO`（168 字节）、
//! `ANIMKEYS`（32 字节/关键帧：position+quat+time）、`SCALEKEYS`（16 字节：scale+time）。
//! 与 PSK 一致做 Y 镜像；缩放键写相对参考姿态的比例。

use crate::{ExportBone, ExportError, ExportOptions};

/// PSA 主版本号。
const PSA_VERSION: i32 = 20_100_422;
/// 骨骼记录长度。
const BONE_SIZE: i32 = 120;
/// 动画信息长度。
const ANIM_INFO_SIZE: i32 = 168;
/// 四元数关键帧长度。
const QUAT_KEY_SIZE: i32 = 32;
/// 缩放关键帧长度。
const SCALE_KEY_SIZE: i32 = 16;

/// 一帧中一根骨骼的关键值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimKey {
    /// 局部平移。
    pub position: [f32; 3],
    /// 局部旋转（xyzw）。
    pub rotation: [f32; 4],
    /// 局部缩放。
    pub scale: [f32; 3],
}

/// PSA 导出输入。
#[derive(Debug, Clone)]
pub struct PsaExport {
    /// 动画名。
    pub name: String,
    /// 分组名。
    pub group: String,
    /// 帧数。
    pub frames: u32,
    /// 帧率。
    pub fps: f32,
    /// 参考骨架（顺序与关键帧一致）。
    pub bones: Vec<ExportBone>,
    /// 逐帧逐骨骼关键值：`keys[frame][bone]`。
    pub keys: Vec<Vec<AnimKey>>,
}

impl Default for PsaExport {
    fn default() -> Self {
        Self {
            name: "Animation".to_string(),
            group: "None".to_string(),
            frames: 0,
            fps: 30.0,
            bones: Vec::new(),
            keys: Vec::new(),
        }
    }
}

/// 写出 PSA。
pub fn write_psa(export: &PsaExport, options: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    validate(export)?;
    let scale = options.scale.max(f32::MIN_POSITIVE);
    let bones = &export.bones;
    let frames = export.frames;
    let key_count = frames as i32 * bones.len() as i32;
    let mut out = Vec::new();

    // ANIMHEAD。
    write_chunk(&mut out, "ANIMHEAD", PSA_VERSION, 0, 0, &[]);

    // BONENAMES。
    let mut bone_bytes = Vec::with_capacity(bones.len() * BONE_SIZE as usize);
    for (index, bone) in bones.iter().enumerate() {
        write_fixed_name(&mut bone_bytes, &bone.name);
        bone_bytes.extend_from_slice(&0u32.to_le_bytes()); // Flags
        let children = bones
            .iter()
            .enumerate()
            .filter(|(child, candidate)| *child != index && candidate.parent_index == index as i32)
            .count();
        bone_bytes.extend_from_slice(&(children as i32).to_le_bytes());
        bone_bytes.extend_from_slice(&bone.parent_index.to_le_bytes());
        let mut rotation = bone.rotation;
        rotation[1] = -rotation[1];
        if index == 0 {
            rotation[3] = -rotation[3];
        }
        for value in rotation {
            bone_bytes.extend_from_slice(&value.to_le_bytes());
        }
        push_vec3(
            &mut bone_bytes,
            [
                bone.position[0] * scale,
                -bone.position[1] * scale,
                bone.position[2] * scale,
            ],
        );
        bone_bytes.extend_from_slice(&1.0f32.to_le_bytes()); // Length
        push_vec3(&mut bone_bytes, [0.0; 3]); // Size
    }
    write_chunk(
        &mut out,
        "BONENAMES",
        0,
        BONE_SIZE,
        bones.len() as i32,
        &bone_bytes,
    );

    // ANIMINFO。
    let mut info = Vec::with_capacity(ANIM_INFO_SIZE as usize);
    write_fixed_name(&mut info, &export.name);
    write_fixed_name(&mut info, &export.group);
    info.extend_from_slice(&(bones.len() as i32).to_le_bytes()); // TotalBones
    info.extend_from_slice(&0i32.to_le_bytes()); // RootInclude
    info.extend_from_slice(&0i32.to_le_bytes()); // KeyCompressionStyle
    info.extend_from_slice(&key_count.to_le_bytes()); // KeyQuotum
    info.extend_from_slice(&0f32.to_le_bytes()); // KeyReduction
    info.extend_from_slice(&(frames as f32).to_le_bytes()); // TrackTime
    info.extend_from_slice(&export.fps.to_le_bytes()); // AnimRate
    info.extend_from_slice(&0i32.to_le_bytes()); // StartBone
    info.extend_from_slice(&0i32.to_le_bytes()); // FirstRawFrame
    info.extend_from_slice(&(frames as i32).to_le_bytes()); // NumRawFrames
    write_chunk(&mut out, "ANIMINFO", 0, ANIM_INFO_SIZE, 1, &info);

    // ANIMKEYS：帧优先排列。
    let mut keys = Vec::with_capacity(key_count as usize * QUAT_KEY_SIZE as usize);
    for frame in &export.keys {
        for (bone_index, key) in frame.iter().enumerate() {
            push_vec3(
                &mut keys,
                [
                    key.position[0] * scale,
                    -key.position[1] * scale,
                    key.position[2] * scale,
                ],
            );
            let mut rotation = key.rotation;
            rotation[1] = -rotation[1];
            if bone_index == 0 {
                rotation[3] = -rotation[3];
            }
            for value in rotation {
                keys.extend_from_slice(&value.to_le_bytes());
            }
            keys.extend_from_slice(&1.0f32.to_le_bytes()); // time
        }
    }
    write_chunk(&mut out, "ANIMKEYS", 0, QUAT_KEY_SIZE, key_count, &keys);

    // SCALEKEYS：相对参考缩放。
    let mut scale_keys = Vec::with_capacity(key_count as usize * SCALE_KEY_SIZE as usize);
    for frame in &export.keys {
        for key in frame.iter() {
            let reference = rest_scale();
            let relative = [
                ratio(key.scale[0], reference[0]),
                ratio(key.scale[1], reference[1]),
                ratio(key.scale[2], reference[2]),
            ];
            push_vec3(&mut scale_keys, relative);
            scale_keys.extend_from_slice(&1.0f32.to_le_bytes());
        }
    }
    write_chunk(
        &mut out,
        "SCALEKEYS",
        0,
        SCALE_KEY_SIZE,
        key_count,
        &scale_keys,
    );
    Ok(out)
}

/// 参考缩放暂固定为 1（ExportBone 不携带缩放）。
fn rest_scale() -> [f32; 3] {
    [1.0; 3]
}

/// 相对缩放比例；参考为 0 时回退 1。
fn ratio(value: f32, reference: f32) -> f32 {
    if reference.abs() <= f32::EPSILON {
        1.0
    } else {
        value / reference
    }
}

/// 校验动画输入。
fn validate(export: &PsaExport) -> Result<(), ExportError> {
    if export.frames == 0 || export.bones.is_empty() {
        return Err(ExportError::Serialization("PSA 缺少帧或骨骼".to_string()));
    }
    if export.keys.len() != export.frames as usize {
        return Err(ExportError::Serialization(format!(
            "PSA 关键帧数量 {} 与帧数 {} 不一致",
            export.keys.len(),
            export.frames
        )));
    }
    for (frame, keys) in export.keys.iter().enumerate() {
        if keys.len() != export.bones.len() {
            return Err(ExportError::Serialization(format!(
                "PSA 第 {frame} 帧骨骼数 {} 与骨架 {} 不一致",
                keys.len(),
                export.bones.len()
            )));
        }
        for key in keys {
            if !key
                .position
                .iter()
                .chain(key.rotation.iter())
                .chain(key.scale.iter())
                .all(|value| value.is_finite())
            {
                return Err(ExportError::Serialization(format!(
                    "PSA 第 {frame} 帧含非有限数值"
                )));
            }
        }
    }
    Ok(())
}

/// 写 32 字节 chunk 头 + 数据体。
fn write_chunk(
    out: &mut Vec<u8>,
    name: &str,
    type_flag: i32,
    data_size: i32,
    count: i32,
    body: &[u8],
) {
    let mut header = [0u8; 20];
    let bytes = name.as_bytes();
    let length = bytes.len().min(header.len());
    header[..length].copy_from_slice(&bytes[..length]);
    out.extend_from_slice(&header);
    out.extend_from_slice(&type_flag.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(body);
}

/// 固定长度名称字段。
fn write_fixed_name(out: &mut Vec<u8>, name: &str) {
    let mut field = [0u8; 64];
    let bytes = name.as_bytes();
    let length = bytes.len().min(field.len());
    field[..length].copy_from_slice(&bytes[..length]);
    out.extend_from_slice(&field);
}

/// 写入 vec3。
fn push_vec3(out: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        out.extend_from_slice(&component.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export() -> PsaExport {
        let bones = vec![
            ExportBone {
                name: "Root".to_string(),
                parent_index: -1,
                position: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            ExportBone {
                name: "Child".to_string(),
                parent_index: 0,
                position: [0.0, 10.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
        ];
        let frame = |x: f32| {
            vec![
                AnimKey {
                    position: [x, 1.0, 0.0],
                    rotation: [0.1, 0.2, 0.3, 0.9],
                    scale: [2.0, 1.0, 1.0],
                },
                AnimKey {
                    position: [0.0, 20.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ]
        };
        PsaExport {
            name: "ANIM_Test".to_string(),
            group: "None".to_string(),
            frames: 2,
            fps: 30.0,
            bones,
            keys: vec![frame(0.0), frame(5.0)],
        }
    }

    /// chunk 布局与镜像。
    #[test]
    fn writes_psa_chunks() {
        let data = write_psa(&export(), &ExportOptions::default()).expect("PSA");
        let mut position = 0usize;
        let mut chunks = Vec::new();
        while position + 32 <= data.len() {
            let name = String::from_utf8_lossy(&data[position..position + 20])
                .trim_end_matches('\0')
                .to_string();
            let data_size =
                i32::from_le_bytes(data[position + 24..position + 28].try_into().unwrap());
            let count = i32::from_le_bytes(data[position + 28..position + 32].try_into().unwrap());
            chunks.push((name, data_size, count, position + 32));
            position += 32 + data_size.max(0) as usize * count.max(0) as usize;
        }
        let names: Vec<&str> = chunks.iter().map(|(name, ..)| name.as_str()).collect();
        assert_eq!(
            names,
            ["ANIMHEAD", "BONENAMES", "ANIMINFO", "ANIMKEYS", "SCALEKEYS"]
        );
        assert_eq!(chunks[0].1, 0);
        assert_eq!(chunks[1].2, 2);
        assert_eq!(chunks[1].1, BONE_SIZE);
        assert_eq!(chunks[2].1, ANIM_INFO_SIZE);
        assert_eq!(chunks[3].1, QUAT_KEY_SIZE);
        assert_eq!(chunks[3].2, 4);
        assert_eq!(chunks[4].1, SCALE_KEY_SIZE);
        let keys = chunks[3].3;
        let y = f32::from_le_bytes(data[keys + 4..keys + 8].try_into().unwrap());
        assert!((y + 1.0).abs() < 1e-6, "PSA 位置 Y 应镜像：{y}");
        let root_w = f32::from_le_bytes(data[keys + 12 + 12..keys + 12 + 16].try_into().unwrap());
        assert!(
            (root_w + 0.9).abs() < 1e-6,
            "根骨骼 quat W 应取反：{root_w}"
        );
    }

    /// 帧/骨骼数量不一致时报错。
    #[test]
    fn rejects_invalid_animation() {
        let mut export = export();
        export.frames = 3;
        assert!(write_psa(&export, &ExportOptions::default()).is_err());
        let mut export = self::export();
        export.keys[0].pop();
        assert!(write_psa(&export, &ExportOptions::default()).is_err());
    }
}
