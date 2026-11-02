//! 轻量 CPU 光栅器：把首个 LOD 渲染成 RGBA 缩略图。
//!
//! 固定三等轴相机、正交投影、Z-buffer 和简单方向光；用于详情面板/资产
//! 查看器的静态预览。真正的轨道相机和交互放入 UI 层。

use super::{MeshLod, Skeleton};

use super::MaterialAppearance;

/// 材质球采样用贴图（RGBA8）。
#[derive(Clone, Copy)]
pub struct MaterialPreviewTexture<'a> {
    /// RGBA8 像素。
    pub rgba: &'a [u8],
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
}

impl<'a> MaterialPreviewTexture<'a> {
    /// 构造贴图引用。
    pub fn new(rgba: &'a [u8], width: u32, height: u32) -> Self {
        Self {
            rgba,
            width,
            height,
        }
    }

    /// 按 UV 采样（双线性，UV 环绕）。
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        if self.width == 0 || self.height == 0 {
            return [0.0; 3];
        }
        let wrap = |value: f32, size: u32| {
            let scaled = value - value.floor();
            scaled * size as f32 - 0.5
        };
        let x = wrap(u, self.width);
        let y = wrap(v, self.height);
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let fetch = |px: i32, py: i32| {
            let px = px.rem_euclid(self.width as i32) as u32;
            let py = py.rem_euclid(self.height as i32) as u32;
            let offset = ((py * self.width + px) * 4) as usize;
            let data = self.rgba;
            if offset + 3 < data.len() {
                [
                    data[offset] as f32 / 255.0,
                    data[offset + 1] as f32 / 255.0,
                    data[offset + 2] as f32 / 255.0,
                ]
            } else {
                [0.0; 3]
            }
        };
        let c00 = fetch(x0, y0);
        let c10 = fetch(x0 + 1, y0);
        let c01 = fetch(x0, y0 + 1);
        let c11 = fetch(x0 + 1, y0 + 1);
        let mut out = [0.0; 3];
        for channel in 0..3 {
            let top = c00[channel] + (c10[channel] - c00[channel]) * fx;
            let bottom = c01[channel] + (c11[channel] - c01[channel]) * fx;
            out[channel] = top + (bottom - top) * fy;
        }
        out
    }
}

/// 用近似 PBR 参数渲染一个材质球（正方形）。
///
/// 只做单方向光 + 简单高光，用于在详情面板快速判断材质颜色/贴图/粗糙度；
/// 不还原完整材质图或 UE 的着色模型。
pub fn render_material_ball(
    appearance: &MaterialAppearance,
    base_color_texture: Option<MaterialPreviewTexture<'_>>,
    normal_texture: Option<MaterialPreviewTexture<'_>>,
    metallic_roughness_texture: Option<MaterialPreviewTexture<'_>>,
    size: u32,
) -> Vec<u8> {
    render_material_ball_orbit(
        appearance,
        base_color_texture,
        normal_texture,
        metallic_roughness_texture,
        0.0,
        0.0,
        1.0,
        size,
    )
}

