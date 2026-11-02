//! 引擎无关资产导出。
//!
//! 输入是 [`ExportMesh`]（静态/骨骼网格 + 骨骼 + 蒙皮），可写出多种格式：
//! - ActorX PSK（本轮已实现）；
//! - glTF 2.0 binary、UEFormat uemodel、USDA 将在后续轮次补齐。
//!
//! 导出层不依赖任何引擎读取代码，方便查看器、批量导出和未来的 Unity 适配复用。

mod actorx;
mod gltf;
mod sidecar;
mod texture;
mod ueformat;
mod usda;

pub use actorx::{ExportBone, ExportMesh, ExportOptions, MeshSection, MeshVertex, write_psk};
pub use gltf::{ExportMaterial, ExportTexture, GltfExport, write_glb};
pub use sidecar::{MaterialSidecarOptions, material_sidecars, write_resources};
pub use texture::{TextureExportFormat, TextureExportOptions, encode_texture};
pub use ueformat::write_ueformat_model;
pub use usda::{ExportResource, UsdaOutput, write_usda};

/// 导出错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// 网格索引不是 3 的倍数。
    InvalidIndexCount,
    /// 索引引用越界顶点。
    IndexOutOfRange {
        /// 越界索引。
        index: u32,
        /// 顶点数。
        vertices: usize,
    },
    /// section 范围越界或三角形数非法。
    InvalidSection {
        /// section 索引。
        section: usize,
    },
    /// 骨骼父索引越界。
    InvalidBoneParent {
        /// 骨骼索引。
        bone: usize,
        /// 父索引。
        parent: i32,
    },
    /// 蒙皮数组长度与顶点数不一致。
    InvalidSkinLength,
    /// 序列化失败。
    Serialization(String),
    /// 贴图尺寸与像素数据不匹配。
    InvalidTexture(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIndexCount => write!(formatter, "网格索引数量不是 3 的倍数"),
            Self::IndexOutOfRange { index, vertices } => {
                write!(formatter, "索引 {index} 越界顶点数 {vertices}")
            }
            Self::InvalidSection { section } => write!(formatter, "section {section} 范围非法"),
            Self::InvalidBoneParent { bone, parent } => {
                write!(formatter, "骨骼 {bone} 的父索引 {parent} 越界")
            }
            Self::InvalidSkinLength => write!(formatter, "蒙皮数组长度与顶点数不一致"),
            Self::Serialization(error) => write!(formatter, "序列化失败：{error}"),
            Self::InvalidTexture(name) => write!(formatter, "贴图 {name} 尺寸与像素数据不匹配"),
        }
    }
}

impl std::error::Error for ExportError {}
