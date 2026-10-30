//! 轻量 CPU 光栅器：把首个 LOD 渲染成 RGBA 缩略图。
//!
//! 固定三等轴相机、正交投影、Z-buffer 和简单方向光；用于详情面板/资产
//! 查看器的静态预览。真正的轨道相机和交互放入 UI 层。

use super::{MeshLod, Skeleton};

/// 背景颜色（深灰）。
const BACKGROUND: [u8; 4] = [36, 40, 48, 255];
/// 每个 material 槽使用的基色。
const PALETTE: [[u8; 3]; 8] = [
    [120, 170, 255],
    [255, 160, 110],
    [130, 220, 160],
    [240, 210, 120],
    [220, 140, 220],
    [120, 220, 220],
    [240, 150, 160],
    [180, 180, 200],
];

/// 渲染一个 LOD 为宽 `width`、高 `height` 的 RGBA 图像（默认视角）。
pub fn render_static_mesh(mesh: &MeshLod, width: u32, height: u32) -> Vec<u8> {
    render_static_mesh_orbit(mesh, width, height, 35.0, 25.0, 1.0)
}

/// 使用可调轨道角度渲染模型。
///
/// `yaw_deg`/`pitch_deg` 为角度，`zoom` 为缩放倍率；供交互查看器使用。
pub fn render_static_mesh_orbit(
    mesh: &MeshLod,
    width: u32,
    height: u32,
    yaw_deg: f32,
    pitch_deg: f32,
    zoom: f32,
) -> Vec<u8> {
    let width = usize::try_from(width.max(16)).unwrap_or(16);
    let height = usize::try_from(height.max(16)).unwrap_or(16);
    let mut pixels = vec![0u8; width * height * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&BACKGROUND);
    }
    let Some((min, max)) = bounds(&mesh.positions) else {
        return pixels;
    };
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = (max[0] - min[0])
        .max(max[1] - min[1])
        .max(max[2] - min[2])
        .max(1.0e-5);
    let (sin_yaw, cos_yaw) = f64::from(yaw_deg.to_radians()).sin_cos();
    let (sin_pitch, cos_pitch) = f64::from(pitch_deg.to_radians()).sin_cos();
    let scale = (width.min(height) as f64 * 0.42) / extent * f64::from(zoom.clamp(0.1, 8.0));
    let orient = |position: [f64; 3]| -> [f64; 3] {
        let x = position[0] - center[0];
        let y = position[1] - center[1];
        let z = position[2] - center[2];
        let xr = x * cos_yaw - y * sin_yaw;
        let yr = x * sin_yaw + y * cos_yaw;
        let yp = yr * cos_pitch - z * sin_pitch;
        let zp = yr * sin_pitch + z * cos_pitch;
        [xr * scale, -zp * scale, yp]
    };
    let screen_center = [width as f64 * 0.5, height as f64 * 0.5];
    let projected: Vec<[f64; 3]> = mesh
        .positions
        .iter()
        .map(|position| orient(*position))
        .collect();
    let mut depth = vec![f64::INFINITY; width * height];
    let light = normalize([-0.35, -0.65, 0.7]);
    for (triangle_index, triangle) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let a = projected[triangle[0] as usize];
        let b = projected[triangle[1] as usize];
        let c = projected[triangle[2] as usize];
        let normal = cross(sub(b, a), sub(c, a));
        let normal = normalize(normal);
        let shade = 0.62
            + 0.55
                * normal[0]
                    .mul_add(light[0], normal[1] * light[1] + normal[2] * light[2])
                    .abs();
        let color = section_color(mesh, triangle_index);
        let shade = shade.clamp(0.0, 1.15);
        let color = [
            (color[0] as f64 * shade).round().clamp(0.0, 255.0) as u8,
            (color[1] as f64 * shade).round().clamp(0.0, 255.0) as u8,
            (color[2] as f64 * shade).round().clamp(0.0, 255.0) as u8,
        ];
        rasterize(
            a,
            b,
            c,
            color,
            &mut pixels,
            &mut depth,
            width,
            height,
            screen_center,
        );
    }
    pixels
}

/// 返回三角形所属 section 的颜色。
fn section_color(mesh: &MeshLod, triangle_index: usize) -> [u8; 3] {
    let first = triangle_index * 3;
    let section = mesh
        .sections
        .iter()
        .find(|section| {
            let start = section.first_index.max(0) as usize;
            let end = start + section.triangle_count.max(0) as usize * 3;
            first >= start && first < end
        })
        .or_else(|| mesh.sections.first());
    let material = section
        .map(|section| section.material_index.max(0) as usize)
        .unwrap_or(0);
    PALETTE[material % PALETTE.len()]
}

