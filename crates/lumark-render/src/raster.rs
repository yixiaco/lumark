//! CPU 光栅化：三角形收集、近裁面裁剪、Z-buffer 与近似 PBR 着色。

use lumark_scene::{MaterialData, MeshData, Scene, TextureData};

use crate::RenderSettings;
use crate::camera::{Camera, cross, dot, normalize, sub};

/// 近裁面阈值（裁剪空间 w）。
const NEAR_EPSILON: f32 = 1.0e-4;

/// 渲染到 RGBA8。
pub(crate) fn render(
    scene: &Scene,
    settings: &RenderSettings,
    camera: &Camera,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let pixel_count = width as usize * height as usize;
    let mut frame = Frame {
        width,
        height,
        color: vec![[0.0f32; 3]; pixel_count],
        depth: vec![f32::INFINITY; pixel_count],
    };
    // 背景：自上而下略微变亮的渐变。
    let background = settings.background;
    for y in 0..height {
        let ratio = 1.0 - y as f32 / height.max(1) as f32;
        let color = [
            background[0] * (0.75 + 0.5 * ratio),
            background[1] * (0.75 + 0.5 * ratio),
            background[2] * (0.75 + 0.5 * ratio),
        ];
        let start = y as usize * width as usize;
        for pixel in frame.color[start..start + width as usize].iter_mut() {
            *pixel = color;
        }
    }

    let key = normalize(settings.key_direction);
    let lights = [
        (key, settings.key_color, settings.key_intensity),
        (
            [-key[0], -key[1], -key[2]],
            [0.55, 0.62, 0.75],
            settings.fill_intensity,
        ),
    ];
    let default_material = MaterialData::default();

    for (node_index, node) in scene.nodes.iter().enumerate() {
        if !node.visible {
            continue;
        }
        let Some(mesh_handle) = node.mesh else {
            continue;
        };
        let Some(mesh) = scene.meshes.get(mesh_handle.0) else {
            continue;
        };
        let Some(world) = scene.world_transform(lumark_scene::SceneNodeId(node_index)) else {
            continue;
        };
        let vertices = world_vertices(mesh, &world);
        if vertices.is_empty() {
            continue;
        }
        let ranges = section_ranges(mesh);
        for (first, count, material_index) in ranges {
            let material = node
                .materials
                .get(material_index)
                .copied()
                .flatten()
                .and_then(|handle| scene.materials.get(handle.0))
                .unwrap_or(&default_material);
            let end = (first + count).min(mesh.indices.len());
            let mut index = first;
            while index + 3 <= end {
                let i0 = mesh.indices[index] as usize;
                let i1 = mesh.indices[index + 1] as usize;
                let i2 = mesh.indices[index + 2] as usize;
                if i0 < vertices.len() && i1 < vertices.len() && i2 < vertices.len() {
                    let triangle = [vertices[i0], vertices[i1], vertices[i2]];
                    rasterize_triangle(&triangle, material, &lights, settings, camera, &mut frame);
                }
                index += 3;
            }
        }
    }
    frame.to_rgba8()
}

/// 世界空间顶点。
#[derive(Debug, Clone, Copy)]
struct WorldVertex {
    position: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
}

/// 把网格顶点变换到世界空间。
fn world_vertices(mesh: &MeshData, world: &lumark_scene::Transform) -> Vec<WorldVertex> {
    let mut vertices = Vec::with_capacity(mesh.positions.len());
    for (index, position) in mesh.positions.iter().enumerate() {
        let normal = mesh
            .normals
            .get(index)
            .map(|normal| normalize(world.transform_direction(*normal)))
            .unwrap_or([0.0, 0.0, 0.0]);
        let tangent = mesh
            .tangents
            .get(index)
            .map(|tangent| {
                let direction =
                    normalize(world.transform_direction([tangent[0], tangent[1], tangent[2]]));
                [direction[0], direction[1], direction[2], tangent[3]]
            })
            .unwrap_or([1.0, 0.0, 0.0, 1.0]);
        let uv = mesh
            .uvs
            .first()
            .and_then(|channel| channel.get(index))
            .copied()
            .unwrap_or([0.0, 0.0]);
        let color = mesh
            .colors
            .get(index)
            .map(|color| {
                [
                    color[0] as f32 / 255.0,
                    color[1] as f32 / 255.0,
                    color[2] as f32 / 255.0,
                    color[3] as f32 / 255.0,
                ]
            })
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        vertices.push(WorldVertex {
            position: world.transform_point(*position),
            normal,
            tangent,
            uv,
            color,
        });
    }
    vertices
}

