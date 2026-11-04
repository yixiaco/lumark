//! 场景节点树、网格、材质、相机和灯光。

use std::collections::HashSet;

use crate::transform::Transform;

/// 坐标轴约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Y 轴向上（glTF/Unity 常见）。
    Y,
    /// Z 轴向上（Unreal 常见）。
    Z,
}

/// 轴对齐包围盒。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// 最小角。
    pub min: [f32; 3],
    /// 最大角。
    pub max: [f32; 3],
}

impl Default for Bounds {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Bounds {
    /// 空包围盒（min = +∞，max = -∞）。
    pub const EMPTY: Self = Self {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };

    /// 由最小/最大角构造。
    pub fn new(min: [f32; 3], max: [f32; 3]) -> Self {
        Self {
            min: [min[0].min(max[0]), min[1].min(max[1]), min[2].min(max[2])],
            max: [min[0].max(max[0]), min[1].max(max[1]), min[2].max(max[2])],
        }
    }

    /// 是否为空包围盒。
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }

    /// 包含一个点。
    pub fn expand_point(&mut self, point: [f32; 3]) {
        for (axis, coordinate) in point.iter().enumerate() {
            self.min[axis] = self.min[axis].min(*coordinate);
            self.max[axis] = self.max[axis].max(*coordinate);
        }
    }

    /// 合并另一个包围盒。
    pub fn union(&mut self, other: &Bounds) {
        if other.is_empty() {
            return;
        }
        self.expand_point(other.min);
        self.expand_point(other.max);
    }

    /// 中心点；空包围盒返回原点。
    pub fn center(&self) -> [f32; 3] {
        if self.is_empty() {
            return [0.0; 3];
        }
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// 尺寸；空包围盒返回零。
    pub fn size(&self) -> [f32; 3] {
        if self.is_empty() {
            return [0.0; 3];
        }
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    /// 最长边长度。
    pub fn extent(&self) -> f32 {
        let size = self.size();
        size[0].max(size[1]).max(size[2])
    }
}

/// 8 位 RGBA 纹理数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureData {
    /// 贴图名。
    pub name: String,
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// RGBA8 像素。
    pub rgba: Vec<u8>,
}

impl TextureData {
    /// 校验尺寸与字节数一致。
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.rgba.len() >= self.width as usize * self.height as usize * 4
    }
}

/// 材质使用的贴图集合。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialTextures {
    /// 基础色。
    pub base_color: Option<TextureData>,
    /// 法线。
    pub normal: Option<TextureData>,
    /// 金属/粗糙度（ORM）。
    pub metallic_roughness: Option<TextureData>,
    /// 自发光。
    pub emissive: Option<TextureData>,
    /// 环境光遮蔽（FModel `M`/`AEM`/`AO` 参数）。
    pub occlusion: Option<TextureData>,
}

/// 金属/粗糙度贴图的通道布局。
///
/// UE 资产的打包习惯并不统一：多数 PBR 材质用 ORM（R=遮蔽、G=粗糙度、
/// B=金属度），部分角色/武器材质则用高光遮罩（FModel 的 specular mask：
/// G=金属度、B=粗糙度）。布局随材质数据一起传入渲染层，避免在着色器里
/// 猜通道含义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrmLayout {
    /// R=环境光遮蔽、G=粗糙度、B=金属度（默认）。
    #[default]
    AoRoughMetal,
    /// R=遮罩（暂未参与着色）、G=金属度、B=粗糙度。
    SpecularMask,
}

/// 近似 PBR 材质。
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialData {
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
    /// 贴图。
    pub textures: MaterialTextures,
    /// 是否双面。
    pub two_sided: bool,
    /// 金属/粗糙度贴图的通道布局。
    pub orm_layout: OrmLayout,
    /// alpha 裁剪阈值；0 表示不裁剪（Opaque 材质），0.5 对应 Masked 材质。
    pub alpha_cutoff: f32,
    /// 是否半透明混合（Translucent/Additive 等）；此类材质按深度排序后混合。
    pub translucent: bool,
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            base_color: [0.72, 0.72, 0.72, 1.0],
            metallic: 0.0,
            roughness: 0.55,
            emissive: [0.0; 3],
            textures: MaterialTextures::default(),
            two_sided: false,
            orm_layout: OrmLayout::AoRoughMetal,
            alpha_cutoff: 0.0,
            translucent: false,
        }
    }
}

