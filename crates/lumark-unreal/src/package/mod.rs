//! Unreal 包解析入口：名称表、脚本对象和 UE5 Zen 包。
//!
//! 当前里程碑先提供包级元数据（导出对象、类、数据范围）和脚本对象解析，
//! 供资产查看器逐步接入；UE4.27 的旧式 `uasset` 读取器会和纹理/网格
//! 解析一起补充。

use std::io::Cursor;

/// UE 资产种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// 静态网格。
    StaticMesh,
    /// 骨骼网格。
    SkeletalMesh,
    /// 骨骼。
    Skeleton,
    /// 动画序列。
    AnimSequence,
    /// 动画蓝图。
    AnimBlueprint,
    /// 蓝图类。
    Blueprint,
    /// 材质。
    Material,
    /// 材质实例。
    MaterialInstance,
    /// 物理资产。
    PhysicsAsset,
    /// 纹理。
    Texture,
    /// 地图/关卡。
    Map,
    /// 数据表/数据资产。
    Data,
    /// 音频。
    Sound,
    /// 字体。
    Font,
    /// 粒子/特效。
    Vfx,
    /// 其它或未识别的资产。
    Other,
}

impl AssetKind {
    /// 返回界面展示名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::StaticMesh => "StaticMesh",
            Self::SkeletalMesh => "SkeletalMesh",
            Self::Skeleton => "Skeleton",
            Self::AnimSequence => "AnimSequence",
            Self::AnimBlueprint => "AnimBlueprint",
            Self::Blueprint => "Blueprint",
            Self::Material => "Material",
            Self::MaterialInstance => "MaterialInstance",
            Self::PhysicsAsset => "PhysicsAsset",
            Self::Texture => "Texture2D",
            Self::Map => "World",
            Self::Data => "DataAsset",
            Self::Sound => "SoundWave",
            Self::Font => "Font",
            Self::Vfx => "NiagaraSystem",
            Self::Other => "Asset",
        }
    }

    /// 根据类名或对象名推断资产种类。
    ///
    /// 精确类名优先于对象名前缀：游戏资产常见 `NanaF6_Hold` 这类没有
    /// `SK_`/`SM_` 前缀的命名，只有类名能判断真实类型。
    pub fn classify(class_name: &str, object_name: &str) -> Self {
        let class = class_name.to_ascii_lowercase();
        if class.ends_with("staticmesh") {
            return Self::StaticMesh;
        }
        if class.ends_with("skeletalmesh") {
            return Self::SkeletalMesh;
        }
        if class.contains("skeleton") {
            return Self::Skeleton;
        }
        if class.contains("animblueprint") || class.contains("animinstance") {
            return Self::AnimBlueprint;
        }
        if class.contains("animsequence")
            || class.contains("animmontage")
            || class.contains("animcomposite")
            || class.contains("levelsequence")
        {
            return Self::AnimSequence;
        }
        if class.contains("blueprint") || class.contains("userdefined") {
            return Self::Blueprint;
        }
        if class.contains("materialinstance") {
            return Self::MaterialInstance;
        }
        if class.contains("material") {
            return Self::Material;
        }
        if class.contains("physicsasset") {
            return Self::PhysicsAsset;
        }
        if class.contains("texture") {
            return Self::Texture;
        }
        if class.contains("niagara") || class.contains("particlesystem") {
            return Self::Vfx;
        }
        if class.contains("world") || class.contains("level") || class.contains("map") {
            return Self::Map;
        }
        if class.contains("datatable")
            || class.contains("dataasset")
            || class.contains("curvetable")
            || class.contains("stringtable")
        {
            return Self::Data;
        }
        if class.contains("sound") || class.contains("metasound") {
            return Self::Sound;
        }
        if class.contains("font") {
            return Self::Font;
        }
        let name = object_name.to_ascii_lowercase();
        if name.starts_with("sm_") || name.starts_with("s_") {
            Self::StaticMesh
        } else if name.starts_with("sk_") || name.starts_with("skm_") {
            Self::SkeletalMesh
        } else if name.contains("skeleton") {
            Self::Skeleton
        } else if name.starts_with("t_") || name.starts_with("tex_") {
            Self::Texture
        } else if name.starts_with("a_") || name.starts_with("as_") || name.starts_with("am_") {
            Self::AnimSequence
        } else if name.starts_with("abp_") {
            Self::AnimBlueprint
        } else if name.starts_with("bp_") {
            Self::Blueprint
        } else if name.starts_with("mi_") {
            Self::MaterialInstance
        } else if name.starts_with("m_") || name.starts_with("mat_") {
            Self::Material
        } else if name.starts_with("phys_") || name.starts_with("pa_") {
            Self::PhysicsAsset
        } else if name.starts_with("ns_") {
            Self::Vfx
        } else if name.starts_with("dt_") {
            Self::Data
        } else if name.starts_with('w') && name.contains("map") {
            Self::Map
        } else {
            Self::Other
        }
    }
}

