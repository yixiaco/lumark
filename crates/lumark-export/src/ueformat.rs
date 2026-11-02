//! UEFormat `UEMODEL` 导出（UEFormat v10，未压缩）。
//!
//! 布局参考 CUE4Parse `UEModel`：
//! - 文件头：`UEFORMAT` + FString 标识 `UEMODEL` + u8 版本 10 + FString 对象名/路径
//!   + bool 是否压缩（本实现写未压缩）；
//! - 数据根：FDataAttributeSet（属性数量 + 每个属性「FString 名称 + i32 长度 + 数据」）；
//! - `LODS`：每个 LOD 一个属性集，包含 VERTICES/NORMALS/TANGENTS/TEXCOORDS/
//!   INDICES/VERTEXCOLORS/MATERIALS，骨骼网格额外写 WEIGHTS；
//! - `SKELETON`：METADATA + BONES（名称/父索引/平移/四元数/缩放）。

use crate::{ExportError, ExportMesh, ExportOptions};

/// UEFormat 文件版本（AttributeFormatRestructure）。
const UEFORMAT_VERSION: u8 = 10;

/// 写出 UEFormat UEMODEL。
pub fn write_ueformat_model(
    mesh: &ExportMesh,
    options: &ExportOptions,
) -> Result<Vec<u8>, ExportError> {
    crate::actorx::validate(mesh)?;
    let scale = options.scale.max(f32::MIN_POSITIVE);
    let mut out = Vec::new();

    // 文件头。
    out.extend_from_slice(b"UEFORMAT");
    write_fstring(&mut out, "UEMODEL");
    out.push(UEFORMAT_VERSION);
    let object_name = mesh.name.as_str();
    write_fstring(&mut out, object_name);
    let object_path = if mesh.object_path.is_empty() {
        mesh.name.as_str()
    } else {
        mesh.object_path.as_str()
    };
    write_fstring(&mut out, object_path);
    out.push(0); // is_compressed = false

    // 根属性集。
    let tangents = compute_tangents(mesh);
    let mut root_attributes = Vec::new();
    root_attributes.push(attribute(
        "LODS",
        &write_array(&[write_lod(mesh, options, scale, &tangents)]),
    ));
    if !mesh.bones.is_empty() {
        root_attributes.push(attribute("SKELETON", &write_skeleton(mesh, scale)));
    }
    out.extend_from_slice(&attribute_set(&root_attributes));
    Ok(out)
}

