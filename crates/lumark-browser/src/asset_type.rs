//! 资产类型分类：用于文件表和归档的“按类型搜索/筛选”。
//!
//! 分类只依据扩展名和路径/文件名里的 UE 命名约定做启发式判断，不读取文件
//! 内容；游戏容器里的 `.uasset` 依靠目录名（如 `Model`、`Texture`）和常见前缀
//! （`SK_`、`SM_`、`AS_`、`Skeleton`）区分模型、骨骼与动画。

/// 浏览器搜索和筛选使用的资产类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetCategory {
    /// 图片、纹理和贴图。
    Image,
    /// 静态网格、骨骼网格等模型资产。
    Model,
    /// 骨骼资产。
    Skeleton,
    /// 动画序列、动画蒙太奇。
    Animation,
    /// 音频。
    Audio,
    /// 文本、配置和数据表。
    Text,
    /// 压缩包和游戏容器。
    Archive,
    /// 其它无法归类的文件。
    Other,
}

impl AssetCategory {
    /// 按界面展示顺序返回全部类型。
    pub const ALL: [Self; 8] = [
        AssetCategory::Image,
        AssetCategory::Model,
        AssetCategory::Skeleton,
        AssetCategory::Animation,
        AssetCategory::Audio,
        AssetCategory::Text,
        AssetCategory::Archive,
        AssetCategory::Other,
    ];
}

/// 按路径判断资产类型；目录没有类型，返回 `None`。
pub fn classify(path: &str, is_dir: bool) -> Option<AssetCategory> {
    if is_dir {
        return None;
    }
    // 文件系统路径在 Windows 上使用反斜杠，统一成正斜杠后再做目录启发式判断。
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    let extension = lower
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    Some(match extension {
        "uasset" | "umap" => classify_unreal_asset(&lower),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "tga" | "tif" | "tiff" | "dds"
        | "ico" | "svg" | "hdr" | "exr" | "psd" => AssetCategory::Image,
        "wav" | "mp3" | "ogg" | "flac" | "wem" | "bank" | "bnk" | "sfx" => AssetCategory::Audio,
        "txt" | "md" | "json" | "ini" | "cfg" | "xml" | "yaml" | "yml" | "csv" | "log"
        | "usmap" | "lua" | "py" | "rs" | "toml" => AssetCategory::Text,
        "zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz" | "zst" | "pak" | "utoc" | "ucas"
        | "enc" | "iso" | "wim" | "lzh" => AssetCategory::Archive,
        "mesh" | "fbx" | "obj" | "gltf" | "glb" | "psk" | "psa" => AssetCategory::Model,
        "anim" | "animsequence" => AssetCategory::Animation,
        "skeleton" | "skel" => AssetCategory::Skeleton,
        _ => AssetCategory::Other,
    })
}

/// 按 UE4/UE5 的目录名和命名前缀判断 `.uasset`/`.umap` 的类型。
fn classify_unreal_asset(lower: &str) -> AssetCategory {
    // 骨骼优先于模型：`SK_` 前缀通常表示 SkeletalMesh，而 `Skeleton` 是骨骼。
    if lower.contains("skeleton") || lower.contains("_skel") || lower.contains("/skeleton") {
        return AssetCategory::Skeleton;
    }
    if lower.contains("/animation")
        || lower.contains("/anim")
        || lower.contains("_anim")
        || lower.contains("animsequence")
        || lower.contains("animmontage")
        || lower.contains("/sequence")
    {
        return AssetCategory::Animation;
    }
    if lower.contains("texture")
        || lower.contains("/texture")
        || lower.contains("/textures")
        || lower.contains("_tex")
        || lower.contains("_t_")
    {
        return AssetCategory::Image;
    }
    if lower.contains("staticmesh")
        || lower.contains("skeletalmesh")
        || lower.contains("/model")
        || lower.contains("/models")
        || lower.contains("/mesh")
        || lower.contains("/meshes")
        || lower.contains("/sk_")
        || lower.contains("/sm_")
        || lower.contains("/skm_")
        || lower.contains("/skm")
        || lower.ends_with(".umap")
    {
        return AssetCategory::Model;
    }
    AssetCategory::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证常见文件扩展名分类。
    #[test]
    fn classifies_common_extensions() {
        assert_eq!(classify("photo.PNG", false), Some(AssetCategory::Image));
        assert_eq!(classify("music.ogg", false), Some(AssetCategory::Audio));
        assert_eq!(classify("readme.txt", false), Some(AssetCategory::Text));
        assert_eq!(classify("pack.zip", false), Some(AssetCategory::Archive));
        assert_eq!(classify("hero.fbx", false), Some(AssetCategory::Model));
        assert_eq!(classify("unknown.bin", false), Some(AssetCategory::Other));
        assert_eq!(classify("docs", true), None);
    }

    /// 验证 UE 资产的目录/前缀启发式分类。
    #[test]
    fn classifies_unreal_assets_by_path() {
        let model = "End/Content/Character/Enemy/Rufus/Model/EB0016_00.uasset";
        assert_eq!(classify(model, false), Some(AssetCategory::Model));
        let skeleton = "End/Content/Character/Enemy/Rufus/Model/EB0016_00_Skeleton.uasset";
        assert_eq!(classify(skeleton, false), Some(AssetCategory::Skeleton));
        let animation = "End/Content/Character/Hero/Animation/Idle_Anim.uasset";
        assert_eq!(classify(animation, false), Some(AssetCategory::Animation));
        let texture = "End/Content/Character/Hero/Texture/T_Hero_D.uasset";
        assert_eq!(classify(texture, false), Some(AssetCategory::Image));
        assert_eq!(
            classify("Game/Content/Maps/Level01.umap", false),
            Some(AssetCategory::Model)
        );
        assert_eq!(
            classify("Game\\Content\\Texture\\T_Hero_D.uasset", false),
            Some(AssetCategory::Image)
        );
        assert_eq!(
            classify("Game/Content/Data/Table.uasset", false),
            Some(AssetCategory::Other)
        );
    }
}
