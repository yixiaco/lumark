//! USDA（USD ASCII）导出 + 自动材质旁挂。
//!
//! 输出一份主 `.usda`（`defaultPrim = Root`、`upAxis = "Y"`）与若干资源文件：
//! - 每个材质自动生成 `UsdPreviewSurface` 材质（基础色/金属度/粗糙度/自发光）；
//! - 基础色贴图写为相对路径 `textures/<材质>_BaseColor.png`；
//! - 多 section 网格用 `GeomSubset` 绑定不同材质。
//!
//! 坐标使用与 glTF 相同的 Z-up→Y-up 转换；`metersPerUnit` 根据缩放推导
//! （UE 厘米 → USD 米）。

use crate::gltf::ExportTexture;
use crate::{ExportError, ExportMaterial, ExportMesh, ExportOptions};

/// 一个需要写到旁挂路径的资源文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResource {
    /// 相对主文件所在目录的路径（使用 `/` 分隔）。
    pub relative_path: String,
    /// 文件字节。
    pub bytes: Vec<u8>,
}

/// USDA 导出结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsdaOutput {
    /// 主 `.usda` 文本。
    pub text: String,
    /// 贴图等旁挂资源。
    pub resources: Vec<ExportResource>,
}

/// 写出 USDA。
pub fn write_usda(
    mesh: &ExportMesh,
    materials: &[ExportMaterial],
    options: &ExportOptions,
) -> Result<UsdaOutput, ExportError> {
    crate::actorx::validate(mesh)?;
    let scale = options.scale.max(f32::MIN_POSITIVE);
    let meters_per_unit = 0.01 / scale;
    let mesh_id = sanitize_identifier(&mesh.name);
    let mut resources = Vec::new();
    let mut text = String::new();

    // 文件头。
    text.push_str("#usda 1.0\n");
    text.push_str("(\n");
    text.push_str("    defaultPrim = \"Root\"\n");
    text.push_str(&format!(
        "    metersPerUnit = {}\n",
        format_float(meters_per_unit)
    ));
    text.push_str("    upAxis = \"Y\"\n");
    text.push_str(")\n\n");
    text.push_str("def Xform \"Root\"\n{\n");

    // 材质 Scope。
    let material_names = material_names(mesh, materials);
    text.push_str("    def Scope \"Materials\"\n    {\n");
    for (index, material_name) in material_names.iter().enumerate() {
        let material = materials.get(index);
        write_material(&mut text, material_name, material, &mut resources)?;
    }
    text.push_str("    }\n\n");

    // Mesh。
    write_mesh(&mut text, &mesh_id, mesh, &material_names, scale, options);
    text.push_str("}\n");
    Ok(UsdaOutput { text, resources })
}

/// 每个 section 对应的材质名。
fn material_names(mesh: &ExportMesh, materials: &[ExportMaterial]) -> Vec<String> {
    let count = mesh.sections.len().max(1);
    (0..count)
        .map(|index| {
            materials
                .get(index)
                .map(|material| material.name.clone())
                .or_else(|| mesh.material_names.get(index).cloned())
                .unwrap_or_else(|| format!("MaterialSlot_{index}"))
        })
        .collect()
}

/// 写一个自动生成的材质。
fn write_material(
    text: &mut String,
    name: &str,
    material: Option<&ExportMaterial>,
    resources: &mut Vec<ExportResource>,
) -> Result<(), ExportError> {
    let default = ExportMaterial::default();
    let material = material.unwrap_or(&default);
    let id = sanitize_identifier(name);
    let path = format!("/Root/Materials/{id}");
    text.push_str(&format!("        def Material \"{id}\"\n        {{\n"));
    text.push_str(&format!(
        "            token outputs:surface.connect = <{path}/PreviewSurface.outputs:surface>\n\n"
    ));
    text.push_str("            def Shader \"PreviewSurface\"\n            {\n");
    text.push_str("                uniform token info:id = \"UsdPreviewSurface\"\n");
    let (texture_file, texture_name) = if let Some(texture) = &material.base_color_texture {
        let file = format!("textures/{id}_BaseColor.png");
        resources.push(ExportResource {
            relative_path: file.clone(),
            bytes: encode_png(texture)?,
        });
        (Some(file), format!("{id}_BaseColorTexture"))
    } else {
        (None, String::new())
    };
    if let Some(_file) = &texture_file {
        text.push_str(&format!(
            "                color3f inputs:diffuseColor.connect = <{path}/{texture_name}.outputs:rgb>\n"
        ));
    } else {
        text.push_str(&format!(
            "                color3f inputs:diffuseColor = {}\n",
            format_vec3([
                material.base_color[0],
                material.base_color[1],
                material.base_color[2],
            ])
        ));
    }
    text.push_str(&format!(
        "                float inputs:metallic = {}\n",
        format_float(material.metallic.clamp(0.0, 1.0))
    ));
    text.push_str(&format!(
        "                float inputs:roughness = {}\n",
        format_float(material.roughness.clamp(0.04, 1.0))
    ));
    text.push_str(&format!(
        "                color3f inputs:emissiveColor = {}\n",
        format_vec3(material.emissive)
    ));
    text.push_str("                float inputs:opacity = 1\n");
    text.push_str("                token outputs:surface\n");
    text.push_str("            }\n\n");

    if let Some(file) = texture_file {
        text.push_str(&format!(
            "            def Shader \"{texture_name}\"\n            {{\n"
        ));
        text.push_str("                uniform token info:id = \"UsdUVTexture\"\n");
        text.push_str(&format!("                asset inputs:file = @{file}@\n"));
        text.push_str(&format!(
            "                float2 inputs:st.connect = <{path}/UVReader.outputs:result>\n"
        ));
        text.push_str("                token inputs:wrapS = \"repeat\"\n");
        text.push_str("                token inputs:wrapT = \"repeat\"\n");
        text.push_str("                float3 outputs:rgb\n");
        text.push_str("            }\n\n");
        text.push_str("            def Shader \"UVReader\"\n            {\n");
        text.push_str("                uniform token info:id = \"UsdPrimvarReader_float2\"\n");
        text.push_str("                string inputs:varname = \"st\"\n");
        text.push_str("                float2 outputs:result\n");
        text.push_str("            }\n");
    }
    text.push_str("        }\n");
    Ok(())
}

