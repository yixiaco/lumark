//! UE5 Zen 包解析。
//!
//! Zen 包由摘要、可选版本信息、名称表、导入/导出映射和导出数据组成。
//! 当前实现读取包级元数据与导出数据范围，为后续 Tagged Property、纹理和
//! 网格解析提供入口；不负责旧式 UE4 `uasset`。

use std::io::{Cursor, Seek, SeekFrom};

use super::name_map::{MappedName, NameMap};
use super::script_objects::ZenScriptObjects;
use super::{
    AssetKind, BulkDataEntry, PackageError, PackageExport, PackageInfo, UnrealPackage, invalid,
    read_bytes, read_i32, read_i64, read_u32, read_u64,
};

/// 当前支持的最高容器头版本（UE5.6）。
const MAX_HEADER_VERSION: u8 = 5;
/// 单包导出数据上限。
const MAX_EXPORT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// 导出对象数量上限。
const MAX_EXPORTS: usize = 100_000;

/// Zen 包摘要。
#[derive(Debug, Clone, Copy)]
struct Summary {
    has_versioning_info: u32,
    header_size: u32,
    name: MappedName,
    cooked_header_size: u32,
    imported_public_export_hashes_offset: i32,
    import_map_offset: i32,
    export_map_offset: i32,
    export_bundle_entries_offset: i32,
    graph_data_offset: i32,
    dependency_bundle_headers_offset: i32,
    imported_package_names_offset: i32,
}

/// 版本信息。
#[derive(Debug, Clone, Copy)]
struct Versioning {
    file_version_ue4: i32,
    file_version_ue5: i32,
}