/// 光栅化一个三角形。
#[allow(clippy::too_many_arguments)]
fn rasterize(
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    color: [u8; 3],
    pixels: &mut [u8],
    depth: &mut [f64],
    width: usize,
    height: usize,
    center: [f64; 2],
) {
    let area = edge(a, b, c);
    if area.abs() < 1.0e-9 {
        return;
    }
    let min_x = a[0].min(b[0]).min(c[0]) + center[0];
    let max_x = a[0].max(b[0]).max(c[0]) + center[0];
    let min_y = a[1].min(b[1]).min(c[1]) + center[1];
    let max_y = a[1].max(b[1]).max(c[1]) + center[1];
    let x0 = min_x.floor().clamp(0.0, width as f64 - 1.0) as usize;
    let x1 = max_x.ceil().clamp(0.0, width as f64 - 1.0) as usize;
    let y0 = min_y.floor().clamp(0.0, height as f64 - 1.0) as usize;
    let y1 = max_y.ceil().clamp(0.0, height as f64 - 1.0) as usize;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = [x as f64 + 0.5 - center[0], y as f64 + 0.5 - center[1]];
            let wa = edge(b, c, [p[0], p[1], 0.0]) / area;
            let wb = edge(c, a, [p[0], p[1], 0.0]) / area;
            let wc = 1.0 - wa - wb;
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let z = a[2] * wa + b[2] * wb + c[2] * wc;
            let index = y * width + x;
            if z < depth[index] {
                depth[index] = z;
                let offset = index * 4;
                pixels[offset..offset + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
}

/// 二维叉积，z 分量表示面积。
fn edge(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// 三维叉积。
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 三维相减。
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 归一化向量。
fn normalize(value: [f64; 3]) -> [f64; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2])
        .sqrt()
        .max(1.0e-12);
    [value[0] / length, value[1] / length, value[2] / length]
}

/// 计算顶点包围盒。
fn bounds(positions: &[[f64; 3]]) -> Option<([f64; 3], [f64; 3])> {
    let first = positions.first()?;
    let mut min = *first;
    let mut max = *first;
    for position in positions.iter().skip(1) {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    Some((min, max))
}

/// 渲染骨骼参考姿态为线条图。
///
/// 骨骼参考姿态是相对父骨骼的局部变换；这里累积世界旋转/平移后，按
/// 固定三轴相机把根到子骨骼的连线投影成图像。
pub fn render_skeleton_orbit(
    skeleton: &Skeleton,
    width: u32,
    height: u32,
    yaw_deg: f32,
    pitch_deg: f32,
    zoom: f32,
) -> Vec<u8> {
    let width = usize::try_from(width.max(16)).unwrap_or(16);
    let height = usize::try_from(height.max(16)).unwrap_or(16);
    let mut pixels = vec![0u8; width * height * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&BACKGROUND);
    }
    if skeleton.bones.is_empty() {
        return pixels;
    }
    let (positions, _rotations) = world_bones(skeleton);
    let Some((min, max)) = bounds(&positions) else {
        return pixels;
    };
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = (max[0] - min[0])
        .max(max[1] - min[1])
        .max(max[2] - min[2])
        .max(1.0e-4);
    let (sin_yaw, cos_yaw) = f64::from(yaw_deg.to_radians()).sin_cos();
    let (sin_pitch, cos_pitch) = f64::from(pitch_deg.to_radians()).sin_cos();
    let scale = (width.min(height) as f64 * 0.4) / extent * f64::from(zoom.clamp(0.1, 8.0));
    let center_screen = [width as f64 * 0.5, height as f64 * 0.5];
    let project = |position: [f64; 3]| -> [f64; 3] {
        let x = position[0] - center[0];
        let y = position[1] - center[1];
        let z = position[2] - center[2];
        let xr = x * cos_yaw - y * sin_yaw;
        let yr = x * sin_yaw + y * cos_yaw;
        let yp = yr * cos_pitch - z * sin_pitch;
        let zp = yr * sin_pitch + z * cos_pitch;
        [
            xr * scale + center_screen[0],
            -zp * scale + center_screen[1],
            yp,
        ]
    };
    let projected: Vec<[f64; 3]> = positions
        .iter()
        .map(|position| project(*position))
        .collect();
    let mut depth = vec![f64::INFINITY; width * height];
    for (index, bone) in skeleton.bones.iter().enumerate() {
        if bone.parent_index >= 0 {
            let parent = bone.parent_index as usize;
            if parent < index {
                let color = PALETTE[index % PALETTE.len()];
                draw_segment(
                    projected[parent],
                    projected[index],
                    color,
                    &mut pixels,
                    &mut depth,
                    width,
                    height,
                );
            }
        }
        draw_joint(
            projected[index],
            PALETTE[index % PALETTE.len()],
            &mut pixels,
            &mut depth,
            width,
            height,
        );
    }
    pixels
}

/// 累积骨骼世界位置和旋转。
fn world_bones(skeleton: &Skeleton) -> (Vec<[f64; 3]>, Vec<[f64; 4]>) {
    let mut positions = vec![[0.0; 3]; skeleton.bones.len()];
    let mut rotations = vec![[0.0, 0.0, 0.0, 1.0]; skeleton.bones.len()];
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let local_rotation = normalize_quat(bone.rotation);
        if bone.parent_index >= 0 && (bone.parent_index as usize) < index {
            let parent = bone.parent_index as usize;
            positions[index] = add(
                positions[parent],
                rotate_quat(rotations[parent], bone.translation),
            );
            rotations[index] = quat_mul(rotations[parent], local_rotation);
        } else {
            positions[index] = bone.translation;
            rotations[index] = local_rotation;
        }
    }
    (positions, rotations)
}