/// 写一个 LOD 的属性集。
fn write_lod(
    mesh: &ExportMesh,
    options: &ExportOptions,
    scale: f32,
    tangents: &[[f32; 3]],
) -> Vec<u8> {
    let mut attributes = Vec::new();

    // VERTICES。
    let mut vertices = Vec::with_capacity(mesh.vertices.len() * 12);
    for vertex in &mesh.vertices {
        push_vec3(
            &mut vertices,
            [
                vertex.position[0] * scale,
                vertex.position[1] * scale,
                vertex.position[2] * scale,
            ],
        );
    }
    attributes.push(attribute(
        "VERTICES",
        &blob_array(mesh.vertices.len(), &vertices),
    ));

    // NORMALS：float W + xyz（W 记录切线手性，这里统一 1.0）。
    let mut normals = Vec::with_capacity(mesh.vertices.len() * 16);
    for vertex in &mesh.vertices {
        let normal = normalize(vertex.normal);
        normals.extend_from_slice(&1.0f32.to_le_bytes());
        push_vec3(&mut normals, normal);
    }
    attributes.push(attribute(
        "NORMALS",
        &blob_array(mesh.vertices.len(), &normals),
    ));

    // TANGENTS。
    let mut tangent_bytes = Vec::with_capacity(mesh.vertices.len() * 12);
    for tangent in tangents {
        push_vec3(&mut tangent_bytes, *tangent);
    }
    attributes.push(attribute(
        "TANGENTS",
        &blob_array(mesh.vertices.len(), &tangent_bytes),
    ));

    // TEXCOORDS：UV0 + 额外通道。
    let mut uv_channels: Vec<(String, &Vec<[f32; 2]>)> = Vec::new();
    let mut main_uvs = Vec::with_capacity(mesh.vertices.len());
    for vertex in &mesh.vertices {
        main_uvs.push(vertex.uv);
    }
    uv_channels.push(("UV0".to_string(), &main_uvs));
    for (index, channel) in mesh.extra_uvs.iter().enumerate() {
        if channel.len() == mesh.vertices.len() {
            uv_channels.push((format!("UV{}", index + 1), channel));
        }
    }
    let mut uv_sets = Vec::new();
    for (name, uvs) in &uv_channels {
        let mut body = Vec::with_capacity(uvs.len() * 8);
        for uv in uvs.iter() {
            body.extend_from_slice(&uv[0].to_le_bytes());
            body.extend_from_slice(&uv[1].to_le_bytes());
        }
        let mut set = Vec::new();
        write_fstring(&mut set, name);
        set.extend_from_slice(&blob_array(uvs.len(), &body));
        uv_sets.push(set);
    }
    attributes.push(attribute("TEXCOORDS", &write_array(&uv_sets)));

    // INDICES。
    let mut indices = Vec::with_capacity(mesh.indices.len() * 4);
    for index in &mesh.indices {
        indices.extend_from_slice(&(*index as i32).to_le_bytes());
    }
    attributes.push(attribute(
        "INDICES",
        &blob_array(mesh.indices.len(), &indices),
    ));

    // VERTEXCOLORS。
    if options.vertex_colors && !mesh.vertices.is_empty() {
        let mut colors = Vec::with_capacity(mesh.vertices.len() * 4);
        for vertex in &mesh.vertices {
            colors.extend_from_slice(&vertex.color);
        }
        let mut set = Vec::new();
        write_fstring(&mut set, "Colors");
        set.extend_from_slice(&blob_array(mesh.vertices.len(), &colors));
        attributes.push(attribute("VERTEXCOLORS", &write_array(&[set])));
    }

    // MATERIALS：槽名/材质路径/起始索引/三角形数。
    let mut materials = Vec::with_capacity(mesh.sections.len() * 24);
    for (index, section) in mesh.sections.iter().enumerate() {
        let name = mesh
            .material_names
            .get(section.material_index)
            .cloned()
            .unwrap_or_else(|| format!("MaterialSlot_{index}"));
        write_fstring(&mut materials, &name);
        write_fstring(&mut materials, "");
        materials.extend_from_slice(&(section.first_index as i32).to_le_bytes());
        materials.extend_from_slice(&(section.triangle_count as i32).to_le_bytes());
    }
    attributes.push(attribute(
        "MATERIALS",
        &blob_array(mesh.sections.len(), &materials),
    ));

    // WEIGHTS。
    if mesh.is_skinned() {
        let mut weights = Vec::new();
        let mut count = 0usize;
        for (vertex_index, (indices, vertex_weights)) in mesh
            .bone_indices
            .iter()
            .zip(mesh.bone_weights.iter())
            .enumerate()
        {
            for (influence, bone) in indices.iter().enumerate() {
                let weight = vertex_weights[influence];
                if weight <= 0.0 || (*bone as usize) >= mesh.bones.len() {
                    continue;
                }
                weights.extend_from_slice(&bone.to_le_bytes());
                weights.extend_from_slice(&(vertex_index as i32).to_le_bytes());
                weights.extend_from_slice(&weight.to_le_bytes());
                count += 1;
            }
        }
        attributes.push(attribute("WEIGHTS", &blob_array(count, &weights)));
    }

    // LOD 名称 + 属性集。
    let mut lod = Vec::new();
    write_fstring(&mut lod, "LOD0");
    lod.extend_from_slice(&attribute_set(&attributes));
    lod
}

/// 写 SKELETON 属性（属性集：METADATA + BONES）。
fn write_skeleton(mesh: &ExportMesh, scale: f32) -> Vec<u8> {
    let mut attributes = Vec::new();
    let mut metadata = Vec::new();
    write_fstring(&mut metadata, &mesh.name);
    attributes.push(attribute("METADATA", &metadata));

    let mut bones = Vec::with_capacity(mesh.bones.len() * 56);
    for bone in &mesh.bones {
        write_fstring(&mut bones, &bone.name);
        bones.extend_from_slice(&bone.parent_index.to_le_bytes());
        push_vec3(
            &mut bones,
            [
                bone.position[0] * scale,
                bone.position[1] * scale,
                bone.position[2] * scale,
            ],
        );
        for component in bone.rotation {
            bones.extend_from_slice(&component.to_le_bytes());
        }
        push_vec3(&mut bones, [1.0, 1.0, 1.0]);
    }
    attributes.push(attribute("BONES", &blob_array(mesh.bones.len(), &bones)));
    attribute_set(&attributes)
}

/// 属性：FString 名称 + i32 数据长度 + 数据。
fn attribute(name: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 8 + data.len());
    write_fstring(&mut out, name);
    out.extend_from_slice(&(data.len() as i32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// 属性集：i32 数量 + 每个属性。
fn attribute_set(attributes: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(attributes.len() as i32).to_le_bytes());
    for attribute in attributes {
        out.extend_from_slice(attribute);
    }
    out
}

/// 数组：i32 数量 + 每个元素字节。
fn write_array(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(items.len() as i32).to_le_bytes());
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// 数量 + 连续数据。
fn blob_array(count: usize, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&(count as i32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// FString：i32 UTF-8 字节数 + 字节。
fn write_fstring(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as i32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// 写入 vec3。
fn push_vec3(out: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        out.extend_from_slice(&component.to_le_bytes());
    }
}

/// 归一化；零向量回退 +Z。
fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, value[1] / length, value[2] / length]
    }
}

