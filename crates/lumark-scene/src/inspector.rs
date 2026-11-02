//! 通用详情面板模型：UI 只负责渲染键值行，不关心资产种类。

use crate::scene::{Axis, Scene};

/// 一行详情。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorRow {
    /// 标签。
    pub label: String,
    /// 值。
    pub value: String,
}

/// 一组详情。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectorSection {
    /// 分组标题；空标题表示不显示分组头。
    pub title: String,
    /// 行。
    pub rows: Vec<InspectorRow>,
}

/// 详情面板模型。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectorModel {
    /// 标题。
    pub title: String,
    /// 副标题。
    pub subtitle: Option<String>,
    /// 分组。
    pub sections: Vec<InspectorSection>,
}

impl InspectorModel {
    /// 创建带标题的模型。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// 设置副标题。
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// 追加一个分组。
    pub fn push_section(&mut self, title: impl Into<String>) {
        self.sections.push(InspectorSection {
            title: title.into(),
            rows: Vec::new(),
        });
    }

    /// 向最后一个分组追加一行；没有分组时自动创建“详情”分组。
    pub fn push_row(&mut self, label: impl Into<String>, value: impl Into<String>) {
        if self.sections.is_empty() {
            self.push_section("详情");
        }
        if let Some(section) = self.sections.last_mut() {
            section.rows.push(InspectorRow {
                label: label.into(),
                value: value.into(),
            });
        }
    }

    /// 是否没有任何内容。
    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(|section| section.rows.is_empty())
    }

    /// 总行数。
    pub fn row_count(&self) -> usize {
        self.sections.iter().map(|section| section.rows.len()).sum()
    }

    /// 查找第一条匹配标签的值。
    pub fn value(&self, label: &str) -> Option<&str> {
        self.sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .find(|row| row.label == label)
            .map(|row| row.value.as_str())
    }
}

/// 由场景生成详情面板模型。
pub fn scene_inspector(scene: &Scene) -> InspectorModel {
    let stats = scene.stats();
    let mut model = InspectorModel::new(&scene.name).with_subtitle(format!(
        "{} · {} 节点 · {} 三角形",
        scene.source, stats.nodes, stats.triangles
    ));
    model.push_section("概览");
    model.push_row("来源", &scene.source);
    model.push_row(
        "坐标轴",
        match scene.up_axis {
            Axis::Y => "Y-up",
            Axis::Z => "Z-up",
        },
    );
    model.push_row("单位换算", format!("{:.4}", scene.unit_scale));
    model.push_row("节点", stats.nodes.to_string());
    model.push_row("网格", stats.meshes.to_string());
    model.push_row("材质", stats.materials.to_string());
    model.push_row("顶点", stats.vertices.to_string());
    model.push_row("三角形", stats.triangles.to_string());
    if stats.textures > 0 {
        model.push_row("贴图", stats.textures.to_string());
    }
    if stats.lights > 0 || stats.cameras > 0 {
        model.push_section("灯光与相机");
        if stats.lights > 0 {
            model.push_row("灯光", stats.lights.to_string());
        }
        if stats.cameras > 0 {
            model.push_row("相机", stats.cameras.to_string());
        }
    }
    if !scene.meshes.is_empty() {
        model.push_section("网格");
        for mesh in &scene.meshes {
            model.push_row(
                &mesh.name,
                format!(
                    "{} 顶点 · {} 三角形 · {} section",
                    mesh.vertex_count(),
                    mesh.triangle_count(),
                    mesh.sections.len()
                ),
            );
        }
    }
    if !scene.materials.is_empty() {
        model.push_section("材质");
        for material in &scene.materials {
            model.push_row(
                &material.name,
                format!(
                    "基色 #{:02X}{:02X}{:02X} · 金属 {:.2} · 粗糙 {:.2}{}",
                    encode_color(material.base_color[0]),
                    encode_color(material.base_color[1]),
                    encode_color(material.base_color[2]),
                    material.metallic,
                    material.roughness,
                    if material.textures.base_color.is_some() {
                        " · 有基础色贴图"
                    } else {
                        ""
                    }
                ),
            );
        }
    }
    model.push_section("校验");
    match scene.validate() {
        Ok(()) => model.push_row("状态", "通过"),
        Err(error) => model.push_row("状态", format!("失败：{error}")),
    }
    model
}

/// 把线性颜色分量编码为 0–255。
fn encode_color(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{MeshData, SceneNode};
    use crate::transform::Transform;

    /// 没有分组时 push_row 自动创建并累计行数。
    #[test]
    fn auto_creates_section() {
        let mut model = InspectorModel::new("测试");
        model.push_row("A", "1");
        model.push_row("B", "2");
        assert_eq!(model.sections.len(), 1);
        assert_eq!(model.row_count(), 2);
        assert_eq!(model.value("B"), Some("2"));
        assert!(!model.is_empty());
    }

    /// 场景详情包含概览、网格和校验行。
    #[test]
    fn builds_scene_inspector() {
        let mut scene = Scene::new("SM_Test");
        scene.source = "BNB".to_string();
        let mesh = scene.add_mesh(MeshData::new(
            "LOD0",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        ));
        scene.add_node(SceneNode {
            name: "Root".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh),
            materials: Vec::new(),
            visible: true,
            kind: crate::scene::NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        let model = scene_inspector(&scene);
        assert_eq!(model.title, "SM_Test");
        assert_eq!(model.value("来源"), Some("BNB"));
        assert_eq!(model.value("三角形"), Some("1"));
        assert_eq!(model.value("状态"), Some("通过"));
        assert!(model.row_count() >= 9);
    }
}
