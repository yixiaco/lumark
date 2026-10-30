//! Unreal Engine 游戏资产支持：目录扫描、引擎版本探测、AES 密钥提取和 Zen 包读取。
//!
//! 本 crate 只做本地只读分析，不依赖 UI；容器读取复用 `lumark-archive`。

pub mod game;
pub mod keys;
pub mod package;
pub mod raster;

pub use game::{
    ContainerInfo, ContainerKind, DetectedGame, GameScanError, UnrealEngine, scan_game,
};
pub use keys::{KeyExtraction, KeyStage, extract_key, extract_key_for_game, verify_container_key};
pub use package::legacy::{LegacyPackageSummary, is_legacy_package, parse_legacy_summary};
pub use package::{
    AssetKind, AssetMetadata, Bone, BulkDataEntry, Mappings, MeshLod, MeshSection, PackageError,
    PackageExport, PackageInfo, PropSchema, PropType, PropValue, SkeletalMeshPreview, Skeleton,
    StaticMesh, StructSchema, Texture2D, TextureMip, UnrealPackage, UnversionedProperties,
    ZenScriptObjects, parse_package, read_asset_metadata,
};
pub use raster::{render_skeleton_orbit, render_static_mesh, render_static_mesh_orbit};