/// 网格 section。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshSection {
    /// 材质槽索引。
    pub material_index: usize,
    /// 在索引缓冲中的起始索引。
    pub first_index: u32,
    /// 索引数量（3 的倍数）。
    pub index_count: u32,
}

/// 网格蒙皮数据（LBS，最多 4 影响/顶点）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshSkin {
    /// 关节对应的场景节点（顺序与 `inverse_bind_matrices` 一致）。
    pub joints: Vec<SceneNodeId>,
    /// 每顶点的关节索引（与 `joint_weights` 对齐）。
    pub joint_indices: Vec<[u16; 4]>,
    /// 每顶点的关节权重（0–1，渲染前会归一化）。
    pub joint_weights: Vec<[f32; 4]>,
    /// 关节的 inverse bind 矩阵（列主序 4×4）。
    pub inverse_bind_matrices: Vec<[f32; 16]>,
    /// 姿态覆盖：每关节最终蒙皮矩阵（列主序）；为空时使用关节节点世界变换 × inverse bind。
    pub pose: Option<Vec<[f32; 16]>>,
}

impl MeshSkin {
    /// 是否包含有效的逐顶点影响。
    pub fn is_valid(&self) -> bool {
        !self.joint_indices.is_empty()
            && self.joint_indices.len() == self.joint_weights.len()
            && self.inverse_bind_matrices.len() == self.joints.len()
    }
}

/// 引擎无关的 CPU 网格。
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    /// 网格名。
    pub name: String,
    /// 顶点位置。
    pub positions: Vec<[f32; 3]>,
    /// 三角形索引。
    pub indices: Vec<u32>,
    /// 法线（可为空）。
    pub normals: Vec<[f32; 3]>,
    /// 切线 xyz + 副切线符号（可为空）。
    pub tangents: Vec<[f32; 4]>,
    /// 各 UV 通道（可为空）。
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// 顶点颜色（可为空）。
    pub colors: Vec<[u8; 4]>,
    /// section。
    pub sections: Vec<MeshSection>,
    /// 局部包围盒。
    pub bounds: Bounds,
    /// 蒙皮数据（静态网格为 `None`）。
    pub skin: Option<MeshSkin>,
}

impl MeshData {
    /// 由位置/索引构造，并计算包围盒。
    pub fn new(name: impl Into<String>, positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        let mut bounds = Bounds::EMPTY;
        for position in &positions {
            bounds.expand_point(*position);
        }
        Self {
            name: name.into(),
            positions,
            indices,
            bounds,
            ..Self::default()
        }
    }

    /// 三角形数量。
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// 顶点数量。
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// 属性数量是否与顶点数一致（空属性视为合法）。
    pub fn attributes_are_consistent(&self) -> bool {
        let vertices = self.positions.len();
        (self.normals.is_empty() || self.normals.len() == vertices)
            && (self.tangents.is_empty() || self.tangents.len() == vertices)
            && (self.colors.is_empty() || self.colors.len() == vertices)
            && self.uvs.iter().all(|channel| channel.len() == vertices)
    }
}

/// 网格句柄。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshHandle(pub usize);

/// 材质句柄。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialHandle(pub usize);

/// 节点句柄。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SceneNodeId(pub usize);

/// 节点种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// 空节点/分组。
    Empty,
    /// 静态网格。
    StaticMesh,
    /// 骨骼网格。
    SkeletalMesh,
    /// 光源。
    Light,
    /// 相机。
    Camera,
    /// 其它引擎对象。
    Other(String),
}

/// 场景节点。
#[derive(Debug, Clone)]
pub struct SceneNode {
    /// 节点名。
    pub name: String,
    /// 父节点。
    pub parent: Option<SceneNodeId>,
    /// 局部变换。
    pub transform: Transform,
    /// 网格。
    pub mesh: Option<MeshHandle>,
    /// 材质槽到材质句柄的映射。
    pub materials: Vec<Option<MaterialHandle>>,
    /// 是否可见。
    pub visible: bool,
    /// 节点种类。
    pub kind: NodeKind,
    /// 附加信息行。
    pub metadata: Vec<(String, String)>,
}

/// 相机投影。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraProjection {
    /// 透视投影。
    Perspective {
        /// 垂直视场角（弧度）。
        fov_y_radians: f32,
        /// 近裁面。
        near: f32,
        /// 远裁面。
        far: f32,
    },
    /// 正交投影。
    Orthographic {
        /// 垂直尺寸。
        height: f32,
        /// 近裁面。
        near: f32,
        /// 远裁面。
        far: f32,
    },
}

