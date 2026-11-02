//! 引擎无关的三维场景抽象。
//!
//! 查看器、导出器和未来的 Unity 适配层都通过 [`Scene`] 交互；这里不引用任何
//! Unreal 类型，资产解析层负责把各自引擎的数据转换为 CPU 网格、材质与节点树。
//!
//! 坐标系约定：右手系、列主序矩阵；`up_axis` 与 `unit_scale` 记录源数据约定，
//! 渲染层按需转换到 Y-up、米制。

pub mod inspector;
pub mod scene;
pub mod transform;

pub use inspector::{InspectorModel, InspectorRow, InspectorSection, scene_inspector};
pub use scene::{
    Axis, Bounds, Camera, CameraProjection, Light, LightKind, MaterialData, MaterialHandle,
    MaterialTextures, MeshData, MeshHandle, MeshSection, NodeKind, Scene, SceneError, SceneNode,
    SceneNodeId, SceneProvider, SceneStats, TextureData,
};
pub use transform::Transform;