/// 返回 `(first_index, index_count, material_index)` 列表。
fn section_ranges(mesh: &MeshData) -> Vec<(usize, usize, usize)> {
    if mesh.sections.is_empty() {
        return vec![(0, mesh.indices.len(), 0)];
    }
    mesh.sections
        .iter()
        .map(|section| {
            (
                section.first_index as usize,
                section.index_count as usize,
                section.material_index,
            )
        })
        .collect()
}

/// 裁剪空间顶点。
#[derive(Debug, Clone, Copy)]
struct ClipVertex {
    clip: [f32; 4],
    world: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
}

impl ClipVertex {
    fn from_world(vertex: &WorldVertex, camera: &Camera) -> Self {
        Self {
            clip: camera.clip_position(vertex.position),
            world: vertex.position,
            normal: vertex.normal,
            tangent: vertex.tangent,
            uv: vertex.uv,
            color: vertex.color,
        }
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            clip: [
                mix(self.clip[0], other.clip[0]),
                mix(self.clip[1], other.clip[1]),
                mix(self.clip[2], other.clip[2]),
                mix(self.clip[3], other.clip[3]),
            ],
            world: [
                mix(self.world[0], other.world[0]),
                mix(self.world[1], other.world[1]),
                mix(self.world[2], other.world[2]),
            ],
            normal: [
                mix(self.normal[0], other.normal[0]),
                mix(self.normal[1], other.normal[1]),
                mix(self.normal[2], other.normal[2]),
            ],
            tangent: [
                mix(self.tangent[0], other.tangent[0]),
                mix(self.tangent[1], other.tangent[1]),
                mix(self.tangent[2], other.tangent[2]),
                self.tangent[3],
            ],
            uv: [mix(self.uv[0], other.uv[0]), mix(self.uv[1], other.uv[1])],
            color: [
                mix(self.color[0], other.color[0]),
                mix(self.color[1], other.color[1]),
                mix(self.color[2], other.color[2]),
                mix(self.color[3], other.color[3]),
            ],
        }
    }
}

/// 对三角形做近裁面裁剪并光栅化。
fn rasterize_triangle(
    triangle: &[WorldVertex; 3],
    material: &MaterialData,
    lights: &[([f32; 3], [f32; 3], f32)],
    settings: &RenderSettings,
    camera: &Camera,
    frame: &mut Frame,
) {
    // 没有法线时用面法线。
    let face_normal = normalize(cross(
        sub(triangle[1].position, triangle[0].position),
        sub(triangle[2].position, triangle[0].position),
    ));
    let mut triangle = *triangle;
    for vertex in &mut triangle {
        if dot(vertex.normal, vertex.normal) <= 1.0e-8 {
            vertex.normal = face_normal;
        }
    }
    let clip = [
        ClipVertex::from_world(&triangle[0], camera),
        ClipVertex::from_world(&triangle[1], camera),
        ClipVertex::from_world(&triangle[2], camera),
    ];
    for polygon in clip_near(&clip) {
        project_and_raster(&polygon, material, lights, settings, camera, frame);
    }
}

/// 逐个裁剪近裁面，返回 0–2 个三角形。
fn clip_near(triangle: &[ClipVertex; 3]) -> Vec<[ClipVertex; 3]> {
    let mut polygon: Vec<ClipVertex> = triangle.to_vec();
    let mut output: Vec<ClipVertex> = Vec::with_capacity(4);
    for index in 0..polygon.len() {
        let current = polygon[index];
        let next = polygon[(index + 1) % polygon.len()];
        let current_inside = current.clip[3] >= NEAR_EPSILON;
        let next_inside = next.clip[3] >= NEAR_EPSILON;
        if current_inside {
            output.push(current);
        }
        if current_inside != next_inside {
            let t = (NEAR_EPSILON - current.clip[3]) / (next.clip[3] - current.clip[3]);
            output.push(current.lerp(&next, t.clamp(0.0, 1.0)));
        }
    }
    polygon = output;
    if polygon.len() < 3 {
        return Vec::new();
    }
    if polygon.len() == 3 {
        return vec![[polygon[0], polygon[1], polygon[2]]];
    }
    let mut triangles = Vec::with_capacity(polygon.len() - 2);
    for index in 1..polygon.len() - 1 {
        triangles.push([polygon[0], polygon[index], polygon[index + 1]]);
    }
    triangles
}

