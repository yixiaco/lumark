//! ActorX PSK 导出（UE Viewer / Blender 导入器兼容）。
//!
//! 布局参考 CUE4Parse 的 `ActorXMesh`：主头 `ACTRHEAD` + 版本 20220723，随后
//! `PNTS0000`、`VTXW0000`、`FACE0000/FACE3200`、`MATT0000`、`VTXNORMS`、
//! `RAWWEIGHTS`、`VERTEXCOLOR`、`EXTRAUVSn`、`REFSKELT`。
//!
//! 坐标处理：UE 为 Z-up、左手系；PSK 约定导出时镜像 Y（位置/法线/骨骼位置取反，
//! 四元数 y 取反，根骨骼 w 再取反）。

use crate::ExportError;

/// ActorX PSK 主版本号。
const PSK_VERSION: i32 = 20_220_723;
/// 名称字段长度。
const NAME_LENGTH: usize = 64;

/// 一个导出顶点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshVertex {
    /// 位置（UE 空间，Z-up）。
    pub position: [f32; 3],
    /// 法线。
    pub normal: [f32; 3],
    /// 第一组 UV。
    pub uv: [f32; 2],
    /// RGBA 顶点色。
    pub color: [u8; 4],
}

impl Default for MeshVertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0; 2],
            color: [255; 4],
        }
    }
}

/// 网格 section。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshSection {
    /// 材质槽索引（对应 `ExportMesh::material_names`）。
    pub material_index: usize,
    /// 在索引缓冲中的起始索引。
    pub first_index: usize,
    /// 三角形数量。
    pub triangle_count: usize,
}

/// 一根导出骨骼。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportBone {
    /// 骨骼名。
    pub name: String,
    /// 父索引；根为 -1。
    pub parent_index: i32,
    /// 参考姿态平移。
    pub position: [f32; 3],
    /// 参考姿态旋转（xyzw）。
    pub rotation: [f32; 4],
}

/// 导出网格（静态或骨骼）。
#[derive(Debug, Clone, Default)]
pub struct ExportMesh {
    /// 网格名。
    pub name: String,
    /// 顶点。
    pub vertices: Vec<MeshVertex>,
    /// 三角形索引。
    pub indices: Vec<u32>,
    /// section。
    pub sections: Vec<MeshSection>,
    /// 材质槽名。
    pub material_names: Vec<String>,
    /// 额外 UV 通道（第一组之后），写为 `EXTRAUVS0/1/...`。
    pub extra_uvs: Vec<Vec<[f32; 2]>>,
    /// 参考骨架。
    pub bones: Vec<ExportBone>,
    /// 每顶点的骨骼索引（与 `bone_weights` 对齐，最多 4 个）。
    pub bone_indices: Vec<[u16; 4]>,
    /// 每顶点的骨骼权重（0–1）。
    pub bone_weights: Vec<[f32; 4]>,
}

impl ExportMesh {
    /// 构造单 section 静态网格。
    pub fn static_mesh(
        name: impl Into<String>,
        vertices: Vec<MeshVertex>,
        indices: Vec<u32>,
    ) -> Self {
        let triangle_count = indices.len() / 3;
        Self {
            name: name.into(),
            vertices,
            indices,
            sections: vec![MeshSection {
                material_index: 0,
                first_index: 0,
                triangle_count,
            }],
            ..Self::default()
        }
    }

    /// 三角形数量。
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// 是否含蒙皮信息。
    pub fn is_skinned(&self) -> bool {
        !self.bones.is_empty() && self.bone_indices.len() == self.vertices.len()
    }
}

/// 导出选项。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportOptions {
    /// 位置/骨骼平移缩放（UE 厘米 → 其它单位时可设 0.01）。
    pub scale: f32,
    /// 是否写顶点色。
    pub vertex_colors: bool,
    /// 是否写额外 UV。
    pub extra_uvs: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            vertex_colors: true,
            extra_uvs: true,
        }
    }
}

