//! UE 包伴随文件的识别与合并。
//!
//! 游戏容器里的 `.uexp/.ubulk/.uptnl` 只是某个 `.uasset/.umap` 的伴随数据。
//! 游戏资产按包浏览时隐藏这些条目，并把它们的大小并入所属包，既减少列表噪声，
//! 又保证文件夹大小与进入后的可见条目总和一致。

use std::collections::{HashMap, HashSet};

use lumark_archive::ArchiveEntry;

/// 游戏资产包伴随数据的扩展名。
pub const UE_SIDECAR_EXTENSIONS: [&str; 3] = [".uexp", ".ubulk", ".uptnl"];

/// 判断路径是否为 UE 包的伴随数据文件（大小写不敏感）。
pub fn is_ue_sidecar(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    UE_SIDECAR_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
}

/// 去掉伴随文件扩展名，得到包的路径主干。
fn sidecar_stem(path: &str) -> Option<&str> {
    let lower = path.to_ascii_lowercase();
    UE_SIDECAR_EXTENSIONS.iter().find_map(|extension| {
        lower
            .ends_with(extension)
            .then(|| &path[..path.len() - extension.len()])
    })
}

/// 把伴随文件的大小并入所属包，并从结果中移除这些条目。
///
/// 找不到对应 `.uasset/.umap` 的伴随文件保持原样，避免隐藏后数据既看不见、
/// 又没有被计入所在文件夹。
pub fn merge_ue_sidecars(entries: &[ArchiveEntry]) -> Vec<ArchiveEntry> {
    let assets = entries
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.path.as_str())
        .collect::<HashSet<_>>();
    let mut merged = HashMap::<String, (u64, Option<u64>)>::new();
    let mut hidden = HashSet::<&str>::new();
    for entry in entries {
        if entry.is_dir || !is_ue_sidecar(&entry.path) {
            continue;
        }
        let Some(stem) = sidecar_stem(&entry.path) else {
            continue;
        };
        let Some(asset) = [".uasset", ".umap"]
            .iter()
            .map(|extension| format!("{stem}{extension}"))
            .find(|candidate| assets.contains(candidate.as_str()))
        else {
            continue;
        };
        hidden.insert(entry.path.as_str());
        let merged_size = merged.entry(asset).or_insert((0, None));
        merged_size.0 = merged_size.0.saturating_add(entry.size);
        if let Some(compressed) = entry.compressed_size {
            merged_size.1 = Some(merged_size.1.unwrap_or(0).saturating_add(compressed));
        }
    }
    entries
        .iter()
        .filter(|entry| !hidden.contains(entry.path.as_str()))
        .map(|entry| {
            let Some((size_delta, compressed_delta)) = merged.get(&entry.path) else {
                return entry.clone();
            };
            let mut entry = entry.clone();
            entry.size = entry.size.saturating_add(*size_delta);
            if let Some(compressed_delta) = compressed_delta {
                entry.compressed_size = Some(
                    entry
                        .compressed_size
                        .unwrap_or(0)
                        .saturating_add(*compressed_delta),
                );
            }
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个只填测试所需字段的归档条目。
    fn entry(path: &str, size: u64, compressed: Option<u64>) -> ArchiveEntry {
        ArchiveEntry {
            id: 0,
            path: path.to_string(),
            is_dir: false,
            size,
            compressed_size: compressed,
            modified: None,
            crc32: None,
            method: String::new(),
            encrypted: false,
        }
    }

    /// 伴随文件被隐藏并计入所属 `.uasset`；孤立伴随文件保持可见。
    #[test]
    fn sidecars_merge_into_their_package() {
        let entries = vec![
            entry("Enemy/a.uasset", 4, Some(3)),
            entry("Enemy/a.uexp", 40, Some(20)),
            entry("Enemy/a.ubulk", 400, Some(100)),
            entry("Enemy/orphan.uexp", 7, Some(1)),
        ];
        let merged = merge_ue_sidecars(&entries);
        let paths = merged
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["Enemy/a.uasset", "Enemy/orphan.uexp"]);
        assert_eq!(merged[0].size, 444);
        assert_eq!(merged[0].compressed_size, Some(123));
        assert_eq!(merged[1].size, 7);
    }

    /// 伴随文件扩展名判断不区分大小写。
    #[test]
    fn sidecar_extensions_are_case_insensitive() {
        assert!(is_ue_sidecar("Enemy/a.UEXP"));
        assert!(is_ue_sidecar("Enemy/a.UBULK"));
        assert!(is_ue_sidecar("Enemy/a.uptnl"));
        assert!(!is_ue_sidecar("Enemy/a.uasset"));
        assert!(!is_ue_sidecar("Enemy/ubulk.txt"));
    }
}