/// 解析指定头版本的 Zen 包。
fn parse_with_version(
    data: &[u8],
    scripts: Option<&ZenScriptObjects>,
    header_version: u8,
) -> Result<(PackageInfo, Vec<(usize, usize)>), PackageError> {
    if header_version == 0 || header_version > MAX_HEADER_VERSION {
        return Err(PackageError::Unsupported(format!(
            "不支持的 Zen 容器头版本 {header_version}"
        )));
    }
    let mut cursor = Cursor::new(data);
    let summary = read_summary(&mut cursor, header_version)?;
    if summary.header_size < 0x40 || summary.header_size as usize > data.len() {
        return Err(invalid("Zen 包头长度越界"));
    }
    let versioning = if summary.has_versioning_info != 0 {
        read_versioning(&mut cursor)?
    } else {
        Versioning {
            file_version_ue4: 0,
            file_version_ue5: 0,
        }
    };
    let (cell_import_map_offset, cell_export_map_offset) = if header_version >= 5 {
        (read_i32(&mut cursor)?, read_i32(&mut cursor)?)
    } else {
        (
            summary.export_bundle_entries_offset,
            summary.export_bundle_entries_offset,
        )
    };
    let name_map_position = cursor.position();
    if name_map_position > u64::from(summary.header_size) {
        return Err(invalid("Zen 名称表位置越过包头"));
    }
    let names = NameMap::read(&mut cursor)?;
    if names.len() == 0 {
        return Err(invalid("Zen 名称表为空"));
    }

    // UE5.4+ 的名称表后是 bulk data 映射（指向 .ubulk 或内联数据）。
    let mut bulk_data = Vec::new();
    if header_version >= 5 {
        let pad = read_u64(&mut cursor)?;
        if pad > summary.header_size as u64 {
            return Err(invalid("Zen bulk data 偏移越界"));
        }
        cursor
            .seek(SeekFrom::Current(pad as i64))
            .map_err(|_| invalid("Zen bulk data 偏移非法"))?;
        let map_size = read_i64(&mut cursor)?;
        if map_size < 0 {
            return Err(invalid("Zen bulk data 映射长度为负"));
        }
        let map_size = map_size as usize;
        if map_size > summary.header_size as usize || !map_size.is_multiple_of(32) {
            return Err(invalid("Zen bulk data 映射长度异常"));
        }
        let count = map_size / 32;
        if count > 1_000_000 {
            return Err(invalid("Zen bulk data 条目过多"));
        }
        for _ in 0..count {
            bulk_data.push(BulkDataEntry {
                serial_offset: read_i64(&mut cursor)?,
                duplicate_serial_offset: read_i64(&mut cursor)?,
                serial_size: read_i64(&mut cursor)?,
                flags: read_u32(&mut cursor)?,
                cooked_index: read_bytes(&mut cursor, 1)?[0],
            });
            let _padding = read_bytes(&mut cursor, 3)?;
        }
        if cursor.position() > u64::from(summary.header_size) {
            return Err(invalid("Zen bulk data 映射越过包头"));
        }
    }

    // 表偏移必须单调且位于包头内。
    let mut offsets = [
        summary.imported_public_export_hashes_offset,
        summary.import_map_offset,
        summary.export_map_offset,
        cell_import_map_offset,
        cell_export_map_offset,
        summary.export_bundle_entries_offset,
    ];
    if header_version >= 3 {
        offsets[5] = summary.dependency_bundle_headers_offset;
    }
    let header_size = summary.header_size as i32;
    if offsets
        .iter()
        .any(|offset| *offset < 0 || *offset > header_size)
    {
        return Err(invalid("Zen 表偏移越界"));
    }
    if !(summary.import_map_offset >= summary.imported_public_export_hashes_offset
        && summary.export_map_offset >= summary.import_map_offset
        && cell_import_map_offset >= summary.export_map_offset
        && cell_export_map_offset >= cell_import_map_offset
        && summary.export_bundle_entries_offset >= cell_export_map_offset)
    {
        return Err(invalid("Zen 表偏移顺序非法"));
    }
    if header_version >= 3
        && summary.dependency_bundle_headers_offset < summary.export_bundle_entries_offset
    {
        return Err(invalid("Zen 依赖表偏移顺序非法"));
    }
    let export_map_size = (cell_import_map_offset - summary.export_map_offset) as usize;
    if !export_map_size.is_multiple_of(72) {
        return Err(invalid("Zen 导出表长度不是 72 的倍数"));
    }
    let exports_count = export_map_size / 72;
    if exports_count == 0 || exports_count > MAX_EXPORTS {
        return Err(invalid("Zen 导出数量异常"));
    }
    let import_map_size = (summary.export_map_offset - summary.import_map_offset) as usize;
    if !import_map_size.is_multiple_of(8) {
        return Err(invalid("Zen 导入表长度不是 8 的倍数"));
    }
    let export_bundle_entries_end = if header_version >= 3 {
        summary.dependency_bundle_headers_offset
    } else {
        summary.export_bundle_entries_offset
    };
    let bundle_entries_size =
        (export_bundle_entries_end - summary.export_bundle_entries_offset) as usize;
    if !bundle_entries_size.is_multiple_of(8) {
        return Err(invalid("Zen 导出 bundle 表长度不是 8 的倍数"));
    }
    let bundle_entries_count = bundle_entries_size / 8;
    if bundle_entries_count != exports_count * 2 {
        return Err(invalid(format!(
            "Zen 导出 bundle 条目数 {bundle_entries_count} 与导出数 {exports_count} 不匹配"
        )));
    }

    // 读取导出表。
    cursor
        .seek(SeekFrom::Start(summary.export_map_offset as u64))
        .map_err(|_| invalid("Zen 导出表偏移非法"))?;
    let mut raw_exports = Vec::with_capacity(exports_count);
    for _ in 0..exports_count {
        let cooked_serial_offset = read_u64(&mut cursor)?;
        let cooked_serial_size = read_u64(&mut cursor)?;
        let object_name = NameMap::read_name(&mut cursor)?;
        let outer_index = read_u64(&mut cursor)?;
        let class_index = read_u64(&mut cursor)?;
        let _super_index = read_u64(&mut cursor)?;
        let _template_index = read_u64(&mut cursor)?;
        let public_export_hash = read_u64(&mut cursor)?;
        let _object_flags = read_u32(&mut cursor)?;
        let _filter_and_padding = read_bytes(&mut cursor, 4)?;
        raw_exports.push((
            cooked_serial_offset,
            cooked_serial_size,
            object_name,
            outer_index,
            class_index,
            public_export_hash,
        ));
    }

    let package_name = names.get(summary.name)?;
    let base = u64::from(summary.header_size);
    let mut exports = Vec::with_capacity(raw_exports.len());
    let mut ranges = Vec::with_capacity(raw_exports.len());
    for (index, (offset, size, object_name, outer_index, class_index, public_export_hash)) in
        raw_exports.iter().enumerate()
    {
        if *size > MAX_EXPORT_BYTES || base + offset + size > data.len() as u64 {
            return Err(invalid("Zen 导出数据范围越界"));
        }
        let name = names.get(*object_name)?;
        let class_name = resolve_class(*class_index, index, &exports, scripts);
        let kind = AssetKind::classify(&class_name, &name);
        let outer_export = match class_kind(*outer_index) {
            0 => Some(*outer_index as usize).filter(|outer| *outer < exports_count),
            _ => None,
        };
        let start = (base + offset) as usize;
        let end = start + *size as usize;
        ranges.push((start, end));
        exports.push(PackageExport {
            name,
            class_name,
            outer_export,
            data_offset: start,
            data_size: *size as usize,
            kind,
            public_export_hash: *public_export_hash,
        });
    }

    // 导入公共导出哈希表与 ImportMap（`FPackageObjectIndex` 原始 u64）。
    let imported_public_export_hashes = read_u64_table(
        data,
        summary.imported_public_export_hashes_offset,
        summary.import_map_offset,
    )?;
    let import_map = read_u64_table(data, summary.import_map_offset, summary.export_map_offset)?;
    // 避免未使用字段告警，同时保留摘要中的布局信息用于调试。
    let _ = (
        summary.cooked_header_size,
        summary.imported_package_names_offset,
        summary.graph_data_offset,
    );

    Ok((
        PackageInfo {
            package_name,
            file_version_ue4: versioning.file_version_ue4,
            file_version_ue5: versioning.file_version_ue5,
            header_version,
            names: names.names().to_vec(),
            import_map,
            imported_public_export_hashes,
            bulk_data,
            exports,
        },
        ranges,
    ))
}