/// 写出 ActorX PSK。
pub fn write_psk(mesh: &ExportMesh, options: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    validate(mesh)?;
    let scale = options.scale.max(f32::MIN_POSITIVE);
    let mut out = Vec::new();
    // 主头：TypeFlag 为 PSK 版本，DataSize/DataCount 为 0。
    write_chunk(&mut out, "ACTRHEAD", PSK_VERSION, 0, 0, &[]);

    // PNTS0000：镜像 Y 的位置。
    let mut points = Vec::with_capacity(mesh.vertices.len() * 12);
    for vertex in &mesh.vertices {
        let position = [
            vertex.position[0] * scale,
            -vertex.position[1] * scale,
            vertex.position[2] * scale,
        ];
        push_f32x3(&mut points, position);
    }
    write_chunk(
        &mut out,
        "PNTS0000",
        0,
        12,
        mesh.vertices.len() as i32,
        &points,
    );

    // VTXW0000：uint16 顶点索引 + uint16 保留 + UV + 材质 + 保留 + int16 填充。
    let wedge_material = wedge_materials(mesh);
    let mut wedges = Vec::with_capacity(mesh.vertices.len() * 16);
    for (index, vertex) in mesh.vertices.iter().enumerate() {
        wedges.extend_from_slice(&(index as u16).to_le_bytes());
        wedges.extend_from_slice(&0u16.to_le_bytes());
        wedges.extend_from_slice(&vertex.uv[0].to_le_bytes());
        wedges.extend_from_slice(&vertex.uv[1].to_le_bytes());
        wedges.push(wedge_material[index] as u8);
        wedges.push(0);
        wedges.extend_from_slice(&0i16.to_le_bytes());
    }
    write_chunk(
        &mut out,
        "VTXW0000",
        0,
        16,
        mesh.vertices.len() as i32,
        &wedges,
    );

    // FACE0000 / FACE3200。
    if mesh.vertices.len() <= u16::MAX as usize {
        let mut faces = Vec::with_capacity(mesh.triangle_count() * 12);
        for section in &mesh.sections {
            let start = section.first_index;
            let end = start + section.triangle_count * 3;
            for triangle in mesh.indices[start..end].as_chunks::<3>().0 {
                // 交换前两个索引以匹配镜像后的绕序。
                faces.extend_from_slice(&(triangle[1] as u16).to_le_bytes());
                faces.extend_from_slice(&(triangle[0] as u16).to_le_bytes());
                faces.extend_from_slice(&(triangle[2] as u16).to_le_bytes());
                faces.push(section.material_index.min(255) as u8);
                faces.push(0);
                faces.extend_from_slice(&1u32.to_le_bytes());
            }
        }
        write_chunk(
            &mut out,
            "FACE0000",
            0,
            12,
            mesh.triangle_count() as i32,
            &faces,
        );
    } else {
        let mut faces = Vec::with_capacity(mesh.triangle_count() * 18);
        for section in &mesh.sections {
            let start = section.first_index;
            let end = start + section.triangle_count * 3;
            for triangle in mesh.indices[start..end].as_chunks::<3>().0 {
                faces.extend_from_slice(&triangle[1].to_le_bytes());
                faces.extend_from_slice(&triangle[0].to_le_bytes());
                faces.extend_from_slice(&triangle[2].to_le_bytes());
                faces.push(section.material_index.min(255) as u8);
                faces.push(0);
                faces.extend_from_slice(&1u32.to_le_bytes());
            }
        }
        write_chunk(
            &mut out,
            "FACE3200",
            0,
            18,
            mesh.triangle_count() as i32,
            &faces,
        );
    }

    // MATT0000。
    let mut materials = Vec::with_capacity(mesh.sections.len() * 88);
    for (index, section) in mesh.sections.iter().enumerate() {
        let name = mesh
            .material_names
            .get(section.material_index)
            .cloned()
            .unwrap_or_else(|| format!("MaterialSlot_{index}"));
        write_fixed_name(&mut materials, &name);
        materials.extend_from_slice(&(section.material_index as i32).to_le_bytes());
        materials.extend_from_slice(&0u32.to_le_bytes());
        materials.extend_from_slice(&0i32.to_le_bytes());
        materials.extend_from_slice(&0u32.to_le_bytes());
        materials.extend_from_slice(&0i32.to_le_bytes());
        materials.extend_from_slice(&0i32.to_le_bytes());
    }
    write_chunk(
        &mut out,
        "MATT0000",
        0,
        88,
        mesh.sections.len() as i32,
        &materials,
    );

    // VTXNORMS：镜像 Y 的法线。
    let mut normals = Vec::with_capacity(mesh.vertices.len() * 12);
    for vertex in &mesh.vertices {
        let normal = normalize([vertex.normal[0], -vertex.normal[1], vertex.normal[2]]);
        push_f32x3(&mut normals, normal);
    }
    write_chunk(
        &mut out,
        "VTXNORMS",
        0,
        12,
        mesh.vertices.len() as i32,
        &normals,
    );

    // RAWWEIGHTS：每个影响一条记录。
    let mut weights = Vec::new();
    let mut weight_count = 0i32;
    if mesh.is_skinned() {
        for (vertex_index, indices) in mesh.bone_indices.iter().enumerate() {
            let vertex_weights = mesh.bone_weights[vertex_index];
            for (influence, bone) in indices.iter().enumerate() {
                let weight = vertex_weights[influence];
                if weight <= 0.0 || (*bone as usize) >= mesh.bones.len() {
                    continue;
                }
                weights.extend_from_slice(&weight.to_le_bytes());
                weights.extend_from_slice(&(vertex_index as i32).to_le_bytes());
                weights.extend_from_slice(&(*bone as i32).to_le_bytes());
                weight_count += 1;
            }
        }
    }
    write_chunk(&mut out, "RAWWEIGHTS", 0, 12, weight_count, &weights);

    // VERTEXCOLOR。
    if options.vertex_colors && !mesh.vertices.is_empty() {
        let mut colors = Vec::with_capacity(mesh.vertices.len() * 4);
        for vertex in &mesh.vertices {
            colors.extend_from_slice(&vertex.color);
        }
        write_chunk(
            &mut out,
            "VERTEXCOLOR",
            0,
            4,
            mesh.vertices.len() as i32,
            &colors,
        );
    }

    // EXTRAUVS0..n。
    if options.extra_uvs {
        for (channel, uvs) in mesh.extra_uvs.iter().enumerate() {
            let mut body = Vec::with_capacity(uvs.len() * 8);
            for uv in uvs {
                body.extend_from_slice(&uv[0].to_le_bytes());
                body.extend_from_slice(&uv[1].to_le_bytes());
            }
            write_chunk(
                &mut out,
                &format!("EXTRAUVS{channel}"),
                0,
                8,
                uvs.len() as i32,
                &body,
            );
        }
    }

    // REFSKELT。
    let mut bones = Vec::with_capacity(mesh.bones.len() * 120);
    for (index, bone) in mesh.bones.iter().enumerate() {
        let children = mesh
            .bones
            .iter()
            .enumerate()
            .filter(|(child, candidate)| *child != index && candidate.parent_index == index as i32)
            .count();
        write_fixed_name(&mut bones, &bone.name);
        bones.extend_from_slice(&0u32.to_le_bytes()); // Flags
        bones.extend_from_slice(&(children as i32).to_le_bytes());
        bones.extend_from_slice(&bone.parent_index.to_le_bytes());
        let mut rotation = bone.rotation;
        rotation[1] = -rotation[1];
        if index == 0 {
            rotation[3] = -rotation[3];
        }
        for component in rotation {
            bones.extend_from_slice(&component.to_le_bytes());
        }
        let position = [
            bone.position[0] * scale,
            -bone.position[1] * scale,
            bone.position[2] * scale,
        ];
        push_f32x3(&mut bones, position);
        bones.extend_from_slice(&0f32.to_le_bytes()); // Length
        push_f32x3(&mut bones, [0.0; 3]); // Size
    }
    write_chunk(
        &mut out,
        "REFSKELT",
        0,
        120,
        mesh.bones.len() as i32,
        &bones,
    );
    Ok(out)
}

