//! glTF 2.0 binary（GLB）导出。
//!
//! 输出单个 `.glb`：内嵌 PNG 贴图、PBR 材质、section 图元；骨骼网格输出
//! `skin`、关节节点层级与 inverse bind matrices。坐标从 UE 的 Z-up 转成
//! glTF 的 Y-up：`(x, y, z) → (x, z, -y)`，单位由 `ExportOptions::scale`
//! 控制（UE 厘米 → 米用 0.01）。

use std::f32::consts::FRAC_1_SQRT_2;

use lumark_scene::Transform;
use lumark_scene::transform::quat_mul;
use serde_json::{Value, json};

use crate::{ExportBone, ExportError, ExportMesh, ExportOptions};

/// 内嵌贴图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportTexture {
    /// 贴图名。
    pub name: String,
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// RGBA8 像素。
    pub rgba: Vec<u8>,
}

/// 导出材质。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMaterial {
    /// 材质名。
    pub name: String,
    /// 基础色（线性 RGBA）。
    pub base_color: [f32; 4],
    /// 金属度。
    pub metallic: f32,
    /// 粗糙度。
    pub roughness: f32,
    /// 自发光（线性 RGB）。
    pub emissive: [f32; 3],
    /// 基础色贴图。
    pub base_color_texture: Option<ExportTexture>,
    /// 法线贴图。
    pub normal_texture: Option<ExportTexture>,
    /// 是否双面。
    pub two_sided: bool,
}

impl Default for ExportMaterial {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            base_color: [0.72, 0.72, 0.72, 1.0],
            metallic: 0.0,
            roughness: 0.55,
            emissive: [0.0; 3],
            base_color_texture: None,
            normal_texture: None,
            two_sided: false,
        }
    }
}

/// 一次 glTF 导出的输入。
#[derive(Debug, Clone, Default)]
pub struct GltfExport {
    /// 网格。
    pub mesh: ExportMesh,
    /// 材质（按 section 的 `material_index` 索引）。
    pub materials: Vec<ExportMaterial>,
}

impl GltfExport {
    /// 由网格和材质构造。
    pub fn new(mesh: ExportMesh, materials: Vec<ExportMaterial>) -> Self {
        Self { mesh, materials }
    }
}