/// 写 Mesh 与 section 子集。
fn write_mesh(
    text: &mut String,
    mesh_id: &str,
    mesh: &ExportMesh,
    material_names: &[String],
    scale: f32,
    options: &ExportOptions,
) {
    text.push_str(&format!("    def Mesh \"{mesh_id}\" (\n"));
    text.push_str("        prepend apiSchemas = [\"MaterialBindingAPI\"]\n");
    text.push_str("    )\n    {\n");
    text.push_str("        uniform bool doubleSided = 0\n");

    // points：Y-up 转换。
    text.push_str("        point3f[] points = [");
    for (index, vertex) in mesh.vertices.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&format_point(convert_point(vertex.position, scale)));
    }
    text.push_str("]\n");

    // normals。
    text.push_str("        normal3f[] normals = [");
    for (index, vertex) in mesh.vertices.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&format_vec3(normalize(convert_vector(vertex.normal))));
    }
    text.push_str("] (\n            interpolation = \"vertex\"\n        )\n");

    // UV。
    text.push_str("        texCoord2f[] primvars:st = [");
    for (index, vertex) in mesh.vertices.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&format!(
            "({}, {})",
            format_float(vertex.uv[0]),
            format_float(vertex.uv[1])
        ));
    }
    text.push_str("] (\n            interpolation = \"vertex\"\n        )\n");

    // 顶点色。
    if options.vertex_colors && !mesh.vertices.is_empty() {
        text.push_str("        color3f[] primvars:displayColor = [");
        for (index, vertex) in mesh.vertices.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            text.push_str(&format_vec3([
                f32::from(vertex.color[0]) / 255.0,
                f32::from(vertex.color[1]) / 255.0,
                f32::from(vertex.color[2]) / 255.0,
            ]));
        }
        text.push_str("] (\n            interpolation = \"vertex\"\n        )\n");
    }

    // 面/索引。
    text.push_str("        int[] faceVertexCounts = [");
    for triangle in 0..mesh.triangle_count() {
        if triangle > 0 {
            text.push_str(", ");
        }
        text.push('3');
    }
    text.push_str("]\n");
    text.push_str("        int[] faceVertexIndices = [");
    for (index, vertex) in mesh.indices.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&vertex.to_string());
    }
    text.push_str("]\n");

    // 单 section 直接绑定；多 section 用 GeomSubset。
    if mesh.sections.len() <= 1 {
        let material_index = mesh
            .sections
            .first()
            .map(|section| section.material_index)
            .unwrap_or(0)
            .min(material_names.len().saturating_sub(1));
        let name = material_names
            .get(material_index)
            .cloned()
            .unwrap_or_else(|| "MaterialSlot_0".to_string());
        text.push_str(&format!(
            "        rel material:binding = </Root/Materials/{}>\n",
            sanitize_identifier(&name)
        ));
    } else {
        for (index, section) in mesh.sections.iter().enumerate() {
            let material_index = section
                .material_index
                .min(material_names.len().saturating_sub(1));
            let name = material_names
                .get(material_index)
                .cloned()
                .unwrap_or_else(|| format!("MaterialSlot_{index}"));
            text.push_str(&format!(
                "        def GeomSubset \"subset_{index}\"\n        {{\n"
            ));
            text.push_str("            uniform token elementType = \"face\"\n");
            text.push_str("            uniform token familyName = \"materialBind\"\n");
            text.push_str("            int[] indices = [");
            for face in 0..section.triangle_count {
                if face > 0 {
                    text.push_str(", ");
                }
                text.push_str(&(section.first_index / 3 + face).to_string());
            }
            text.push_str("]\n");
            text.push_str(&format!(
                "            rel material:binding = </Root/Materials/{}>\n",
                sanitize_identifier(&name)
            ));
            text.push_str("        }\n");
        }
    }
    text.push_str("    }\n");
}

