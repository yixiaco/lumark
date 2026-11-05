//! UE 包导入解析：把 `FPackageIndex` 导入项解析为目标包 ID 与公共导出哈希。
//!
//! Zen（UE5 IoStore）包的对象引用不直接带包路径：属性里的负 `FPackageIndex`
//! 指向包 ImportMap 中的 `FPackageObjectIndex`，再配合 ContainerHeader 的
//! 导入包 ID 表与包内公共导出哈希表，才能定位到目标包中的具体导出。
//! 世界场景加载和材质图还原共用这里的解析逻辑。

use crate::package::{PackageError, PackageExport, PackageInfo};

/// 解析一条包导入：返回 `(目标包 ID, 目标公共导出哈希)`。
pub(crate) fn resolve_package_import(
    info: &PackageInfo,
    imported_ids: &[u64],
    import_index: usize,
) -> Result<(u64, u64), PackageError> {
    let raw = *info
        .import_map
        .get(import_index)
        .ok_or_else(|| unsupported("导入索引越界"))?;
    if raw == u64::MAX || (raw >> 62) != 2 {
        return Err(unsupported("导入项不是 PackageImport"));
    }
    let value = raw & ((1u64 << 62) - 1);
    let package_index = (value >> 32) as usize;
    let hash_index = (value & 0xffff_ffff) as usize;
    let package_id = *imported_ids
        .get(package_index)
        .ok_or_else(|| unsupported("导入包索引越界"))?;
    let hash = *info
        .imported_public_export_hashes
        .get(hash_index)
        .ok_or_else(|| unsupported("公共导出哈希索引越界"))?;
    Ok((package_id, hash))
}

/// 按公共导出哈希查找导出。
pub(crate) fn find_export_by_public_hash(exports: &[PackageExport], hash: u64) -> Option<usize> {
    exports
        .iter()
        .position(|export| export.public_export_hash == hash)
}

/// 构造“不支持/越界”类包错误。
fn unsupported(message: &str) -> PackageError {
    PackageError::Unsupported(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解码 PackageImport 的包索引与哈希索引。
    #[test]
    fn decodes_package_import_reference() {
        // type=2, package_index=0x52, hash_index=0
        let info = PackageInfo {
            package_name: "Test".to_string(),
            file_version_ue4: 0,
            file_version_ue5: 0,
            header_version: 5,
            header_size: 0,
            cooked_header_size: 0,
            package_flags: 0,
            names: Vec::new(),
            import_map: vec![0x8000_0052_0000_0000],
            imported_public_export_hashes: vec![0xAAAA],
            bulk_data: Vec::new(),
            exports: Vec::new(),
        };
        let mut ids = vec![0u64; 0x53];
        ids[0x52] = 0x1234;
        assert_eq!(
            resolve_package_import(&info, &ids, 0).unwrap(),
            (0x1234, 0xAAAA)
        );
    }

    /// 空导入或非 PackageImport 类型应报错而不是返回错误目标。
    #[test]
    fn rejects_non_package_imports() {
        let mut info = PackageInfo {
            package_name: "Test".to_string(),
            file_version_ue4: 0,
            file_version_ue5: 0,
            header_version: 5,
            header_size: 0,
            cooked_header_size: 0,
            package_flags: 0,
            names: Vec::new(),
            import_map: vec![u64::MAX, 0x8000_0000_0000_0001],
            imported_public_export_hashes: vec![1],
            bulk_data: Vec::new(),
            exports: Vec::new(),
        };
        assert!(resolve_package_import(&info, &[1], 0).is_err());
        assert!(resolve_package_import(&info, &[1], 1).is_err());
        info.import_map.clear();
        assert!(resolve_package_import(&info, &[1], 0).is_err());
    }
}