/// 画一条有深度的线段。
#[allow(clippy::too_many_arguments)]
fn draw_segment(
    a: [f64; 3],
    b: [f64; 3],
    color: [u8; 3],
    pixels: &mut [u8],
    depth: &mut [f64],
    width: usize,
    height: usize,
) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as usize;
    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        let x = a[0] + dx * t;
        let y = a[1] + dy * t;
        let z = a[2] + (b[2] - a[2]) * t;
        put_pixel(x, y, z, color, pixels, depth, width, height);
        put_pixel(x + 1.0, y, z, color, pixels, depth, width, height);
    }
}

/// 画骨骼关节小方块。
fn draw_joint(
    point: [f64; 3],
    color: [u8; 3],
    pixels: &mut [u8],
    depth: &mut [f64],
    width: usize,
    height: usize,
) {
    for dy in -1..=1 {
        for dx in -1..=1 {
            put_pixel(
                point[0] + f64::from(dx),
                point[1] + f64::from(dy),
                point[2] - 0.001,
                color,
                pixels,
                depth,
                width,
                height,
            );
        }
    }
}

/// 写一个深度测试后的像素。
#[allow(clippy::too_many_arguments)]
fn put_pixel(
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 3],
    pixels: &mut [u8],
    depth: &mut [f64],
    width: usize,
    height: usize,
) {
    if x < 0.0 || y < 0.0 {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    if x >= width || y >= height {
        return;
    }
    let index = y * width + x;
    if z >= depth[index] {
        return;
    }
    depth[index] = z;
    let offset = index * 4;
    pixels[offset..offset + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
}

/// 四元数乘法。
fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// 用四元数旋转向量。
fn rotate_quat(quat: [f64; 4], value: [f64; 3]) -> [f64; 3] {
    let q = normalize_quat(quat);
    let u = [q[0], q[1], q[2]];
    let t = [
        2.0 * (u[1] * value[2] - u[2] * value[1]),
        2.0 * (u[2] * value[0] - u[0] * value[2]),
        2.0 * (u[0] * value[1] - u[1] * value[0]),
    ];
    [
        value[0] + q[3] * t[0] + u[1] * t[2] - u[2] * t[1],
        value[1] + q[3] * t[1] + u[2] * t[0] - u[0] * t[2],
        value[2] + q[3] * t[2] + u[0] * t[1] - u[1] * t[0],
    ]
}

/// 归一化四元数。
fn normalize_quat(mut quat: [f64; 4]) -> [f64; 4] {
    let length =
        (quat[0] * quat[0] + quat[1] * quat[1] + quat[2] * quat[2] + quat[3] * quat[3]).sqrt();
    if length < 1.0e-12 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    for value in &mut quat {
        *value /= length;
    }
    quat
}

/// 两个三维向量相加。
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::MeshSection;

    /// 渲染 fixture 风格的两个三角形，输出图像含前景像素。
    #[test]
    fn renders_non_empty_mesh() {
        let mesh = MeshLod {
            positions: vec![
                [-50.0, -50.0, 0.0],
                [50.0, -50.0, 0.0],
                [50.0, 50.0, 20.0],
                [-50.0, 50.0, 0.0],
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            sections: vec![MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 2,
            }],
        };
        let image = render_static_mesh(&mesh, 96, 96);
        assert!(
            image
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != BACKGROUND)
        );
    }

    /// 改变轨道角度应产生不同图像。
    #[test]
    fn orbit_changes_rendered_image() {
        let mesh = MeshLod {
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            sections: vec![MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 2,
            }],
        };
        let front = render_static_mesh_orbit(&mesh, 64, 64, 0.0, 0.0, 1.0);
        let side = render_static_mesh_orbit(&mesh, 64, 64, 70.0, 20.0, 1.0);
        assert_ne!(front, side);
    }

    /// 骨骼参考姿态渲染出可见连线。
    #[test]
    fn renders_skeleton_line_art() {
        use crate::package::Bone;

        let skeleton = Skeleton {
            name: "SKEL_Test".to_string(),
            bones: vec![
                Bone {
                    name: "Root".to_string(),
                    parent_index: -1,
                    translation: [0.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                Bone {
                    name: "Joint".to_string(),
                    parent_index: 0,
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ],
        };
        let image = render_skeleton_orbit(&skeleton, 96, 96, 35.0, 25.0, 1.0);
        assert!(
            image
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != BACKGROUND)
        );
    }
}