/// 读取摘要。
fn read_summary(cursor: &mut Cursor<&[u8]>, header_version: u8) -> Result<Summary, PackageError> {
    let has_versioning_info = read_u32(cursor)?;
    let header_size = read_u32(cursor)?;
    let name = NameMap::read_name(cursor)?;
    let _ = read_u32(cursor)?; // package_flags
    let cooked_header_size = read_u32(cursor)?;
    let imported_public_export_hashes_offset = read_i32(cursor)?;
    let import_map_offset = read_i32(cursor)?;
    let export_map_offset = read_i32(cursor)?;
    let export_bundle_entries_offset = read_i32(cursor)?;
    let mut graph_data_offset = -1;
    let mut dependency_bundle_headers_offset = -1;
    let imported_package_names_offset;
    if header_version >= 3 {
        dependency_bundle_headers_offset = read_i32(cursor)?;
        let _dependency_bundle_entries_offset = read_i32(cursor)?;
        imported_package_names_offset = read_i32(cursor)?;
    } else {
        graph_data_offset = read_i32(cursor)?;
        imported_package_names_offset = -1;
    }
    Ok(Summary {
        has_versioning_info,
        header_size,
        name,
        cooked_header_size,
        imported_public_export_hashes_offset,
        import_map_offset,
        export_map_offset,
        export_bundle_entries_offset,
        graph_data_offset,
        dependency_bundle_headers_offset,
        imported_package_names_offset,
    })
}