/// 由位置/法线/UV 估算逐顶点切线（Gram-Schmidt 正交化）。
fn compute_tangents(mesh: &ExportMesh) -> Vec<[f32; 3]> {
    let mut accumulators = vec![[0.0f32; 3]; mesh.vertices.len()];
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let (Some(va), Some(vb), Some(vc)) = (
            mesh.vertices.get(a),
            mesh.vertices.get(b),
            mesh.vertices.get(c),
        ) else {
            continue;
        };
        let edge1 = subtract(vb.position, va.position);
        let edge2 = subtract(vc.position, va.position);
        let duv1 = [vb.uv[0] - va.uv[0], vb.uv[1] - va.uv[1]];
        let duv2 = [vc.uv[0] - va.uv[0], vc.uv[1] - va.uv[1]];
        let determinant = duv1[0] * duv2[1] - duv2[0] * duv1[1];
        if determinant.abs() <= f32::EPSILON {
            continue;
        }
        let inverse = 1.0 / determinant;
        let tangent = [
            inverse * (duv2[1] * edge1[0] - duv1[1] * edge2[0]),
            inverse * (duv2[1] * edge1[1] - duv1[1] * edge2[1]),
            inverse * (duv2[1] * edge1[2] - duv1[1] * edge2[2]),
        ];
        for index in [a, b, c] {
            accumulators[index][0] += tangent[0];
            accumulators[index][1] += tangent[1];
            accumulators[index][2] += tangent[2];
        }
    }
    accumulators
        .into_iter()
        .enumerate()
        .map(|(index, tangent)| {
            let normal = mesh
                .vertices
                .get(index)
                .map(|vertex| normalize(vertex.normal))
                .unwrap_or([0.0, 0.0, 1.0]);
            let dot = tangent[0] * normal[0] + tangent[1] * normal[1] + tangent[2] * normal[2];
            let orthogonal = [
                tangent[0] - normal[0] * dot,
                tangent[1] - normal[1] * dot,
                tangent[2] - normal[2] * dot,
            ];
            if orthogonal.iter().map(|value| value * value).sum::<f32>() <= f32::EPSILON {
                let fallback = if normal[0].abs() < 0.9 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                };
                let dot =
                    fallback[0] * normal[0] + fallback[1] * normal[1] + fallback[2] * normal[2];
                normalize([
                    fallback[0] - normal[0] * dot,
                    fallback[1] - normal[1] * dot,
                    fallback[2] - normal[2] * dot,
                ])
            } else {
                normalize(orthogonal)
            }
        })
        .collect()
}