/// 写出 GLB。
pub fn write_glb(export: &GltfExport, options: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    crate::actorx::validate(&export.mesh)?;
    let mesh = &export.mesh;
    let scale = options.scale.max(f32::MIN_POSITIVE);
    let mut builder = Builder::default();

    // 顶点属性：positions/normals/uvs/colors/tangents/joints/weights。
    let positions: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .map(|vertex| convert_point(vertex.position, scale))
        .collect();
    let mut position_min = [f64::MAX; 3];
    let mut position_max = [f64::MIN; 3];
    let mut position_bytes = Vec::with_capacity(positions.len() * 12);
    for position in &positions {
        for (axis, value) in position.iter().enumerate() {
            position_min[axis] = position_min[axis].min(f64::from(*value));
            position_max[axis] = position_max[axis].max(f64::from(*value));
        }
        for value in position {
            position_bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let position_view = builder.add_view(&position_bytes, Some(34_962));
    let position_accessor = builder.add_accessor(
        position_view,
        5126,
        positions.len(),
        "VEC3",
        false,
        Some(vec![position_min.to_vec(), position_max.to_vec()]),
    );

    let mut normal_bytes = Vec::with_capacity(mesh.vertices.len() * 12);
    for vertex in &mesh.vertices {
        let normal = convert_vector(vertex.normal);
        for value in normal {
            normal_bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let normal_view = builder.add_view(&normal_bytes, Some(34_962));
    let normal_accessor =
        builder.add_accessor(normal_view, 5126, mesh.vertices.len(), "VEC3", false, None);

    let mut attributes = json!({
        "POSITION": position_accessor,
        "NORMAL": normal_accessor,
    });

    // TEXCOORD_0 + 额外通道。
    let mut uv_channels: Vec<&Vec<[f32; 2]>> = Vec::new();
    let main_uvs: Vec<[f32; 2]> = mesh.vertices.iter().map(|vertex| vertex.uv).collect();
    uv_channels.push(&main_uvs);
    if options.extra_uvs {
        for channel in &mesh.extra_uvs {
            uv_channels.push(channel);
        }
    }
    for (channel_index, uvs) in uv_channels.iter().enumerate() {
        if uvs.len() != mesh.vertices.len() {
            continue;
        }
        let mut bytes = Vec::with_capacity(uvs.len() * 8);
        for uv in uvs.iter() {
            bytes.extend_from_slice(&uv[0].to_le_bytes());
            bytes.extend_from_slice(&uv[1].to_le_bytes());
        }
        let view = builder.add_view(&bytes, Some(34_962));
        let accessor = builder.add_accessor(view, 5126, uvs.len(), "VEC2", false, None);
        attributes[format!("TEXCOORD_{channel_index}")] = json!(accessor);
    }

    if options.vertex_colors && !mesh.vertices.is_empty() {
        let mut bytes = Vec::with_capacity(mesh.vertices.len() * 4);
        for vertex in &mesh.vertices {
            bytes.extend_from_slice(&vertex.color);
        }
        let view = builder.add_view(&bytes, Some(34_962));
        let accessor = builder.add_accessor(view, 5121, mesh.vertices.len(), "VEC4", true, None);
        attributes["COLOR_0"] = json!(accessor);
    }

    let skinned = mesh.is_skinned();
    if skinned {
        let mut joint_bytes = Vec::with_capacity(mesh.vertices.len() * 8);
        let mut weight_bytes = Vec::with_capacity(mesh.vertices.len() * 16);
        for vertex in 0..mesh.vertices.len() {
            let mut indices = mesh.bone_indices[vertex];
            let mut weights = mesh.bone_weights[vertex];
            let mut total: f32 = weights.iter().sum();
            if total <= 0.0 {
                indices = [0; 4];
                weights = [1.0, 0.0, 0.0, 0.0];
                total = 1.0;
            }
            for joint in indices {
                joint_bytes.extend_from_slice(&joint.to_le_bytes());
            }
            for weight in weights {
                weight_bytes.extend_from_slice(&(weight / total).to_le_bytes());
            }
        }
        let joint_view = builder.add_view(&joint_bytes, Some(34_962));
        let joint_accessor =
            builder.add_accessor(joint_view, 5123, mesh.vertices.len(), "VEC4", false, None);
        let weight_view = builder.add_view(&weight_bytes, Some(34_962));
        let weight_accessor =
            builder.add_accessor(weight_view, 5126, mesh.vertices.len(), "VEC4", false, None);
        attributes["JOINTS_0"] = json!(joint_accessor);
        attributes["WEIGHTS_0"] = json!(weight_accessor);
    }

    // 索引：每个 section 一个 primitive。
    let sections: Vec<crate::MeshSection> = if mesh.sections.is_empty() {
        vec![crate::MeshSection {
            material_index: 0,
            first_index: 0,
            triangle_count: mesh.triangle_count(),
        }]
    } else {
        mesh.sections.clone()
    };
    let mut primitives = Vec::with_capacity(sections.len());
    for section in &sections {
        let start = section.first_index;
        let end = (start + section.triangle_count * 3).min(mesh.indices.len());
        let indices = &mesh.indices[start..end];
        let mut bytes = Vec::new();
        let width_16 = mesh.vertices.len() <= u16::MAX as usize;
        for index in indices {
            if width_16 {
                bytes.extend_from_slice(&(*index as u16).to_le_bytes());
            } else {
                bytes.extend_from_slice(&index.to_le_bytes());
            }
        }
        let view = builder.add_view(&bytes, Some(34_963));
        let accessor = builder.add_accessor(
            view,
            if width_16 { 5123 } else { 5125 },
            indices.len(),
            "SCALAR",
            false,
            None,
        );
        let material = if export.materials.is_empty() {
            None
        } else {
            Some(section.material_index.min(export.materials.len() - 1))
        };
        let mut primitive = json!({
            "attributes": attributes,
            "indices": accessor,
            "mode": 4,
        });
        if let Some(material) = material {
            primitive["material"] = json!(material);
        }
        primitives.push(primitive);
    }

    // 材质与内嵌贴图。
    let materials_json = build_materials(&mut builder, &export.materials)?;
    let (materials_json, images_json, textures_json, samplers_json) = (
        materials_json.materials,
        materials_json.images,
        materials_json.textures,
        materials_json.samplers,
    );

    // 节点：骨骼节点 + 网格节点。
    let bone_count = mesh.bones.len();
    let mesh_node = bone_count;
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); bone_count];
    let mut root_bones = Vec::new();
    let mut node_json = Vec::with_capacity(bone_count + 1);
    let mut world_transforms: Vec<[f32; 16]> = Vec::with_capacity(bone_count);
    let converted_bones: Vec<Transform> = mesh
        .bones
        .iter()
        .map(|bone| convert_bone_transform(bone, scale))
        .collect();
    for (index, bone) in mesh.bones.iter().enumerate() {
        let local = converted_bones[index];
        let local_matrix = local.matrix();
        let world = if bone.parent_index >= 0 {
            let parent = bone.parent_index as usize;
            let parent_matrix = unflatten_matrix(&world_transforms[parent]);
            multiply_mat4(&parent_matrix, &local_matrix)
        } else {
            local_matrix
        };
        let flat = flatten_matrix(&world);
        world_transforms.push(flat);
        node_json.push(json!({
            "name": bone.name,
            "matrix": flat.iter().map(|value| f64::from(*value)).collect::<Vec<_>>(),
        }));
        if bone.parent_index >= 0 {
            children[bone.parent_index as usize].push(index);
        } else {
            root_bones.push(index);
        }
    }
    for (index, child_nodes) in children.iter().enumerate() {
        if !child_nodes.is_empty() {
            node_json[index]["children"] = json!(child_nodes);
        }
    }
    let mesh_matrix = Transform::IDENTITY.matrix();
    let mut mesh_node_json = json!({
        "name": mesh.name,
        "mesh": 0,
        "matrix": flatten_matrix(&mesh_matrix).iter().map(|value| f64::from(*value)).collect::<Vec<_>>(),
    });
    let mut skin_json = Value::Null;
    if skinned {
        let mut inverse_bytes = Vec::with_capacity(bone_count * 64);
        for flat in &world_transforms {
            let inverse = invert4(flat);
            for value in inverse {
                inverse_bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let view = builder.add_view(&inverse_bytes, None);
        let accessor = builder.add_accessor(view, 5126, bone_count, "MAT4", false, None);
        let joints: Vec<usize> = (0..bone_count).collect();
        let skeleton_root = root_bones.first().copied().unwrap_or(0);
        skin_json = json!({
            "joints": joints,
            "inverseBindMatrices": accessor,
            "skeleton": skeleton_root,
            "name": format!("{}_Skin", mesh.name),
        });
        mesh_node_json["skin"] = json!(0);
    }
    node_json.push(mesh_node_json);
    let mut scene_nodes: Vec<usize> = root_bones;
    scene_nodes.push(mesh_node);

    // 二进制数据先补齐 4 字节，保证 buffer.byteLength 与实际一致。
    while !builder.bin.len().is_multiple_of(4) {
        builder.bin.push(0);
    }
    let bin_length = builder.bin.len().max(1);
    let mut json_root = json!({
        "asset": {
            "version": "2.0",
            "generator": "Lumark",
        },
        "scene": 0,
        "scenes": [{"nodes": scene_nodes}],
        "nodes": node_json,
        "meshes": [{"name": mesh.name, "primitives": primitives}],
        "accessors": builder.accessors,
        "bufferViews": builder.views,
        "buffers": [{"byteLength": bin_length}],
    });
    if !materials_json.is_empty() {
        json_root["materials"] = json!(materials_json);
    }
    if !textures_json.is_empty() {
        json_root["textures"] = json!(textures_json);
        json_root["images"] = json!(images_json);
        json_root["samplers"] = json!(samplers_json);
    }
    if !skin_json.is_null() {
        json_root["skins"] = json!([skin_json]);
    }

    let mut json_bytes = serde_json::to_vec(&json_root)
        .map_err(|error| ExportError::Serialization(error.to_string()))?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let bin = builder.bin;
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

/// glTF 构建器。
#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn add_view(&mut self, bytes: &[u8], target: Option<u64>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let mut view = json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": bytes.len(),
        });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.views.push(view);
        self.views.len() - 1
    }

    fn add_accessor(
        &mut self,
        view: usize,
        component_type: u32,
        count: usize,
        kind: &str,
        normalized: bool,
        min_max: Option<Vec<Vec<f64>>>,
    ) -> usize {
        let mut accessor = json!({
            "bufferView": view,
            "componentType": component_type,
            "count": count,
            "type": kind,
        });
        if normalized {
            accessor["normalized"] = json!(true);
        }
        if let Some(min_max) = min_max {
            accessor["min"] = json!(min_max[0]);
            accessor["max"] = json!(min_max[1]);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }
}

/// 材质构建结果。
struct MaterialJson {
    materials: Vec<Value>,
    images: Vec<Value>,
    textures: Vec<Value>,
    samplers: Vec<Value>,
}

/// 构造材质、图像、纹理与采样器。
fn build_materials(
    builder: &mut Builder,
    materials: &[ExportMaterial],
) -> Result<MaterialJson, ExportError> {
    let mut output = Vec::with_capacity(materials.len());
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let samplers = vec![json!({
        "magFilter": 9729,
        "minFilter": 9987,
        "wrapS": 10497,
        "wrapT": 10497,
    })];
    for material in materials {
        let mut pbr = json!({
            "baseColorFactor": material.base_color.iter().map(|value| f64::from(*value)).collect::<Vec<_>>(),
            "metallicFactor": f64::from(material.metallic.clamp(0.0, 1.0)),
            "roughnessFactor": f64::from(material.roughness.clamp(0.04, 1.0)),
        });
        if let Some(texture) = &material.base_color_texture {
            let index = add_texture(builder, &mut images, &mut textures, texture)?;
            pbr["baseColorTexture"] = json!({"index": index, "texCoord": 0});
        }
        let mut output_material = json!({
            "name": material.name,
            "pbrMetallicRoughness": pbr,
            "emissiveFactor": material.emissive.iter().map(|value| f64::from(*value)).collect::<Vec<_>>(),
            "doubleSided": material.two_sided,
        });
        if let Some(texture) = &material.normal_texture {
            let index = add_texture(builder, &mut images, &mut textures, texture)?;
            output_material["normalTexture"] = json!({"index": index, "texCoord": 0});
        }
        output.push(output_material);
    }
    Ok(MaterialJson {
        materials: output,
        images,
        textures,
        samplers,
    })
}

/// 追加 PNG 图像与纹理，返回纹理索引。
fn add_texture(
    builder: &mut Builder,
    images: &mut Vec<Value>,
    textures: &mut Vec<Value>,
    texture: &ExportTexture,
) -> Result<usize, ExportError> {
    let png = encode_png(texture)?;
    let view = builder.add_view(&png, None);
    images.push(json!({
        "name": texture.name,
        "bufferView": view,
        "mimeType": "image/png",
    }));
    textures.push(json!({
        "sampler": 0,
        "source": images.len() - 1,
    }));
    Ok(textures.len() - 1)
}

/// RGBA8 → PNG。
fn encode_png(texture: &ExportTexture) -> Result<Vec<u8>, ExportError> {
    use image::{ExtendedColorType, ImageEncoder};
    let buffer = image::RgbaImage::from_raw(texture.width, texture.height, texture.rgba.clone())
        .ok_or_else(|| ExportError::InvalidTexture(texture.name.clone()))?;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            buffer.as_raw(),
            texture.width,
            texture.height,
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| ExportError::Serialization(error.to_string()))?;
    Ok(out)
}

/// UE Z-up → glTF Y-up。
fn convert_point(point: [f32; 3], scale: f32) -> [f32; 3] {
    [point[0] * scale, point[2] * scale, -point[1] * scale]
}

/// 方向向量转换。
fn convert_vector(vector: [f32; 3]) -> [f32; 3] {
    [vector[0], vector[2], -vector[1]]
}

/// 骨骼局部变换转换到 glTF 空间。
fn convert_bone_transform(bone: &ExportBone, scale: f32) -> Transform {
    let conversion = [-FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2];
    let conversion_inverse = [FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2];
    let rotation = quat_mul(quat_mul(conversion, bone.rotation), conversion_inverse);
    Transform {
        translation: convert_point(bone.position, scale),
        rotation,
        scale: [1.0; 3],
    }
}

/// 列主序展平数组还原为矩阵。
fn unflatten_matrix(flat: &[f32; 16]) -> [[f32; 4]; 4] {
    let mut matrix = [[0.0f32; 4]; 4];
    for column in 0..4 {
        matrix[column].copy_from_slice(&flat[column * 4..column * 4 + 4]);
    }
    matrix
}

/// 列主序 4×4 乘法。
fn multiply_mat4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0f32; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            let mut value = 0.0f32;
            for index in 0..4 {
                value += a[index][row] * b[column][index];
            }
            result[column][row] = value;
        }
    }
    result
}