/// 用近似 PBR 参数渲染可绕球体旋转的材质球（交互式材质查看器）。
///
/// `yaw`/`pitch` 是相机绕球体旋转的角度（度），`zoom` 缩放球体；光线和
/// 视线保持世界空间固定，因此转动时能观察到真实的明暗变化。
#[allow(clippy::too_many_arguments)]
pub fn render_material_ball_orbit(
    appearance: &MaterialAppearance,
    base_color_texture: Option<MaterialPreviewTexture<'_>>,
    normal_texture: Option<MaterialPreviewTexture<'_>>,
    metallic_roughness_texture: Option<MaterialPreviewTexture<'_>>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    size: u32,
) -> Vec<u8> {
    let size = size.max(16);
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 * 0.5;
    let radius = (size as f32 * 0.42 * zoom.clamp(0.2, 6.0)).min(size as f32 * 2.0);
    let camera_yaw = yaw.to_radians();
    let camera_pitch = pitch.to_radians();
    let light = normalize3([-0.42, 0.5, 0.76]);
    let view = world_from_camera([0.0, 0.0, 1.0], camera_yaw, camera_pitch);
    let half = normalize3([light[0] + view[0], light[1] + view[1], light[2] + view[2]]);
    let right = world_from_camera([1.0, 0.0, 0.0], camera_yaw, camera_pitch);
    let up = world_from_camera([0.0, 1.0, 0.0], camera_yaw, camera_pitch);
    for y in 0..size {
        for x in 0..size {
            let fx = (x as f32 + 0.5 - center) / radius;
            let fy = (center - y as f32 - 0.5) / radius;
            let distance = (fx * fx + fy * fy).sqrt();
            let offset = ((y * size + x) * 4) as usize;
            if distance > 1.0 {
                // 球下方软阴影。
                let shadow = if distance < 1.35 && fy < 0.0 {
                    let falloff = 1.0 - ((distance - 1.0) / 0.35).clamp(0.0, 1.0);
                    (falloff * 0.35 * (1.0 + fy)).clamp(0.0, 0.35)
                } else {
                    0.0
                };
                let base = [24.0, 27.0, 33.0];
                let color = [
                    base[0] * (1.0 - shadow),
                    base[1] * (1.0 - shadow),
                    base[2] * (1.0 - shadow),
                ];
                pixels[offset] = color[0] as u8;
                pixels[offset + 1] = color[1] as u8;
                pixels[offset + 2] = color[2] as u8;
                pixels[offset + 3] = 255;
                continue;
            }
            let mut normal = normalize3(world_from_camera(
                [fx, fy, (1.0 - fx * fx - fy * fy).max(0.0).sqrt()],
                camera_yaw,
                camera_pitch,
            ));
            // 球面 UV：u 沿赤道，v 沿极轴。
            let u = 0.5 + normal[0].atan2(normal[2]) / std::f32::consts::TAU;
            let v = 0.5 - normal[1].asin() / std::f32::consts::PI;
            let mut base_color = appearance.base_color;
            if let Some(texture) = base_color_texture {
                let sample = texture.sample(u, v);
                // 贴图存在时作为基础色；参数色只在与默认灰差异很大时作为染色。
                base_color = [
                    sample[0] * base_color[0].max(0.35) * 1.6,
                    sample[1] * base_color[1].max(0.35) * 1.6,
                    sample[2] * base_color[2].max(0.35) * 1.6,
                ];
                base_color
                    .iter_mut()
                    .for_each(|value| *value = value.min(1.0));
            }
            let mut roughness = appearance.roughness.clamp(0.04, 1.0);
            let mut metallic = appearance.metallic.clamp(0.0, 1.0);
            if let Some(texture) = metallic_roughness_texture {
                let sample = texture.sample(u, v);
                roughness *= sample[1].max(0.05);
                metallic = metallic.max(sample[2]);
            }
            if let Some(texture) = normal_texture {
                let sample = texture.sample(u, v);
                let tangent = [sample[0] * 2.0 - 1.0, sample[1] * 2.0 - 1.0];
                for channel in 0..3 {
                    normal[channel] +=
                        (right[channel] * tangent[0] + up[channel] * tangent[1]) * 0.55;
                }
                normal = normalize3(normal);
            }
            let ndotl = dot3(normal, light).max(0.0);
            let ndoth = dot3(normal, half).max(0.0);
            let ndotv = dot3(normal, view).max(0.0);
            let shininess: f32 =
                (2.0f32 / (roughness * roughness * roughness * roughness) - 2.0).clamp(4.0, 256.0);
            let specular = ndoth.powf(shininess) * (0.04 + metallic * 0.5);
            let fresnel = 0.04 + 0.96 * (1.0 - ndotv).powi(5);
            let specular_tint = [
                1.0 - metallic + base_color[0] * metallic,
                1.0 - metallic + base_color[1] * metallic,
                1.0 - metallic + base_color[2] * metallic,
            ];
            let ambient = 0.16;
            let diffuse = ambient + 0.84 * ndotl;
            let mut color = [0.0f32; 3];
            for channel in 0..3 {
                let diffuse_term = base_color[channel] * diffuse * (1.0 - metallic);
                let specular_term = specular_tint[channel] * specular * (1.0 - roughness * 0.6);
                let rim = fresnel * 0.18 * diffuse;
                color[channel] =
                    diffuse_term + specular_term + rim + appearance.emissive[channel] * 0.8;
            }
            let encode = |value: f32| -> u8 {
                (value.max(0.0).powf(1.0 / 2.2) * 255.0).clamp(0.0, 255.0) as u8
            };
            pixels[offset] = encode(color[0]);
            pixels[offset + 1] = encode(color[1]);
            pixels[offset + 2] = encode(color[2]);
            pixels[offset + 3] = 255;
        }
    }
    pixels
}

