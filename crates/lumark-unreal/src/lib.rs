//! Unreal Engine 游戏资产支持：目录扫描、引擎版本探测、AES 密钥提取和 Zen 包读取。
//!
//! 本 crate 只做本地只读分析，不依赖 UI；容器读取复用 `lumark-archive`。

pub mod asset_export;
pub mod batch;
pub mod export_input;
pub mod game;
mod imports;
pub mod keys;
mod material_graph;
mod material_params;
pub mod materials;
pub mod package;
pub mod properties_json;
pub mod provider;
pub mod raster;
pub mod scene;
pub mod skinning;
pub mod source;
pub mod world_scene;

pub use asset_export::{
    AssetExportConfig, AssetExportProgress, AssetExportReport, export_asset, write_report,
};
pub use batch::{
    BatchExportConfig, BatchExportFormat, BatchExportReport, BatchFailure, BatchProgress,
    ExportedAsset, export_archive, export_archive_cancellable,
};
pub use export_input::{MeshExportBundle, build_mesh_export, export_materials};

pub use game::{
    ContainerInfo, ContainerKind, DetectedGame, GameScanError, UnrealEngine, scan_game,
};
pub use keys::{KeyExtraction, KeyStage, extract_key, extract_key_for_game, verify_container_key};
pub use materials::{
    MaterialResolveOptions, ResolvedMaterial, TextureCache, resolve_material_asset,
    resolve_material_slots, resolve_material_slots_exact, resolve_static_materials, texture_cache,
};
pub use package::legacy::{
    LegacyExport, LegacyPackage, LegacyPackageSummary, LegacyPropertyHeader, is_legacy_package,
    parse_legacy_summary, read_legacy_property_header, read_legacy_texture,
};
pub use package::{
    AnimSequenceData, AssetKind, AssetMetadata, Bone, BulkDataEntry, DecodedTexture,
    LegacySkeletalMeshGeometry, LegacySkeletalMeshSummary, Mappings, MaterialAppearance,
    MaterialSlot, MaterialTextureInfo, MaterialTextureKind, MaterialTextureRef, MaterialViewerData,
    MaterialViewerImage, MeshLod, MeshSection, MeshVertexAttributes, PackageError, PackageExport,
    PackageInfo, PropSchema, PropType, PropValue, SkeletalMeshPreview, Skeleton, SkinWeightStats,
    SkinWeights, StaticMesh, StructSchema, Texture2D, TextureMip, TexturePixels, TrackPose,
    UnrealPackage, UnversionedProperties, WorldActor, WorldSummary, ZenScriptObjects,
    is_material_class, material_parent_package, mip_level_for_limit, parse_package,
    read_anim_sequence, read_asset_metadata, read_legacy_anim_sequence,
    read_legacy_material_appearance, read_legacy_skeletal_mesh_geometry,
    read_legacy_skeletal_mesh_skeleton, read_legacy_skeletal_mesh_summary, read_legacy_skeleton,
    read_static_mesh, read_world_summary, read_zen_material_appearance, texture_references,
};
pub use properties_json::{legacy_properties_json, properties_to_json, zen_properties_json};
pub use provider::{
    AnimationInfo, LoadedScene, LodPolicy, SceneLoadOptions, SceneLoadReport, SceneLoadStage,
    SceneRequest, load_scene, lod_index,
};
pub use raster::{
    MaterialBallSettings, MaterialPreviewTexture, render_material_ball, render_material_ball_orbit,
    render_skeletal_mesh_orbit, render_skeleton_orbit, render_static_mesh,
    render_static_mesh_orbit,
};
pub use scene::{
    UE_UNIT_SCALE, add_material, apply_animation, apply_skin_pose, convert_mesh, scene_material,
    skeletal_mesh_scene, static_mesh_scene,
};
pub use skinning::{
    AnimationSample, BonePose, BoneRotation, animation_poses, sample_animation,
    skin_matrices_from_poses, skin_mesh, skin_mesh_animation, skin_mesh_pose,
};
pub use source::{
    AssetClassSummary, AssetContainerKind, AssetInputs, AssetPackageRead, AssetSourceError,
    GameAssetEntry, GameAssetOptions, GameAssetSource, read_asset_package,
};
pub use world_scene::{WorldSceneLoad, WorldSceneOptions, WorldSceneReport, load_world_scene};