/// 校验导出网格。
fn validate(mesh: &ExportMesh) -> Result<(), ExportError> {
    if !mesh.indices.len().is_multiple_of(3) {
        return Err(ExportError::InvalidIndexCount);
    }
    let vertex_count = mesh.vertices.len();
    if let Some(index) = mesh
        .indices
        .iter()
        .find(|index| **index as usize >= vertex_count)
    {
        return Err(ExportError::IndexOutOfRange {
            index: *index,
            vertices: vertex_count,
        });
    }
    for (index, section) in mesh.sections.iter().enumerate() {
        let end = section
            .first_index
            .checked_add(section.triangle_count.saturating_mul(3))
            .ok_or(ExportError::InvalidSection { section: index })?;
        if end > mesh.indices.len() || !section.first_index.is_multiple_of(3) {
            return Err(ExportError::InvalidSection { section: index });
        }
    }
    if !mesh.bones.is_empty()
        && (mesh.bone_indices.len() != vertex_count || mesh.bone_weights.len() != vertex_count)
    {
        return Err(ExportError::InvalidSkinLength);
    }
    if let Some((index, _)) = mesh
        .bones
        .iter()
        .enumerate()
        .find(|(index, bone)| bone.parent_index >= *index as i32 || bone.parent_index < -1)
    {
        return Err(ExportError::InvalidBoneParent {
            bone: index,
            parent: mesh.bones[index].parent_index,
        });
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

/// 固定长度名称字段（UTF-8 截断、补零）。
fn write_fixed_name(out: &mut Vec<u8>, name: &str) {
    let mut field = [0u8; NAME_LENGTH];
    let bytes = name.as_bytes();
    let length = bytes.len().min(field.len());
    field[..length].copy_from_slice(&bytes[..length]);
    out.extend_from_slice(&field);
}

/// 推导每个顶点所属 section。
fn wedge_materials(mesh: &ExportMesh) -> Vec<usize> {
    let mut materials = vec![0usize; mesh.vertices.len()];
    for (index, section) in mesh.sections.iter().enumerate() {
        let start = section.first_index;
        let end = (start + section.triangle_count * 3).min(mesh.indices.len());
        for vertex in &mesh.indices[start..end] {
            if let Some(slot) = materials.get_mut(*vertex as usize) {
                *slot = index;
            }
        }
    }
    materials
}

/// 写入 3 个 f32。
fn push_f32x3(out: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        out.extend_from_slice(&component.to_le_bytes());
    }
}

/// 归一化向量；零向量回退 +Z。
fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, value[1] / length, value[2] / length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// chunk 描述。
    struct Chunk<'a> {
        name: String,
        type_flag: i32,
        data_size: i32,
        count: i32,
        data: &'a [u8],
    }

    /// 解析 chunk 列表。
    fn parse_chunks(data: &[u8]) -> Vec<Chunk<'_>> {
        let mut chunks = Vec::new();
        let mut position = 0usize;
        while position + 32 <= data.len() {
            let name = String::from_utf8_lossy(&data[position..position + 20])
                .trim_end_matches('\0')
                .to_string();
            let type_flag =
                i32::from_le_bytes(data[position + 20..position + 24].try_into().unwrap());
            let data_size =
                i32::from_le_bytes(data[position + 24..position + 28].try_into().unwrap());
            let count = i32::from_le_bytes(data[position + 28..position + 32].try_into().unwrap());
            position += 32;
            let length = data_size.max(0) as usize * count.max(0) as usize;
            let end = (position + length).min(data.len());
            chunks.push(Chunk {
                name,
                type_flag,
                data_size,
                count,
                data: &data[position..end],
            });
            position = end;
        }
        chunks
    }

    /// 构造一个带法线/UV 的三角形。
    fn triangle() -> ExportMesh {
        ExportMesh::static_mesh(
            "SM_Test",
            vec![
                MeshVertex {
                    position: [0.0, 1.0, 2.0],
                    normal: [0.0, 1.0, 0.0],
                    uv: [0.25, 0.75],
                    color: [10, 20, 30, 255],
                },
                MeshVertex {
                    position: [3.0, 4.0, 5.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.5, 0.5],
                    color: [255; 4],
                },
                MeshVertex {
                    position: [6.0, 7.0, 8.0],
                    normal: [1.0, 0.0, 0.0],
                    uv: [1.0, 0.0],
                    color: [0; 4],
                },
            ],
            vec![0, 1, 2],
        )
    }

    /// 写出静态 PSK 的 chunk 顺序、头版本与镜像位置。
    #[test]
    fn writes_static_psk_chunks() {
        let data = write_psk(&triangle(), &ExportOptions::default()).expect("PSK");
        let chunks = parse_chunks(&data);
        let names: Vec<&str> = chunks.iter().map(|chunk| chunk.name.as_str()).collect();
        assert_eq!(names[0], "ACTRHEAD");
        assert_eq!(chunks[0].type_flag, PSK_VERSION);
        for expected in [
            "PNTS0000",
            "VTXW0000",
            "FACE0000",
            "MATT0000",
            "VTXNORMS",
            "RAWWEIGHTS",
            "VERTEXCOLOR",
            "REFSKELT",
        ] {
            assert!(names.contains(&expected), "缺少 {expected}：{names:?}");
        }
        let points = chunks
            .iter()
            .find(|chunk| chunk.name == "PNTS0000")
            .unwrap();
        assert_eq!(points.count, 3);
        assert_eq!(points.data_size, 12);
        let y = f32::from_le_bytes(points.data[4..8].try_into().unwrap());
        assert!((y + 1.0).abs() < 1e-6, "Y 未镜像：{y}");
        let faces = chunks
            .iter()
            .find(|chunk| chunk.name == "FACE0000")
            .unwrap();
        assert_eq!(faces.count, 1);
        let first = u16::from_le_bytes(faces.data[0..2].try_into().unwrap());
        let second = u16::from_le_bytes(faces.data[2..4].try_into().unwrap());
        assert_eq!((first, second), (1, 0));
    }

    /// 骨骼网格写出权重与骨架。
    #[test]
    fn writes_skeletal_psk_bones_and_weights() {
        let mut mesh = triangle();
        mesh.bones = vec![
            ExportBone {
                name: "Root".to_string(),
                parent_index: -1,
                position: [0.0, 0.0, 0.0],
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
        mesh.bone_weights = vec![
            [0.5, 0.5, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
        ];
        let data = write_psk(&mesh, &ExportOptions::default()).expect("PSK");
        let chunks = parse_chunks(&data);
        let weights = chunks
            .iter()
            .find(|chunk| chunk.name == "RAWWEIGHTS")
            .unwrap();
        assert_eq!(weights.count, 3);
        assert_eq!(weights.data_size, 12);
        let bone_index = i32::from_le_bytes(weights.data[8..12].try_into().unwrap());
        assert_eq!(bone_index, 0);
        // 第二条记录是顶点 0 的第二个影响（骨骼 1）。
        let second_bone = i32::from_le_bytes(weights.data[20..24].try_into().unwrap());
        assert_eq!(second_bone, 1);
        let skeleton = chunks
            .iter()
            .find(|chunk| chunk.name == "REFSKELT")
            .unwrap();
        assert_eq!(skeleton.count, 2);
        assert_eq!(skeleton.data_size, 120);
        let parent = i32::from_le_bytes(skeleton.data[64 + 8..64 + 12].try_into().unwrap());
        assert_eq!(parent, -1);
    }

    /// 非法索引/骨骼父级会返回错误。
    #[test]
    fn rejects_invalid_mesh() {
        let mut mesh = triangle();
        mesh.indices = vec![0, 1, 9];
        assert!(matches!(
            write_psk(&mesh, &ExportOptions::default()),
            Err(ExportError::IndexOutOfRange { .. })
        ));
        let mut mesh = triangle();
        mesh.bones = vec![ExportBone {
            name: "A".to_string(),
            parent_index: 5,
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }];
        mesh.bone_indices = vec![[0; 4]; 3];
        mesh.bone_weights = vec![[0.0; 4]; 3];
        assert!(matches!(
            write_psk(&mesh, &ExportOptions::default()),
            Err(ExportError::InvalidBoneParent { .. })
        ));
    }
}