/// 把相机空间的向量转换到世界空间（相机绕球体 yaw/pitch 旋转的逆变换）。
fn world_from_camera(value: [f32; 3], yaw: f32, pitch: f32) -> [f32; 3] {
    // R = Ry(yaw) * Rx(pitch)，世界向量 = R^T * 相机向量。
    let yaw = -yaw;
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = (-pitch).sin_cos();
    let x = value[0] * cos_yaw + value[2] * sin_yaw;
    let z = -value[0] * sin_yaw + value[2] * cos_yaw;
    let y = value[1] * cos_pitch - z * sin_pitch;
    let z = value[1] * sin_pitch + z * cos_pitch;
    [x, y, z]
}

/// 三维向量点积。
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 三维向量归一化。
fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, value[1] / length, value[2] / length]
    }
}
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
/// 渲染 SkeletalMesh rest pose，并叠加参考骨骼线条。
pub fn render_skeletal_mesh_orbit(
    mesh: &MeshLod,
    skeleton: &Skeleton,
    width: u32,
    height: u32,
    yaw_deg: f32,
    pitch_deg: f32,
    zoom: f32,
) -> Vec<u8> {
    let width = usize::try_from(width.max(16)).unwrap_or(16);
    let height = usize::try_from(height.max(16)).unwrap_or(16);
    let mut pixels =
        render_static_mesh_orbit(mesh, width as u32, height as u32, yaw_deg, pitch_deg, zoom);
    if skeleton.bones.is_empty() {
        return pixels;
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
    let screen_center = [width as f64 * 0.5, height as f64 * 0.5];
    let (bone_positions, _) = world_bones(skeleton);
    let projected: Vec<[f64; 3]> = bone_positions
        .iter()
        .map(|position| {
            let x = position[0] - center[0];
            let y = position[1] - center[1];
            let z = position[2] - center[2];
            let xr = x * cos_yaw - y * sin_yaw;
            let yr = x * sin_yaw + y * cos_yaw;
            let yp = yr * cos_pitch - z * sin_pitch;
            let zp = yr * sin_pitch + z * cos_pitch;
            [
                xr * scale + screen_center[0],
                -zp * scale + screen_center[1],
                yp,
            ]
        })
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

/// 渲染骨架（带轨道相机）。
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
            attributes: Default::default(),
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
            attributes: Default::default(),
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

#[cfg(test)]
mod material_tests {
    use super::*;

    /// 材质球应输出非透明球体，中心比角落亮。
    #[test]
    fn renders_shaded_material_ball() {
        let appearance = MaterialAppearance {
            base_color: [0.8, 0.2, 0.2],
            ..Default::default()
        };
        let pixels = render_material_ball(&appearance, None, None, None, 96);
        assert_eq!(pixels.len(), 96 * 96 * 4);
        let center = ((48 * 96 + 48) * 4) as usize;
        let corner = 0usize;
        let center_luma =
            pixels[center] as u32 + pixels[center + 1] as u32 + pixels[center + 2] as u32;
        let corner_luma =
            pixels[corner] as u32 + pixels[corner + 1] as u32 + pixels[corner + 2] as u32;
        assert!(center_luma > corner_luma, "球体中心应更亮");
        assert!(pixels[center] > pixels[center + 1], "红色基础色应偏红");
        assert_eq!(pixels[center + 3], 255);
    }

    /// 基础色贴图应参与采样，球面出现贴图里的两种颜色。
    #[test]
    fn samples_base_color_texture() {
        let texture = [
            255u8, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255,
        ];
        let appearance = MaterialAppearance::default();
        let pixels = render_material_ball(
            &appearance,
            Some(MaterialPreviewTexture::new(&texture, 2, 2)),
            None,
            None,
            96,
        );
        let mut red = 0;
        let mut blue = 0;
        for pixel in pixels.as_chunks::<4>().0 {
            if pixel[0] as i32 - pixel[2] as i32 > 40 {
                red += 1;
            }
            if pixel[2] as i32 - pixel[0] as i32 > 40 {
                blue += 1;
            }
        }
        assert!(red > 20, "应采样到红色区域");
        assert!(blue > 20, "应采样到蓝色区域");
    }
}

/// 交互式材质查看器：相机旋转后材质球画面应变化。
#[test]
fn renders_orbit_material_ball() {
    let texture = [
        255u8, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255,
    ];
    let appearance = MaterialAppearance::default();
    let base = MaterialPreviewTexture::new(&texture, 2, 2);
    let front = render_material_ball_orbit(&appearance, Some(base), None, None, 0.0, 0.0, 1.0, 96);
    let side = render_material_ball_orbit(
        &appearance,
        Some(MaterialPreviewTexture::new(&texture, 2, 2)),
        None,
        None,
        140.0,
        10.0,
        1.0,
        96,
    );
    assert_eq!(front.len(), 96 * 96 * 4);
    assert_ne!(front, side, "旋转相机后材质球应重绘");
}