impl Default for CameraProjection {
    fn default() -> Self {
        Self::Perspective {
            fov_y_radians: 45.0_f32.to_radians(),
            near: 0.1,
            far: 10_000.0,
        }
    }
}

/// 相机。
#[derive(Debug, Clone)]
pub struct Camera {
    /// 名称。
    pub name: String,
    /// 世界/局部变换。
    pub transform: Transform,
    /// 投影。
    pub projection: CameraProjection,
}

/// 灯光种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    /// 方向光。
    Directional,
    /// 点光源。
    Point,
    /// 聚光灯。
    Spot,
    /// 环境光。
    Ambient,
}

/// 灯光。
#[derive(Debug, Clone)]
pub struct Light {
    /// 名称。
    pub name: String,
    /// 种类。
    pub kind: LightKind,
    /// 颜色（线性 RGB）。
    pub color: [f32; 3],
    /// 强度。
    pub intensity: f32,
    /// 变换（方向光使用旋转方向）。
    pub transform: Transform,
    /// 影响半径；方向光忽略。
    pub range: f32,
}

/// 场景统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneStats {
    /// 节点数。
    pub nodes: usize,
    /// 网格数。
    pub meshes: usize,
    /// 材质数。
    pub materials: usize,
    /// 顶点数。
    pub vertices: usize,
    /// 三角形数。
    pub triangles: usize,
    /// 贴图数。
    pub textures: usize,
    /// 光源数。
    pub lights: usize,
    /// 相机数。
    pub cameras: usize,
}

/// 场景错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    /// 节点父级索引越界。
    InvalidParent {
        /// 节点索引。
        node: usize,
        /// 父节点索引。
        parent: usize,
    },
    /// 节点层级存在环。
    CyclicHierarchy {
        /// 节点索引。
        node: usize,
    },
    /// 网格引用越界。
    InvalidMesh {
        /// 节点索引。
        node: usize,
        /// 网格索引。
        mesh: usize,
    },
    /// 材质引用越界。
    InvalidMaterial {
        /// 节点索引。
        node: usize,
        /// 材质索引。
        material: usize,
    },
    /// 变换包含非法数值。
    InvalidTransform {
        /// 节点索引。
        node: usize,
    },
    /// 网格索引越界。
    InvalidIndices {
        /// 网格索引。
        mesh: usize,
    },
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParent { node, parent } => {
                write!(formatter, "节点 {node} 的父节点 {parent} 越界")
            }
            Self::CyclicHierarchy { node } => write!(formatter, "节点 {node} 的层级存在环"),
            Self::InvalidMesh { node, mesh } => write!(formatter, "节点 {node} 的网格 {mesh} 越界"),
            Self::InvalidMaterial { node, material } => {
                write!(formatter, "节点 {node} 的材质 {material} 越界")
            }
            Self::InvalidTransform { node } => write!(formatter, "节点 {node} 的变换包含非法数值"),
            Self::InvalidIndices { mesh } => {
                write!(
                    formatter,
                    "网格 {mesh} 的索引引用越界顶点或长度不是 3 的倍数"
                )
            }
        }
    }
}

impl std::error::Error for SceneError {}

/// 引擎无关场景。
#[derive(Debug, Clone)]
pub struct Scene {
    /// 场景名。
    pub name: String,
    /// 来源描述（引擎/游戏/资产路径），用于详情面板。
    pub source: String,
    /// 坐标轴约定。
    pub up_axis: Axis,
    /// 单位换算系数（例如 UE 厘米 → 米为 0.01）。
    pub unit_scale: f32,
    /// 节点。
    pub nodes: Vec<SceneNode>,
    /// 网格。
    pub meshes: Vec<MeshData>,
    /// 材质。
    pub materials: Vec<MaterialData>,
    /// 相机。
    pub cameras: Vec<Camera>,
    /// 灯光。
    pub lights: Vec<Light>,
}