/// 向量相减。
fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExportBone, MeshSection, MeshVertex};

    /// 构造三角形。
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
                    position: [10.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [1.0, 0.0],
                    color: [255; 4],
                },
                MeshVertex {
                    position: [0.0, 10.0, 0.0],
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
            material_names: vec!["M_Test".to_string()],
            ..ExportMesh::default()
        }
    }

    /// 读取工具。
    struct Reader<'a> {
        data: &'a [u8],
        position: usize,
    }

    impl<'a> Reader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self { data, position: 0 }
        }

        fn bytes(&mut self, count: usize) -> &'a [u8] {
            let end = (self.position + count).min(self.data.len());
            let slice = &self.data[self.position..end];
            self.position = end;
            slice
        }

        fn u8(&mut self) -> u8 {
            self.bytes(1)[0]
        }

        fn i32(&mut self) -> i32 {
            i32::from_le_bytes(self.bytes(4).try_into().unwrap_or_default())
        }

        fn u16(&mut self) -> u16 {
            u16::from_le_bytes(self.bytes(2).try_into().unwrap_or_default())
        }

        fn f32(&mut self) -> f32 {
            f32::from_le_bytes(self.bytes(4).try_into().unwrap_or_default())
        }

        fn fstring(&mut self) -> String {
            let length = self.i32().max(0) as usize;
            String::from_utf8_lossy(self.bytes(length)).to_string()
        }
    }

    /// 解析属性集为 (名称 → 数据)。
    fn attributes(reader: &mut Reader) -> Vec<(String, Vec<u8>)> {
        let count = reader.i32().max(0) as usize;
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            let name = reader.fstring();
            let length = reader.i32().max(0) as usize;
            let data = reader.bytes(length).to_vec();
            result.push((name, data));
        }
        result
    }

    /// 解析文件头与 LOD 属性。
    fn parse(data: &[u8]) -> (String, String, Vec<(String, Vec<u8>)>) {
        let mut reader = Reader::new(data);
        assert_eq!(reader.bytes(8), b"UEFORMAT");
        assert_eq!(reader.fstring(), "UEMODEL");
        assert_eq!(reader.u8(), UEFORMAT_VERSION);
        let name = reader.fstring();
        let path = reader.fstring();
        assert_eq!(reader.u8(), 0);
        let root = attributes(&mut reader);
        let lods = find(&root, "LODS");
        let mut lod_reader = Reader::new(lods);
        assert_eq!(lod_reader.i32(), 1);
        assert_eq!(lod_reader.fstring(), "LOD0");
        let lod_attributes = attributes(&mut lod_reader);
        (name, path, lod_attributes)
    }

    /// 按属性名取数据。
    fn find<'a>(attributes: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
        attributes
            .iter()
            .find(|(attribute, _)| attribute == name)
            .map(|(_, data)| data.as_slice())
            .unwrap_or_else(|| panic!("缺少属性 {name}"))
    }

    /// 文件头、LOD 属性与材质数据。
    #[test]
    fn writes_ueformat_header_and_lod() {
        let data = write_ueformat_model(&triangle(), &ExportOptions::default()).expect("ueformat");
        let (name, path, attributes) = parse(&data);
        assert_eq!(name, "SM_Test");
        assert_eq!(path, "SM_Test");
        let names: Vec<&str> = attributes.iter().map(|(name, _)| name.as_str()).collect();
        for expected in [
            "VERTICES",
            "NORMALS",
            "TANGENTS",
            "TEXCOORDS",
            "INDICES",
            "VERTEXCOLORS",
            "MATERIALS",
        ] {
            assert!(names.contains(&expected), "缺少 {expected}: {names:?}");
        }
        assert!(!names.contains(&"WEIGHTS"));

        let mut reader = Reader::new(find(&attributes, "VERTICES"));
        assert_eq!(reader.i32(), 3);
        assert_eq!(reader.f32(), 0.0);

        let mut reader = Reader::new(find(&attributes, "MATERIALS"));
        assert_eq!(reader.i32(), 1);
        assert_eq!(reader.fstring(), "M_Test");
        assert_eq!(reader.fstring(), "");
        assert_eq!(reader.i32(), 0);
        assert_eq!(reader.i32(), 1);
    }

    /// 骨骼网格写出 WEIGHTS 与 SKELETON。
    #[test]
    fn writes_ueformat_skinning() {
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
        let data = write_ueformat_model(&mesh, &ExportOptions::default()).expect("ueformat");
        let (_, _, lod_attributes) = parse(&data);
        let mut reader = Reader::new(find(&lod_attributes, "WEIGHTS"));
        assert_eq!(reader.i32(), 6);
        assert_eq!(reader.u16(), 0);
        assert_eq!(reader.i32(), 0);
        let _ = reader.f32();

        let mut reader = Reader::new(&data);
        let _ = reader.bytes(8);
        let _ = reader.fstring();
        let _ = reader.u8();
        let _ = reader.fstring();
        let _ = reader.fstring();
        let _ = reader.u8();
        let root = attributes(&mut reader);
        let mut reader = Reader::new(find(&root, "SKELETON"));
        let skeleton_attributes = attributes(&mut reader);
        let mut reader = Reader::new(find(&skeleton_attributes, "BONES"));
        assert_eq!(reader.i32(), 2);
        assert_eq!(reader.fstring(), "Root");
        assert_eq!(reader.i32(), -1);
    }

    /// 额外 UV 通道按 UV0/UV1/UV2 写入且逐通道数量正确。
    #[test]
    fn writes_ueformat_extra_uvs() {
        let mut mesh = triangle();
        mesh.extra_uvs = vec![vec![[0.1, 0.2]; 3], vec![[0.3, 0.4]; 3]];
        let data = write_ueformat_model(&mesh, &ExportOptions::default()).expect("ueformat");
        let (_, _, attributes) = parse(&data);
        let mut reader = Reader::new(find(&attributes, "TEXCOORDS"));
        assert_eq!(reader.i32(), 3);
        let mut names = Vec::new();
        for _ in 0..3 {
            names.push(reader.fstring());
            assert_eq!(reader.i32(), 3);
            for _ in 0..3 {
                let _ = reader.f32();
                let _ = reader.f32();
            }
        }
        assert_eq!(names, ["UV0", "UV1", "UV2"]);
    }
}