/// 读取可选版本信息。
fn read_versioning(cursor: &mut Cursor<&[u8]>) -> Result<Versioning, PackageError> {
    let _zen_version = read_u32(cursor)?;
    let file_version_ue4 = read_i32(cursor)?;
    let file_version_ue5 = read_i32(cursor)?;
    let _licensee_version = read_i32(cursor)?;
    let custom_count = read_u32(cursor)? as usize;
    if custom_count > 10_000 {
        return Err(invalid("Zen 自定义版本数量异常"));
    }
    for _ in 0..custom_count {
        let _ = read_bytes(cursor, 16)?;
        let _ = read_i32(cursor)?;
    }
    Ok(Versioning {
        file_version_ue4,
        file_version_ue5,
    })
}

/// 读取 `[start, end)` 区间的 u64 表（用于 ImportMap/公共导出哈希）。
fn read_u64_table(data: &[u8], start: i32, end: i32) -> Result<Vec<u64>, PackageError> {
    if start < 0 || end < start {
        return Ok(Vec::new());
    }
    let start = start as usize;
    let end = end as usize;
    if end > data.len() || !(end - start).is_multiple_of(8) {
        return Err(invalid("Zen 导入表范围非法"));
    }
    Ok(data[start..end]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| u64::from_le_bytes(*chunk))
        .collect())
}

/// `FPackageObjectIndex` 的顶层类型位。
fn class_kind(raw: u64) -> u8 {
    (raw >> 62) as u8
}

/// 解析导出类索引。
fn resolve_class(
    class_index: u64,
    own_index: usize,
    previous_exports: &[PackageExport],
    scripts: Option<&ZenScriptObjects>,
) -> String {
    match class_kind(class_index) {
        0 => {
            let target = (class_index & ((1u64 << 62) - 1)) as usize;
            if target == own_index {
                "Class".to_string()
            } else if let Some(export) = previous_exports.get(target) {
                export.name.clone()
            } else {
                format!("Export({target})")
            }
        }
        1 => scripts
            .and_then(|scripts| scripts.resolve_name(class_index))
            .map(str::to_string)
            .unwrap_or_else(|| "ScriptImport".to_string()),
        2 => "PackageImport".to_string(),
        3 => "None".to_string(),
        _ => "Unknown".to_string(),
    }
}

/// 解析 Zen 包；未指定头版本时尝试 UE5.6 到 UE5.0 的布局。
pub fn parse_package(
    data: Vec<u8>,
    scripts: Option<&ZenScriptObjects>,
    header_version: Option<u8>,
) -> Result<UnrealPackage, PackageError> {
    let candidates: Vec<u8> = match header_version {
        Some(version) => vec![version],
        None => (1..=MAX_HEADER_VERSION).rev().collect(),
    };
    let mut last_error = None;
    for version in candidates {
        match parse_with_version(&data, scripts, version) {
            Ok((info, _ranges)) => {
                return Ok(UnrealPackage {
                    info,
                    data: data.clone(),
                });
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| invalid("没有可用的 Zen 头版本")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::AssetKind;

    /// 资产分类按类名和名称前缀工作。
    #[test]
    fn asset_kind_classification() {
        assert_eq!(
            AssetKind::classify("StaticMesh", "SM_Cube"),
            AssetKind::StaticMesh
        );
        assert_eq!(
            AssetKind::classify("Texture2D", "T_Test"),
            AssetKind::Texture
        );
        assert_eq!(
            AssetKind::classify("Skeleton", "SK_Mannequin"),
            AssetKind::Skeleton
        );
        assert_eq!(
            AssetKind::classify("", "SM_Fallback"),
            AssetKind::StaticMesh
        );
    }

    /// 头版本和偏移校验拒绝明显损坏的输入。
    #[test]
    fn corrupt_zen_inputs_are_rejected() {
        let data = vec![0u8; 128];
        assert!(parse_package(data, None, Some(5)).is_err());
    }
}