/// 包内一个导出对象的元数据。
#[derive(Debug, Clone)]
pub struct PackageExport {
    /// 对象名称。
    pub name: String,
    /// 类名称（能解析时）。
    pub class_name: String,
    /// 外层对象在导出表中的索引。
    pub outer_export: Option<usize>,
    /// 序列化数据在包字节中的起始位置。
    pub data_offset: usize,
    /// 序列化数据长度。
    pub data_size: usize,
    /// 推断的资产种类。
    pub kind: AssetKind,
    /// 公共导出哈希（UE5 Zen；0 表示旧版本未序列化）。
    pub public_export_hash: u64,
}

/// 包内一个 bulk data（`.ubulk`/内联）条目。
#[derive(Debug, Clone, Copy, Default)]
pub struct BulkDataEntry {
    /// 在 bulk payload 中的序列化偏移。
    pub serial_offset: i64,
    /// 重复序列化偏移。
    pub duplicate_serial_offset: i64,
    /// 序列化长度。
    pub serial_size: i64,
    /// bulk data flags。
    pub flags: u32,
    /// cooked 索引。
    pub cooked_index: u8,
}

/// 包级元数据。
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// 包名。
    pub package_name: String,
    /// UE4 文件版本。
    pub file_version_ue4: i32,
    /// UE5 文件版本。
    pub file_version_ue5: i32,
    /// 探测到的 Zen 容器头版本。
    pub header_version: u8,
    /// 名称表。
    pub names: Vec<String>,
    /// Zen ImportMap（`FPackageObjectIndex` 原始 u64，按导入索引排列）。
    pub import_map: Vec<u64>,
    /// 被导入包的公共导出哈希表。
    pub imported_public_export_hashes: Vec<u64>,
    /// bulk data 映射。
    pub bulk_data: Vec<BulkDataEntry>,
    /// 导出对象。
    pub exports: Vec<PackageExport>,
}

/// 一个已解析的 UE 包（当前为 Zen 格式）。
pub struct UnrealPackage {
    info: PackageInfo,
    data: Vec<u8>,
}

impl UnrealPackage {
    /// 返回包元数据。
    pub fn info(&self) -> &PackageInfo {
        &self.info
    }

    /// 返回指定导出对象的序列化数据。
    pub fn export_data(&self, index: usize) -> Option<&[u8]> {
        let export = self.info.exports.get(index)?;
        self.data
            .get(export.data_offset..export.data_offset + export.data_size)
    }

    /// 返回包名称表。
    pub fn names(&self) -> &[String] {
        &self.info.names
    }