/// 列主序 4×4 展平。
fn flatten_matrix(matrix: &[[f32; 4]; 4]) -> [f32; 16] {
    let mut flat = [0.0f32; 16];
    for column in 0..4 {
        flat[column * 4..column * 4 + 4].copy_from_slice(&matrix[column]);
    }
    flat
}

/// 列主序 4×4 求逆。
fn invert4(input: &[f32; 16]) -> [f32; 16] {
    let m: Vec<f64> = input.iter().map(|value| f64::from(*value)).collect();
    let mut inverse = [0.0f64; 16];
    inverse[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inverse[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inverse[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inverse[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inverse[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inverse[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inverse[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inverse[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inverse[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inverse[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inverse[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inverse[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inverse[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inverse[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inverse[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inverse[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let determinant =
        m[0] * inverse[0] + m[1] * inverse[4] + m[2] * inverse[8] + m[3] * inverse[12];
    let scale = if determinant.abs() <= f64::EPSILON {
        0.0
    } else {
        1.0 / determinant
    };
    let mut output = [0.0f32; 16];
    for (index, value) in inverse.iter().enumerate() {
        output[index] = (*value * scale) as f32;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExportBone, ExportMesh, MeshSection, MeshVertex, write_glb};

    /// 构造带法线/UV 的三角形。
    fn triangle() -> ExportMesh {
        ExportMesh {
            name: "SM_Test".to_string(),
            vertices: vec![
                MeshVertex {
                    position: [0.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0, 0.0],
                    color: [255; 4],
                },
                MeshVertex {
                    position: [100.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [1.0, 0.0],
                    color: [255; 4],
                },
                MeshVertex {
                    position: [0.0, 0.0, 100.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0, 1.0],
                    color: [255; 4],
                },
            ],
            indices: vec![0, 1, 2],
            sections: vec![MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count: 1,
            }],
            ..ExportMesh::default()
        }
    }

    /// 解析 GLB 的 JSON chunk。
    fn glb_json(data: &[u8]) -> Value {
        assert_eq!(&data[0..4], b"glTF");
        let json_length = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let json = &data[20..20 + json_length];
        serde_json::from_slice(json).expect("GLB JSON")
    }

    /// 静态三角形：GLB 结构合法、Y-up 转换与缩放正确。
    #[test]
    fn writes_static_glb() {
        let export = GltfExport::new(triangle(), Vec::new());
        let options = ExportOptions {
            scale: 0.01,
            ..ExportOptions::default()
        };
        let glb = write_glb(&export, &options).expect("GLB");
        gltf::Gltf::from_slice(&glb).expect("gltf 校验");
        let json = glb_json(&glb);
        assert_eq!(json["asset"]["version"], "2.0");
        let primitive = &json["meshes"][0]["primitives"][0];
        let position_accessor = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
        let accessor = &json["accessors"][position_accessor];
        assert_eq!(accessor["min"], json!([0.0, 0.0, 0.0]));
        assert_eq!(accessor["max"], json!([1.0, 1.0, 0.0]));
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
    }

    /// 骨骼网格写出 skin、joints 与 inverseBindMatrices。
    #[test]
    fn writes_skinned_glb() {
        let mut mesh = triangle();
        mesh.bones = vec![
            ExportBone {
                name: "Root".to_string(),
                parent_index: -1,
                position: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            ExportBone {
                name: "Child".to_string(),
                parent_index: 0,
                position: [0.0, 0.0, 10.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
        ];
        mesh.bone_indices = vec![[0, 1, 0, 0]; 3];
        mesh.bone_weights = vec![[0.5, 0.5, 0.0, 0.0]; 3];
        let glb = write_glb(
            &GltfExport::new(mesh, Vec::new()),
            &ExportOptions::default(),
        )
        .expect("GLB");
        gltf::Gltf::from_slice(&glb).expect("gltf 校验");
        let json = glb_json(&glb);
        let primitive = &json["meshes"][0]["primitives"][0];
        assert!(primitive["attributes"]["JOINTS_0"].is_number());
        assert!(primitive["attributes"]["WEIGHTS_0"].is_number());
        let skin = &json["skins"][0];
        assert_eq!(skin["joints"], json!([0, 1]));
        let ibm = skin["inverseBindMatrices"].as_u64().unwrap() as usize;
        assert_eq!(json["accessors"][ibm]["type"], "MAT4");
        assert_eq!(json["accessors"][ibm]["count"], 2);
        let mesh_node = &json["nodes"][2];
        assert_eq!(mesh_node["skin"], 0);
    }

    /// 内嵌材质贴图与 PNG 编码。
    #[test]
    fn embeds_material_texture() {
        let material = ExportMaterial {
            name: "M_Test".to_string(),
            base_color: [1.0, 0.0, 0.0, 1.0],
            base_color_texture: Some(ExportTexture {
                name: "T_Red".to_string(),
                width: 1,
                height: 1,
                rgba: vec![255, 0, 0, 255],
            }),
            ..ExportMaterial::default()
        };
        let glb = write_glb(
            &GltfExport::new(triangle(), vec![material]),
            &ExportOptions::default(),
        )
        .expect("GLB");
        gltf::Gltf::from_slice(&glb).expect("gltf 校验");
        let json = glb_json(&glb);
        assert_eq!(json["materials"][0]["name"], "M_Test");
        assert_eq!(json["textures"].as_array().unwrap().len(), 1);
        assert_eq!(json["images"][0]["mimeType"], "image/png");
        assert!(glb.windows(8).any(|window| window == b"\x89PNG\r\n\x1a\n"));
    }
}