impl Scene {
    /// 创建空场景。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: String::new(),
            up_axis: Axis::Z,
            unit_scale: 0.01,
            nodes: Vec::new(),
            meshes: Vec::new(),
            materials: Vec::new(),
            cameras: Vec::new(),
            lights: Vec::new(),
        }
    }

    /// 添加节点，返回句柄。
    pub fn add_node(&mut self, node: SceneNode) -> SceneNodeId {
        let id = SceneNodeId(self.nodes.len());
        self.nodes.push(node);
        id
    }

    /// 添加网格，返回句柄。
    pub fn add_mesh(&mut self, mesh: MeshData) -> MeshHandle {
        let handle = MeshHandle(self.meshes.len());
        self.meshes.push(mesh);
        handle
    }

    /// 添加材质，返回句柄。
    pub fn add_material(&mut self, material: MaterialData) -> MaterialHandle {
        let handle = MaterialHandle(self.materials.len());
        self.materials.push(material);
        handle
    }

    /// 取节点。
    pub fn node(&self, id: SceneNodeId) -> Option<&SceneNode> {
        self.nodes.get(id.0)
    }

    /// 计算节点的世界变换（父级链从根到叶组合）。
    pub fn world_transform(&self, id: SceneNodeId) -> Option<Transform> {
        let mut chain = Vec::new();
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = self.nodes.get(node_id.0)?;
            chain.push(node.transform);
            current = node.parent;
            if chain.len() > self.nodes.len() {
                return None;
            }
        }
        let mut world = Transform::IDENTITY;
        for local in chain.iter().rev() {
            world = Transform::combine(&world, local);
        }
        Some(world)
    }

    /// 计算场景包围盒（已应用节点世界变换）。
    pub fn compute_bounds(&self) -> Bounds {
        let mut bounds = Bounds::EMPTY;
        for (index, node) in self.nodes.iter().enumerate() {
            if !node.visible {
                continue;
            }
            let Some(mesh_handle) = node.mesh else {
                continue;
            };
            let Some(mesh) = self.meshes.get(mesh_handle.0) else {
                continue;
            };
            let Some(world) = self.world_transform(SceneNodeId(index)) else {
                continue;
            };
            for position in &mesh.positions {
                bounds.expand_point(world.transform_point(*position));
            }
        }
        bounds
    }

    /// 统计信息。
    pub fn stats(&self) -> SceneStats {
        let mut textures = 0;
        for material in &self.materials {
            let textures_of = &material.textures;
            textures += usize::from(textures_of.base_color.is_some())
                + usize::from(textures_of.normal.is_some())
                + usize::from(textures_of.metallic_roughness.is_some())
                + usize::from(textures_of.emissive.is_some());
        }
        SceneStats {
            nodes: self.nodes.len(),
            meshes: self.meshes.len(),
            materials: self.materials.len(),
            vertices: self.meshes.iter().map(MeshData::vertex_count).sum(),
            triangles: self.meshes.iter().map(MeshData::triangle_count).sum(),
            textures,
            lights: self.lights.len(),
            cameras: self.cameras.len(),
        }
    }

    /// 校验节点层级、引用和索引。
    pub fn validate(&self) -> Result<(), SceneError> {
        for (index, node) in self.nodes.iter().enumerate() {
            if !node.transform.is_finite() {
                return Err(SceneError::InvalidTransform { node: index });
            }
            if let Some(parent) = node.parent
                && parent.0 >= self.nodes.len()
            {
                return Err(SceneError::InvalidParent {
                    node: index,
                    parent: parent.0,
                });
            }
            if let Some(mesh) = node.mesh
                && mesh.0 >= self.meshes.len()
            {
                return Err(SceneError::InvalidMesh {
                    node: index,
                    mesh: mesh.0,
                });
            }
            if let Some(material) = node
                .materials
                .iter()
                .flatten()
                .find(|material| material.0 >= self.materials.len())
            {
                return Err(SceneError::InvalidMaterial {
                    node: index,
                    material: material.0,
                });
            }
        }
        // 检测层级环。
        for index in 0..self.nodes.len() {
            let mut seen = HashSet::new();
            let mut current = Some(SceneNodeId(index));
            while let Some(id) = current {
                if !seen.insert(id) {
                    return Err(SceneError::CyclicHierarchy { node: index });
                }
                current = self.nodes.get(id.0).and_then(|node| node.parent);
            }
        }
        for (index, mesh) in self.meshes.iter().enumerate() {
            if !mesh.indices.len().is_multiple_of(3)
                || mesh
                    .indices
                    .iter()
                    .any(|index| *index as usize >= mesh.positions.len())
            {
                return Err(SceneError::InvalidIndices { mesh: index });
            }
        }
        Ok(())
    }
}