    /// 读取指定导出对象的无版本属性。
    pub fn read_export_properties(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<UnversionedProperties, PackageError> {
        let export = self
            .info
            .exports
            .get(index)
            .ok_or_else(|| invalid("导出索引越界"))?;
        let data = self
            .export_data(index)
            .ok_or_else(|| invalid("导出数据越界"))?;
        unversioned::read_properties(data, &self.info.names, mappings, &export.class_name)
    }

    /// 读取指定导出的 UTexture2D。
    pub fn read_texture2d(
        &self,
        index: usize,
        mappings: &Mappings,
        ubulk: Option<&[u8]>,
    ) -> Result<Texture2D, PackageError> {
        texture::read_texture2d(self, index, mappings, ubulk)
    }

    /// 读取指定导出的 USkeleton。
    pub fn read_skeleton(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<Skeleton, PackageError> {
        skeleton::read_skeleton(self, index, mappings)
    }

    /// 读取 SkeletalMesh 的参考骨架（不含几何/蒙皮）。
    pub fn read_skeletal_mesh_skeleton(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<Skeleton, PackageError> {
        skeleton::read_skeletal_mesh_skeleton(self, index, mappings)
    }

    /// 读取 SkeletalMesh 的参考骨架、材质槽和 LOD 数量。
    pub fn read_skeletal_mesh_preview(
        &self,
        index: usize,
        mappings: &Mappings,
        ubulk: Option<&[u8]>,
    ) -> Result<skeleton::SkeletalMeshPreview, PackageError> {
        skeleton::read_skeletal_mesh_preview(self, index, mappings, ubulk)
    }

    /// 读取 AnimSequence 的 PerTrackCompression 数据。
    pub fn read_anim_sequence(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<animation::AnimSequenceData, PackageError> {
        animation::read_anim_sequence(self, index, mappings)
    }

    /// 读取资产元数据。
    pub fn read_asset_metadata(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<AssetMetadata, PackageError> {
        asset::read_asset_metadata(self, index, mappings)
    }

    /// 读取 StaticMesh 的首个 LOD。
    pub fn read_static_mesh(
        &self,
        index: usize,
        mappings: &Mappings,
    ) -> Result<StaticMesh, PackageError> {
        mesh::read_static_mesh(self, index, mappings)
    }

    /// 读取 StaticMesh 的首个 LOD，并允许传入容器 `.ubulk` 载荷。
    pub fn read_static_mesh_with_bulk(
        &self,
        index: usize,
        mappings: &Mappings,
        ubulk: Option<&[u8]>,
    ) -> Result<StaticMesh, PackageError> {
        mesh::read_static_mesh_with_bulk(self, index, mappings, ubulk)
    }

    /// 读取 `.umap` 的 World/Level 摘要。
    pub fn read_world_summary(&self, mappings: &Mappings) -> Result<WorldSummary, PackageError> {
        world::read_world_summary(self, mappings)
    }

    /// 返回主资产（第一个导出）的种类。
    pub fn primary_kind(&self) -> AssetKind {
        self.info
            .exports
            .first()
            .map(|export| export.kind)
            .unwrap_or(AssetKind::Other)
    }
}

/// 包解析错误。
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    /// 数据布局无效。
    #[error("UE 包数据无效：{0}")]
    Invalid(String),
    /// 版本或格式暂不支持。
    #[error("不支持的 UE 包：{0}")]
    Unsupported(String),
    /// IoStore 读取失败。
    #[error("IoStore 读取失败：{0}")]
    Archive(#[from] lumark_archive::ArchiveError),
    /// 文件读取失败。
    #[error("读取映射文件失败：{0}")]
    Io(#[from] std::io::Error),
}

/// 构造无效包错误。
pub(crate) fn invalid(message: impl Into<String>) -> PackageError {
    PackageError::Invalid(message.into())
}

/// 判断导出类名是否属于材质资产。
///
/// 材质本身没有可以直接光栅化的几何/纹理，详情面板展示引用资源和属性；
/// 该判断用于给 `Material`/`MaterialInstance*`/`MaterialFunction` 等类提供
/// 专门的预览信息，而不是落入“暂不支持”。
pub fn is_material_class(class_name: &str) -> bool {
    let class = class_name.to_ascii_lowercase();
    matches!(
        class.as_str(),
        "material"
            | "materialinstance"
            | "materialinstanceconstant"
            | "materialinstancedynamic"
            | "materialinterface"
            | "materialfunction"
            | "materialfunctioninstance"
    ) || class.starts_with("materialexpression")
}

mod animation;
mod asset;
mod bulk;
pub mod legacy;
mod legacy_animation;
mod legacy_skeleton;
mod mappings;
mod material;
mod mesh;
mod name_map;
pub mod script_objects;
mod skeleton;
mod texture;
mod unversioned;
mod world;
mod zen;

pub use animation::{AnimSequenceData, TrackPose, read_anim_sequence};
pub use asset::{AssetMetadata, read_asset_metadata};
pub use legacy::{
    LegacyExport, LegacyPackage, LegacyPackageSummary, LegacyPropertyHeader, is_legacy_package,
    parse_legacy_summary, read_legacy_property_header, read_legacy_texture,
};
pub use legacy_animation::read_legacy_anim_sequence;
pub use legacy_skeleton::{
    LegacySkeletalMeshGeometry, LegacySkeletalMeshSummary, read_legacy_skeletal_mesh_geometry,
    read_legacy_skeletal_mesh_skeleton, read_legacy_skeletal_mesh_summary, read_legacy_skeleton,
};
pub use mappings::{Mappings, PropSchema, PropType, StructSchema};
pub use material::{
    MaterialAppearance, MaterialTextureInfo, MaterialTextureKind, MaterialTextureRef,
    MaterialViewerData, MaterialViewerImage, material_parent_package,
    read_legacy_material_appearance, read_zen_material_appearance, texture_references,
};
/// 材质属性解析辅助函数：供 `material_graph` 等 crate 内模块复用。
pub(crate) use material::{
    blend_mode_name, blend_mode_property, is_base_color_name, is_overlay_name, parameter_array,
    parameter_entry_color4, parameter_entry_name, parameter_entry_scalar, shading_model_name,
    two_sided_property,
};
pub use mesh::{
    MaterialSlot, MeshLod, MeshSection, MeshVertexAttributes, StaticMesh, read_static_mesh,
    read_static_mesh_with_bulk,
};
pub use script_objects::{ScriptObject, ZenScriptObjects};
pub use skeleton::{
    Bone, SkeletalMeshPreview, Skeleton, SkinWeightStats, SkinWeights, read_skeleton,
};
pub use texture::{
    DecodedTexture, Texture2D, TextureMip, TexturePixels, mip_level_for_limit, read_texture2d,
};
pub use unversioned::{PropValue, UnversionedProperties};
pub use world::{WorldActor, WorldSummary, read_world_summary};
pub use zen::parse_package;

/// 从字节游标读取指定长度，越界返回可读错误。
pub(crate) fn read_bytes<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    length: usize,
) -> Result<&'a [u8], PackageError> {
    let start = cursor.position() as usize;
    let end = start
        .checked_add(length)
        .ok_or_else(|| invalid("读取长度溢出"))?;
    let data = cursor.get_ref();
    let slice = data
        .get(start..end)
        .ok_or_else(|| invalid("包数据被截断"))?;
    cursor.set_position(end as u64);
    Ok(slice)
}

/// 读取 u32。
pub(crate) fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, PackageError> {
    Ok(u32::from_le_bytes(
        read_bytes(cursor, 4)?.try_into().unwrap_or_default(),
    ))
}

/// 读取 i32。
pub(crate) fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32, PackageError> {
    Ok(i32::from_le_bytes(
        read_bytes(cursor, 4)?.try_into().unwrap_or_default(),
    ))
}

/// 读取 u64。
pub(crate) fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, PackageError> {
    Ok(u64::from_le_bytes(
        read_bytes(cursor, 8)?.try_into().unwrap_or_default(),
    ))
}

/// 读取 i64。
pub(crate) fn read_i64(cursor: &mut Cursor<&[u8]>) -> Result<i64, PackageError> {
    Ok(i64::from_le_bytes(
        read_bytes(cursor, 8)?.try_into().unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{AssetKind, is_material_class};

    /// 材质相关类名应被识别为材质资产。
    #[test]
    fn recognizes_material_classes() {
        for class in [
            "Material",
            "MaterialInstanceConstant",
            "MaterialInstanceDynamic",
            "MaterialFunction",
            "MaterialExpressionTextureSample",
        ] {
            assert!(is_material_class(class), "{class} 应识别为材质类");
        }
        for class in ["StaticMesh", "Texture2D", "AnimSequence", "BluePrint"] {
            assert!(!is_material_class(class), "{class} 不应识别为材质类");
        }
    }

    /// 游戏资产按类名区分类型；无前缀命名依赖类名而不是对象名。
    #[test]
    fn classifies_asset_kinds_by_class_name() {
        let cases = [
            ("StaticMesh", "NanaF6_2", AssetKind::StaticMesh),
            ("SkeletalMesh", "NanaF6_Hold", AssetKind::SkeletalMesh),
            ("Skeleton", "NanaF6_Skeleton", AssetKind::Skeleton),
            ("AnimSequence", "Idle", AssetKind::AnimSequence),
            (
                "AnimBlueprintGeneratedClass",
                "ABP_Nana",
                AssetKind::AnimBlueprint,
            ),
            ("BlueprintGeneratedClass", "BP_Tool", AssetKind::Blueprint),
            ("Material", "M_Cloth", AssetKind::Material),
            (
                "MaterialInstanceConstant",
                "MI_Cloth",
                AssetKind::MaterialInstance,
            ),
            ("PhysicsAsset", "PHYS_Nana", AssetKind::PhysicsAsset),
            ("Texture2D", "NanaF6_2_D", AssetKind::Texture),
            ("World", "GinkgoTree", AssetKind::Map),
            ("DataTable", "DT_Items", AssetKind::Data),
            ("SoundWave", "S_Click", AssetKind::Sound),
            ("Font", "F_UI", AssetKind::Font),
            ("NiagaraSystem", "NS_Fire", AssetKind::Vfx),
            ("SomethingUnknown", "NanaF6_Hold", AssetKind::Other),
        ];
        for (class, name, expected) in cases {
            assert_eq!(
                AssetKind::classify(class, name),
                expected,
                "{class}/{name} 分类错误"
            );
        }
        // 没有类名时保留对象名前缀回退。
        assert_eq!(AssetKind::classify("", "SM_Box"), AssetKind::StaticMesh);
        assert_eq!(
            AssetKind::classify("", "SK_Chicken"),
            AssetKind::SkeletalMesh
        );
    }
}