/// 投影并光栅化一个裁剪后的三角形。
fn project_and_raster(
    triangle: &[ClipVertex; 3],
    material: &MaterialData,
    lights: &[([f32; 3], [f32; 3], f32)],
    settings: &RenderSettings,
    camera: &Camera,
    frame: &mut Frame,
) {
    let mut projected = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 3];
    for (index, vertex) in triangle.iter().enumerate() {
        let w = vertex.clip[3];
        if w <= NEAR_EPSILON {
            return;
        }
        let inv_w = 1.0 / w;
        let ndc_x = vertex.clip[0] * inv_w;
        let ndc_y = vertex.clip[1] * inv_w;
        let ndc_z = vertex.clip[2] * inv_w;
        if !ndc_x.is_finite() || !ndc_y.is_finite() || !ndc_z.is_finite() {
            return;
        }
        let screen_x = (ndc_x * 0.5 + 0.5) * frame.width as f32;
        let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * frame.height as f32;
        projected[index] = (screen_x, screen_y, ndc_z, inv_w);
    }
    let area = edge(projected[0], projected[1], projected[2]);
    if area.abs() <= 1.0e-7 {
        return;
    }
    let min_x = projected
        .iter()
        .map(|vertex| vertex.0)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_x = projected
        .iter()
        .map(|vertex| vertex.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(frame.width as f32 - 1.0) as u32;
    let min_y = projected
        .iter()
        .map(|vertex| vertex.1)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_y = projected
        .iter()
        .map(|vertex| vertex.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(frame.height as f32 - 1.0) as u32;
    if min_x > max_x || min_y > max_y {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let point = (x as f32 + 0.5, y as f32 + 0.5);
            let (w0, w1, w2) = barycentric(projected, point, area);
            if w0 < -1.0e-5 || w1 < -1.0e-5 || w2 < -1.0e-5 {
                continue;
            }
            let inv_w = w0 * projected[0].3 + w1 * projected[1].3 + w2 * projected[2].3;
            if inv_w <= 0.0 {
                continue;
            }
            let depth = w0 * projected[0].2 + w1 * projected[1].2 + w2 * projected[2].2;
            if !(0.0..=1.0).contains(&depth) {
                continue;
            }
            let offset = y as usize * frame.width as usize + x as usize;
            if depth >= frame.depth[offset] {
                continue;
            }
            let interpolate = |values: [f32; 3]| {
                [(w0 * values[0] * projected[0].3
                    + w1 * values[1] * projected[1].3
                    + w2 * values[2] * projected[2].3)
                    / inv_w]
            };
            let world = [
                interpolate([
                    triangle[0].world[0],
                    triangle[1].world[0],
                    triangle[2].world[0],
                ])[0],
                interpolate([
                    triangle[0].world[1],
                    triangle[1].world[1],
                    triangle[2].world[1],
                ])[0],
                interpolate([
                    triangle[0].world[2],
                    triangle[1].world[2],
                    triangle[2].world[2],
                ])[0],
            ];
            let normal = normalize([
                interpolate([
                    triangle[0].normal[0],
                    triangle[1].normal[0],
                    triangle[2].normal[0],
                ])[0],
                interpolate([
                    triangle[0].normal[1],
                    triangle[1].normal[1],
                    triangle[2].normal[1],
                ])[0],
                interpolate([
                    triangle[0].normal[2],
                    triangle[1].normal[2],
                    triangle[2].normal[2],
                ])[0],
            ]);
            let tangent = [
                interpolate([
                    triangle[0].tangent[0],
                    triangle[1].tangent[0],
                    triangle[2].tangent[0],
                ])[0],
                interpolate([
                    triangle[0].tangent[1],
                    triangle[1].tangent[1],
                    triangle[2].tangent[1],
                ])[0],
                interpolate([
                    triangle[0].tangent[2],
                    triangle[1].tangent[2],
                    triangle[2].tangent[2],
                ])[0],
                triangle[0].tangent[3],
            ];
            let uv = [
                interpolate([triangle[0].uv[0], triangle[1].uv[0], triangle[2].uv[0]])[0],
                interpolate([triangle[0].uv[1], triangle[1].uv[1], triangle[2].uv[1]])[0],
            ];
            let color = [
                interpolate([
                    triangle[0].color[0],
                    triangle[1].color[0],
                    triangle[2].color[0],
                ])[0],
                interpolate([
                    triangle[0].color[1],
                    triangle[1].color[1],
                    triangle[2].color[1],
                ])[0],
                interpolate([
                    triangle[0].color[2],
                    triangle[1].color[2],
                    triangle[2].color[2],
                ])[0],
                interpolate([
                    triangle[0].color[3],
                    triangle[1].color[3],
                    triangle[2].color[3],
                ])[0],
            ];
            let surface = Surface {
                world,
                normal,
                tangent,
                uv,
                color,
            };
            frame.color[offset] = shade(&surface, material, lights, settings, camera);
            frame.depth[offset] = depth;
        }
    }
}