/// RGBA → PNG。
pub(crate) fn encode_png(texture: &ExportTexture) -> Result<Vec<u8>, ExportError> {
    crate::gltf::encode_png(texture)
}

/// UE Z-up → USD Y-up。
fn convert_point(point: [f32; 3], scale: f32) -> [f32; 3] {
    [point[0] * scale, point[2] * scale, -point[1] * scale]
}

/// 方向向量转换。
fn convert_vector(vector: [f32; 3]) -> [f32; 3] {
    [vector[0], vector[2], -vector[1]]
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

/// US D 合法标识符：非字母数字下划线替换。
fn sanitize_identifier(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "Unnamed".to_string()
    } else if sanitized
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        format!("_{sanitized}")
    } else {
        sanitized
    }
}

/// USD 数值。
fn format_float(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// USD 三元组。
fn format_vec3(value: [f32; 3]) -> String {
    format!(
        "({}, {}, {})",
        format_float(value[0]),
        format_float(value[1]),
        format_float(value[2])
    )
}

/// USD 点。
fn format_point(value: [f32; 3]) -> String {
    format_vec3(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExportMesh, MeshSection, MeshVertex};

    /// 构造带两个 section 的矩形（两个三角形）。
    fn quad() -> ExportMesh {
        ExportMesh {
            name: "SM_Test".to_string(),
            object_path: "Game/Content/SM_Test.SM_Test".to_string(),
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
                    position: [100.0, 100.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [1.0, 1.0],
                    color: [255; 4],
                },
                MeshVertex {
                    position: [0.0, 100.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0, 1.0],
                    color: [255; 4],
                },
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            sections: vec![
                MeshSection {
                    material_index: 0,
                    first_index: 0,
                    triangle_count: 1,
                },
                MeshSection {
                    material_index: 1,
                    first_index: 3,
                    triangle_count: 1,
                },
            ],
            material_names: vec!["M_A".to_string(), "M_B".to_string()],
            ..ExportMesh::default()
        }
    }

    /// 头部、Y-up 点与材质子集。
    #[test]
    fn writes_usda_mesh_and_materials() {
        let materials = vec![
            ExportMaterial {
                name: "M_A".to_string(),
                base_color: [1.0, 0.0, 0.0, 1.0],
                ..ExportMaterial::default()
            },
            ExportMaterial {
                name: "M_B".to_string(),
                ..ExportMaterial::default()
            },
        ];
        let output = write_usda(&quad(), &materials, &ExportOptions::default()).expect("usda");
        assert!(output.text.contains("#usda 1.0"));
        assert!(output.text.contains("defaultPrim = \"Root\""));
        assert!(output.text.contains("metersPerUnit = 0.01"));
        assert!(output.text.contains("upAxis = \"Y\""));
        assert!(output.text.contains("def Material \"M_A\""));
        assert!(output.text.contains("def Material \"M_B\""));
        assert!(output.text.contains("def GeomSubset \"subset_0\""));
        assert!(output.text.contains("def GeomSubset \"subset_1\""));
        // (100, 0, 0) → (100, 0, -0) = (100, 0, 0)；第二点 Y-up：z=0, -y=0。
        assert!(output.text.contains("(100, 0, 0)"));
        assert!(output.text.contains("faceVertexCounts = [3, 3]"));
        assert!(
            output
                .text
                .contains("faceVertexIndices = [0, 1, 2, 0, 2, 3]")
        );
        assert!(output.text.contains("inputs:diffuseColor = (1, 0, 0)"));
    }

    /// 贴图自动旁挂为 textures/*.png，并连接 UsdUVTexture。
    #[test]
    fn writes_usda_texture_resource() {
        let material = ExportMaterial {
            name: "M_Tex".to_string(),
            base_color_texture: Some(ExportTexture {
                name: "T_Base".to_string(),
                width: 1,
                height: 1,
                rgba: vec![255, 0, 0, 255],
            }),
            ..ExportMaterial::default()
        };
        let output = write_usda(&quad(), &[material], &ExportOptions::default()).expect("usda");
        assert_eq!(output.resources.len(), 1);
        assert_eq!(
            output.resources[0].relative_path,
            "textures/M_Tex_BaseColor.png"
        );
        assert_eq!(&output.resources[0].bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(output.text.contains("@textures/M_Tex_BaseColor.png@"));
        assert!(output.text.contains("UsdUVTexture"));
        assert!(output.text.contains("UsdPrimvarReader_float2"));
    }

    /// 缩放推导 metersPerUnit 并影响 points。
    #[test]
    fn writes_usda_scaled_units() {
        let options = ExportOptions {
            scale: 0.01,
            ..ExportOptions::default()
        };
        let output = write_usda(&quad(), &[], &options).expect("usda");
        assert!(output.text.contains("metersPerUnit = 1"));
        assert!(output.text.contains("(1, 0, 0)"));
    }

    /// 名称中的非法字符替换为下划线。
    #[test]
    fn sanitizes_identifiers() {
        assert_eq!(sanitize_identifier("M-Prop 01"), "M_Prop_01");
        assert_eq!(sanitize_identifier("123"), "_123");
        assert_eq!(sanitize_identifier(""), "Unnamed");
    }
}