/// 场景提供者：各引擎适配层实现该 trait 把资产转换为引擎无关场景。
pub trait SceneProvider {
    /// 错误类型。
    type Error: std::error::Error + Send + Sync + 'static;

    /// 构建场景。
    fn scene(&self) -> Result<Scene, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个带单个三角形的网格。
    fn triangle_mesh(name: &str) -> MeshData {
        MeshData::new(
            name,
            vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
            vec![0, 1, 2],
        )
    }

    /// 构造一个静态网格节点。
    fn mesh_node(name: &str, mesh: MeshHandle, transform: Transform) -> SceneNode {
        SceneNode {
            name: name.to_string(),
            parent: None,
            transform,
            mesh: Some(mesh),
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        }
    }

    /// 世界变换沿父链组合。
    #[test]
    fn composes_world_transform_from_hierarchy() {
        let mut scene = Scene::new("Test");
        let root = scene.add_node(SceneNode {
            name: "Root".to_string(),
            parent: None,
            transform: Transform::from_translation([0.0, 0.0, 10.0]),
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Empty,
            metadata: Vec::new(),
        });
        let child = scene.add_node(SceneNode {
            name: "Child".to_string(),
            parent: Some(root),
            transform: Transform::from_translation([5.0, 0.0, 0.0]),
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Empty,
            metadata: Vec::new(),
        });
        let world = scene.world_transform(child).expect("世界变换");
        assert_eq!(world.translation, [5.0, 0.0, 10.0]);
    }

    /// 包围盒应用节点世界变换，隐藏节点被忽略。
    #[test]
    fn computes_bounds_with_world_transform() {
        let mut scene = Scene::new("Test");
        let mesh = scene.add_mesh(triangle_mesh("Tri"));
        let first = scene.add_node(mesh_node(
            "A",
            mesh,
            Transform::from_translation([10.0, 0.0, 0.0]),
        ));
        let _ = first;
        scene.add_node(mesh_node(
            "B",
            mesh,
            Transform::from_translation([-10.0, 0.0, 0.0]),
        ));
        let bounds = scene.compute_bounds();
        assert_eq!(bounds.min, [-10.0, 0.0, 0.0]);
        assert_eq!(bounds.max, [12.0, 2.0, 0.0]);
        assert_eq!(bounds.extent(), 22.0);
    }

    /// 统计汇总网格、材质和贴图。
    #[test]
    fn computes_scene_stats() {
        let mut scene = Scene::new("Test");
        let mesh = scene.add_mesh(triangle_mesh("Tri"));
        let mut material = MaterialData::default();
        material.textures.base_color = Some(TextureData {
            name: "T_Test".to_string(),
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        let material = scene.add_material(material);
        scene.add_node(SceneNode {
            name: "A".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh),
            materials: vec![Some(material)],
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        let stats = scene.stats();
        assert_eq!(stats.nodes, 1);
        assert_eq!(stats.meshes, 1);
        assert_eq!(stats.triangles, 1);
        assert_eq!(stats.vertices, 3);
        assert_eq!(stats.materials, 1);
        assert_eq!(stats.textures, 1);
    }

    /// 校验能发现越界父级、非法索引和层级环。
    #[test]
    fn validates_hierarchy_and_indices() {
        let mut scene = Scene::new("Test");
        let node = scene.add_node(SceneNode {
            name: "BadParent".to_string(),
            parent: Some(SceneNodeId(99)),
            transform: Transform::IDENTITY,
            mesh: None,
            materials: Vec::new(),
            visible: true,
            kind: NodeKind::Empty,
            metadata: Vec::new(),
        });
        assert!(matches!(
            scene.validate(),
            Err(SceneError::InvalidParent { .. })
        ));
        scene.nodes[node.0].parent = None;

        let mut mesh = triangle_mesh("Bad");
        mesh.indices = vec![0, 1, 9];
        scene.add_mesh(mesh);
        assert!(matches!(
            scene.validate(),
            Err(SceneError::InvalidIndices { .. })
        ));

        let mut clean = Scene::new("Clean");
        let mesh = clean.add_mesh(triangle_mesh("OK"));
        let a = clean.add_node(mesh_node("A", mesh, Transform::IDENTITY));
        clean.nodes[a.0].parent = Some(a);
        assert!(matches!(
            clean.validate(),
            Err(SceneError::CyclicHierarchy { .. })
        ));
    }
}
