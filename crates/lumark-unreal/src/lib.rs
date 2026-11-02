//! Unreal Engine 游戏资产支持：目录扫描、引擎版本探测、AES 密钥提取和 Zen 包读取。
//!
//! 本 crate 只做本地只读分析，不依赖 UI；容器读取复用 `lumark-archive`。

pub mod batch;
pub mod export_input;
pub mod game;
pub mod keys;
pub mod materials;
pub mod package;
pub mod raster;
pub mod scene;
pub mod skinning;
pub mod source;

pub use batch::{
    BatchExportConfig, BatchExportFormat, BatchExportReport, BatchFailure, BatchProgress,
    ExportedAsset, export_archive,
};
pub use export_input::{MeshExportBundle, build_mesh_export, export_materials};

pub use game::{
    ContainerInfo, ContainerKind, DetectedGame, GameScanError, UnrealEngine, scan_game,
};
pub use keys::{KeyExtraction, KeyStage, extract_key, extract_key_for_game, verify_container_key};
pub use materials::{
    MaterialResolveOptions, ResolvedMaterial, resolve_material_asset, resolve_material_slots,
};
pub use package::legacy::{
    LegacyExport, LegacyPackage, LegacyPackageSummary, LegacyPropertyHeader, is_legacy_package,
    parse_legacy_summary, read_legacy_property_header, read_legacy_texture,
};
pub use package::{
    AnimSequenceData, AssetKind, AssetMetadata, Bone, BulkDataEntry, DecodedTexture,
    LegacySkeletalMeshGeometry, LegacySkeletalMeshSummary, Mappings, MaterialAppearance,
    MaterialSlot, MaterialTextureKind, MaterialTextureRef, MaterialViewerData, MaterialViewerImage,
    MeshLod, MeshSection, MeshVertexAttributes, PackageError, PackageExport, PackageInfo,
    PropSchema, PropType, PropValue, SkeletalMeshPreview, Skeleton, SkinWeightStats, SkinWeights,
    StaticMesh, StructSchema, Texture2D, TextureMip, TexturePixels, TrackPose, UnrealPackage,
    UnversionedProperties, WorldActor, WorldSummary, ZenScriptObjects, is_material_class,
    material_parent_package, parse_package, read_anim_sequence, read_asset_metadata,
    read_legacy_anim_sequence, read_legacy_material_appearance, read_legacy_skeletal_mesh_geometry,
    read_legacy_skeletal_mesh_skeleton, read_legacy_skeletal_mesh_summary, read_legacy_skeleton,
    read_static_mesh, read_world_summary, read_zen_material_appearance, texture_references,
};
pub use raster::{
    MaterialPreviewTexture, render_material_ball, render_material_ball_orbit,
    render_skeletal_mesh_orbit, render_skeleton_orbit, render_static_mesh,
    render_static_mesh_orbit,
};
pub use scene::{
    UE_UNIT_SCALE, convert_mesh, scene_material, skeletal_mesh_scene, static_mesh_scene,
};
pub use skinning::{
    AnimationSample, BonePose, BoneRotation, animation_poses, sample_animation,
    skin_matrices_from_poses, skin_mesh, skin_mesh_animation, skin_mesh_pose,
};
pub use source::{
    AssetContainerKind, AssetInputs, AssetPackageRead, AssetSourceError, GameAssetEntry,
    GameAssetOptions, GameAssetSource, read_asset_package,
};