/// 屏幕空间边的有向面积。
fn edge(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32), c: (f32, f32, f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// 重心坐标。
fn barycentric(
    projected: [(f32, f32, f32, f32); 3],
    point: (f32, f32),
    area: f32,
) -> (f32, f32, f32) {
    let p = (point.0, point.1, 0.0, 1.0);
    let w0 = edge(projected[1], projected[2], p) / area;
    let w1 = edge(projected[2], projected[0], p) / area;
    let w2 = edge(projected[0], projected[1], p) / area;
    (w0, w1, w2)
}

/// 像素级着色需要的数据。
struct Surface {
    world: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
}

/// 近似 PBR 着色，返回线性辐射度。
fn shade(
    surface: &Surface,
    material: &MaterialData,
    lights: &[([f32; 3], [f32; 3], f32)],
    settings: &RenderSettings,
    camera: &Camera,
) -> [f32; 3] {
    let mut base = [
        material.base_color[0],
        material.base_color[1],
        material.base_color[2],
    ];
    if let Some(texture) = material.textures.base_color.as_ref() {
        let sampled = sample_texture(texture, surface.uv);
        base = [
            base[0] * srgb_to_linear(sampled[0]),
            base[1] * srgb_to_linear(sampled[1]),
            base[2] * srgb_to_linear(sampled[2]),
        ];
    }
    base = [
        base[0] * surface.color[0],
        base[1] * surface.color[1],
        base[2] * surface.color[2],
    ];
    let mut normal = normalize(surface.normal);
    if let Some(texture) = material.textures.normal.as_ref() {
        let sampled = sample_texture(texture, surface.uv);
        let tangent_normal = [
            sampled[0] * 2.0 - 1.0,
            sampled[1] * 2.0 - 1.0,
            sampled[2] * 2.0 - 1.0,
        ];
        let tangent_dot = dot(
            normal,
            [surface.tangent[0], surface.tangent[1], surface.tangent[2]],
        );
        let tangent = normalize([
            surface.tangent[0] - normal[0] * tangent_dot,
            surface.tangent[1] - normal[1] * tangent_dot,
            surface.tangent[2] - normal[2] * tangent_dot,
        ]);
        let bitangent = cross(normal, tangent);
        let scale = surface.tangent[3].signum();
        normal = normalize([
            tangent[0] * tangent_normal[0]
                + bitangent[0] * tangent_normal[1] * scale
                + normal[0] * tangent_normal[2],
            tangent[1] * tangent_normal[0]
                + bitangent[1] * tangent_normal[1] * scale
                + normal[1] * tangent_normal[2],
            tangent[2] * tangent_normal[0]
                + bitangent[2] * tangent_normal[1] * scale
                + normal[2] * tangent_normal[2],
        ]);
    }
    let view = normalize(sub(camera.eye, surface.world));
    if dot(normal, view) < 0.0 {
        normal = [-normal[0], -normal[1], -normal[2]];
    }
    let metallic = material.metallic.clamp(0.0, 1.0);
    let roughness = material.roughness.clamp(0.04, 1.0);
    let shininess = (2.0 / (roughness * roughness * roughness * roughness)).clamp(4.0, 2048.0);
    let ambient = settings.ambient_strength;
    let mut color = [base[0] * ambient, base[1] * ambient, base[2] * ambient];
    for (direction, light_color, intensity) in lights {
        let light = normalize(*direction);
        let ndl = dot(normal, light).max(0.0);
        if ndl <= 0.0 {
            continue;
        }
        let half_vector = normalize([light[0] + view[0], light[1] + view[1], light[2] + view[2]]);
        let ndh = dot(normal, half_vector).max(0.0);
        let specular = ndh.powf(shininess);
        let specular_strength = 0.04 + metallic * 0.5;
        let diffuse = ndl * intensity;
        for channel in 0..3 {
            let light_channel = light_color[channel];
            color[channel] += base[channel] * light_channel * diffuse;
            color[channel] += light_channel * specular * specular_strength * intensity;
        }
    }
    let emissive = material.emissive;
    [
        color[0] + emissive[0],
        color[1] + emissive[1],
        color[2] + emissive[2],
    ]
}

/// sRGB 分量转线性。
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// 纹理采样（repeat 环绕 + 双线性过滤）。
pub(crate) fn sample_texture(texture: &TextureData, uv: [f32; 2]) -> [f32; 4] {
    if !texture.is_valid() {
        return [1.0; 4];
    }
    let width = texture.width as f32;
    let height = texture.height as f32;
    let u = uv[0].rem_euclid(1.0) * width - 0.5;
    let v = uv[1].rem_euclid(1.0) * height - 0.5;
    let x0 = u.floor();
    let y0 = v.floor();
    let fx = u - x0;
    let fy = v - y0;
    let sample = |x: f32, y: f32| {
        let x = x.rem_euclid(width) as usize;
        let y = y.rem_euclid(height) as usize;
        let offset = (y * texture.width as usize + x) * 4;
        let pixel = &texture.rgba[offset..offset + 4];
        [
            pixel[0] as f32 / 255.0,
            pixel[1] as f32 / 255.0,
            pixel[2] as f32 / 255.0,
            pixel[3] as f32 / 255.0,
        ]
    };
    let top_left = sample(x0, y0);
    let top_right = sample(x0 + 1.0, y0);
    let bottom_left = sample(x0, y0 + 1.0);
    let bottom_right = sample(x0 + 1.0, y0 + 1.0);
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let mut result = [0.0f32; 4];
    for channel in 0..4 {
        let top = mix(top_left[channel], top_right[channel], fx);
        let bottom = mix(bottom_left[channel], bottom_right[channel], fx);
        result[channel] = mix(top, bottom, fy);
    }
    result
}

/// 帧缓冲。
struct Frame {
    width: u32,
    height: u32,
    color: Vec<[f32; 3]>,
    depth: Vec<f32>,
}

impl Frame {
    /// 色调映射 + gamma 编码为 RGBA8。
    fn to_rgba8(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.color.len() * 4);
        for color in &self.color {
            let mapped = [
                color[0].max(0.0) / (1.0 + color[0].max(0.0)),
                color[1].max(0.0) / (1.0 + color[1].max(0.0)),
                color[2].max(0.0) / (1.0 + color[2].max(0.0)),
            ];
            output.push((mapped[0].powf(1.0 / 2.2).clamp(0.0, 1.0) * 255.0) as u8);
            output.push((mapped[1].powf(1.0 / 2.2).clamp(0.0, 1.0) * 255.0) as u8);
            output.push((mapped[2].powf(1.0 / 2.2).clamp(0.0, 1.0) * 255.0) as u8);
            output.push(255);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderSettings;
    use lumark_scene::{MeshSection, NodeKind, Scene, SceneNode, Transform};

    fn triangle_scene(color: [f32; 4], z: f32) -> Scene {
        let mut scene = Scene::new("Test");
        let material = scene.add_material(MaterialData {
            name: "M".to_string(),
            base_color: color,
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
            textures: Default::default(),
            two_sided: false,
        });
        let mut mesh = MeshData::new(
            "Tri",
            vec![[-100.0, 0.0, z], [100.0, 0.0, z], [0.0, 100.0, z]],
            vec![0, 1, 2],
        );
        mesh.sections = vec![MeshSection {
            material_index: 0,
            first_index: 0,
            index_count: 3,
        }];
        let mesh = scene.add_mesh(mesh);
        scene.add_node(SceneNode {
            name: "Tri".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh),
            materials: vec![Some(material)],
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        scene
    }

    fn render(scene: &Scene, width: u32) -> Vec<u8> {
        let settings = RenderSettings {
            width,
            height: width,
            supersample: 1,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        crate::render_scene(scene, &settings).expect("渲染").rgba
    }

    /// 红色材质三角形应该出现更多红色像素。
    #[test]
    fn material_color_tints_triangle() {
        let rgba = render(&triangle_scene([1.0, 0.0, 0.0, 1.0], 0.0), 96);
        let mut red_pixels = 0;
        for pixel in rgba.as_chunks::<4>().0 {
            if pixel[0] > pixel[1].saturating_add(pixel[2]) && pixel[0] > 10 {
                red_pixels += 1;
            }
        }
        assert!(red_pixels > 0, "红色三角形未产生红色像素");
    }

    /// 近处三角形遮挡远处三角形。
    #[test]
    fn near_triangle_occludes_far() {
        let mut scene = Scene::new("Depth");
        let red = scene.add_material(MaterialData {
            name: "Red".to_string(),
            base_color: [1.0, 0.0, 0.0, 1.0],
            ..MaterialData::default()
        });
        let blue = scene.add_material(MaterialData {
            name: "Blue".to_string(),
            base_color: [0.0, 0.0, 1.0, 1.0],
            ..MaterialData::default()
        });
        let near_mesh = scene.add_mesh(MeshData::new(
            "Near",
            vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 100.0]],
            vec![0, 1, 2],
        ));
        let far_mesh = scene.add_mesh(MeshData::new(
            "Far",
            vec![
                [-100.0, -300.0, 0.0],
                [100.0, -300.0, 0.0],
                [0.0, -300.0, 100.0],
            ],
            vec![0, 1, 2],
        ));
        for (mesh, material) in [(near_mesh, red), (far_mesh, blue)] {
            scene.add_node(SceneNode {
                name: format!("Node{}", scene.nodes.len()),
                parent: None,
                transform: Transform::IDENTITY,
                mesh: Some(mesh),
                materials: vec![Some(material)],
                visible: true,
                kind: NodeKind::StaticMesh,
                metadata: Vec::new(),
            });
        }
        // 从 +Y 方向观察：两个三角形屏幕投影完全重合，只差 Y 深度。
        let settings = RenderSettings {
            width: 96,
            height: 96,
            supersample: 1,
            yaw_degrees: 90.0,
            pitch_degrees: 0.0,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        let rgba = crate::render_scene(&scene, &settings).expect("渲染").rgba;
        let mut red = 0;
        let mut blue = 0;
        for pixel in rgba.as_chunks::<4>().0 {
            if pixel[0] > pixel[2].saturating_add(10) {
                red += 1;
            } else if pixel[2] > pixel[0].saturating_add(10) {
                blue += 1;
            }
        }
        assert!(red > 0, "近处红色三角形不可见");
        assert!(blue == 0, "远处蓝色三角形不应可见：red={red} blue={blue}");
    }

    /// 基础色贴图参与着色。
    #[test]
    fn samples_base_color_texture() {
        let mut scene = triangle_scene([1.0, 1.0, 1.0, 1.0], 0.0);
        scene.materials[0].textures.base_color = Some(TextureData {
            name: "T_Red".to_string(),
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        });
        let rgba = render(&scene, 96);
        let mut red_pixels = 0;
        for pixel in rgba.as_chunks::<4>().0 {
            if pixel[0] > pixel[1].saturating_add(10) && pixel[0] > 10 {
                red_pixels += 1;
            }
        }
        assert!(red_pixels > 0, "红色基础色贴图未生效");
    }

    /// 纹理采样支持 repeat 与双线性过滤。
    #[test]
    fn samples_texture_with_repeat() {
        let texture = TextureData {
            name: "T".to_string(),
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        };
        let sampled = sample_texture(&texture, [0.3, 0.7]);
        assert!((sampled[0] - 1.0).abs() < 1e-5);
        assert!(sampled[1].abs() < 1e-5);
        let invalid = TextureData {
            name: "Empty".to_string(),
            width: 0,
            height: 0,
            rgba: Vec::new(),
        };
        assert_eq!(sample_texture(&invalid, [0.0, 0.0]), [1.0; 4]);
    }

    /// 近裁面裁剪保留完全在正面/跨越平面的三角形。
    #[test]
    fn clips_near_plane() {
        let camera = Camera::fit(&lumark_scene::Bounds::EMPTY, &RenderSettings::default());
        let vertex = |world: [f32; 3]| ClipVertex {
            clip: camera.clip_position(world),
            world,
            normal: [0.0, 0.0, 1.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
            color: [1.0; 4],
        };
        let front = [
            vertex([-10.0, 0.0, 0.0]),
            vertex([10.0, 0.0, 0.0]),
            vertex([0.0, 10.0, 0.0]),
        ];
        assert_eq!(clip_near(&front).len(), 1);
        let behind = [
            ClipVertex {
                clip: [-1.0, 0.0, 0.0, -1.0],
                ..front[0]
            },
            ClipVertex {
                clip: [-1.0, 1.0, 0.0, -1.0],
                ..front[1]
            },
            ClipVertex {
                clip: [-1.0, 0.0, 1.0, -1.0],
                ..front[2]
            },
        ];
        assert!(clip_near(&behind).is_empty());
    }
}
