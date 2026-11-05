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
/// UE5 对象版本 `DATA_RESOURCES`（1009）：UE5.2 起 Zen 头带 bulk data 映射。
const DATA_RESOURCES_OBJECT_VERSION: i32 = 1009;
/// bulk data 映射条目固定序列化长度。
const BULK_DATA_ENTRY_SIZE: usize = 32;
/// bulk data 映射条目数量上限。
const MAX_BULK_MAP_ENTRIES: usize = 1_000_000;
/// 单条 bulk 数据长度上限，用于拒绝明显损坏的映射。
const MAX_BULK_ENTRY_BYTES: i64 = 1024 * 1024 * 1024;

/// Zen 包摘要。
#[derive(Debug, Clone, Copy)]
struct Summary {
    has_versioning_info: u32,
    header_size: u32,
    name: MappedName,
    package_flags: u32,
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

    // 名称表之后是 bulk data 映射（UE5.2 起，UE5.4 起前面多一段对齐 pad）。
    let bulk_data = read_bulk_data_map(&mut cursor, header_version, &summary, &versioning)?;

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
    // Initial 布局（UE5.0–5.2）的 bundle 表止于 GraphDataOffset；该字段缺失或
    // 乱序时直接报错，避免后续计算 bundle 表长度时出现负数转无符号数。
    if header_version < 3
        && (summary.graph_data_offset < summary.export_bundle_entries_offset
            || summary.graph_data_offset > header_size)
    {
        return Err(invalid("Zen 图数据偏移非法"));
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
        // Initial 布局的 bundle 表后面是 GraphData 表，而不是 bundle 表自身；
        // 使用自身偏移会让条目数恒为 0，导致 UE5.2 包全部解析失败。
        summary.graph_data_offset
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
    // UE5.0–5.2（Initial 布局）的 cooked 包按**导出 bundle 顺序**紧凑存放导出数据，
    // `CookedSerialOffset` 只是 cook 时的虚拟偏移；无法解析 bundle 头时才回退到
    // 以 `CookedHeaderSize` 为基准的换算。新布局（依赖 bundle）直接使用
    // `HeaderSize + offset`。
    let bundle_offsets = if header_version < 3 {
        let export_sizes = raw_exports
            .iter()
            .map(|(_, size, ..)| usize::try_from(*size).ok())
            .collect::<Option<Vec<_>>>();
        export_sizes.and_then(|sizes| {
            bundle_data_offsets(data, &summary, exports_count, bundle_entries_count, &sizes)
        })
    } else {
        None
    };
    let export_data_base = if header_version < 3 {
        i64::from(summary.header_size) - i64::from(summary.cooked_header_size)
    } else {
        i64::from(summary.header_size)
    };
    let mut exports = Vec::with_capacity(raw_exports.len());
    let mut ranges = Vec::with_capacity(raw_exports.len());
    for (index, (offset, size, object_name, outer_index, class_index, public_export_hash)) in
        raw_exports.iter().enumerate()
    {
        let start = match &bundle_offsets {
            Some(offsets) => offsets[index],
            None => {
                let start_i64 = export_data_base + *offset as i64;
                if start_i64 < 0 {
                    return Err(invalid("Zen 导出数据偏移为负"));
                }
                start_i64 as usize
            }
        };
        let end = start
            .checked_add(*size as usize)
            .ok_or_else(|| invalid("Zen 导出数据范围溢出"))?;
        if *size > MAX_EXPORT_BYTES || end > data.len() {
            return Err(invalid("Zen 导出数据范围越界"));
        }
        let name = names.get(*object_name)?;
        let class_name = resolve_class(*class_index, index, &exports, scripts);
        let kind = AssetKind::classify(&class_name, &name);
        let outer_export = match class_kind(*outer_index) {
            0 => Some(*outer_index as usize).filter(|outer| *outer < exports_count),
            _ => None,
        };
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
    );

    Ok((
        PackageInfo {
            package_name,
            file_version_ue4: versioning.file_version_ue4,
            file_version_ue5: versioning.file_version_ue5,
            header_version,
            header_size: summary.header_size,
            cooked_header_size: summary.cooked_header_size,
            package_flags: summary.package_flags,
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
    let package_flags = read_u32(cursor)?;
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
        package_flags,
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

/// 读取名称表之后的 Zen bulk data 映射。
///
/// UE5.2 起（对象版本 `DATA_RESOURCES`）cooked 包的 Zen 头在名称表后紧跟 bulk
/// data 映射，导出数据里的 `FByteBulkData` 只序列化一个映射索引；UE5.4 起映射
/// 前多出一段对齐 pad。UE5.0/5.1 没有这张表，未携带版本信息的 cooked 包只能
/// 按结构探测，探测失败时回退为“无映射”。
fn read_bulk_data_map(
    cursor: &mut Cursor<&[u8]>,
    header_version: u8,
    summary: &Summary,
    versioning: &Versioning,
) -> Result<Vec<BulkDataEntry>, PackageError> {
    let has_versioning = summary.has_versioning_info != 0;
    if has_versioning && versioning.file_version_ue5 < DATA_RESOURCES_OBJECT_VERSION {
        return Ok(Vec::new());
    }
    // 有版本信息时按对象版本确定；UE5.4+ 头版本同样必然带映射；其余靠探测。
    let mandatory = has_versioning || header_version >= 5;
    let map_start = cursor.position();
    if header_version >= 5 {
        let pad = read_u64(cursor)?;
        if pad > u64::from(summary.header_size) {
            return Err(invalid("Zen bulk data 偏移越界"));
        }
        cursor
            .seek(SeekFrom::Current(pad as i64))
            .map_err(|_| invalid("Zen bulk data 偏移非法"))?;
    }
    let size_position = cursor.position();
    // 缓冲区剩余不足 8 字节时无法读取映射长度：探测模式回退，强制场景报错。
    if size_position + 8 > cursor.get_ref().len() as u64 {
        return fallback_without_map(cursor, map_start, mandatory, "Zen bulk data 映射被截断");
    }
    let map_size = read_i64(cursor)?;
    // 长度必须非负、不超过包头且是条目长度的整数倍。
    if map_size < 0
        || map_size as u64 > u64::from(summary.header_size)
        || !(map_size as usize).is_multiple_of(BULK_DATA_ENTRY_SIZE)
    {
        return fallback_without_map(cursor, map_start, mandatory, "Zen bulk data 映射长度异常");
    }
    let map_size = map_size as usize;
    let count = map_size / BULK_DATA_ENTRY_SIZE;
    if count > MAX_BULK_MAP_ENTRIES {
        return fallback_without_map(cursor, map_start, mandatory, "Zen bulk data 条目过多");
    }
    // 探测模式额外要求映射整体位于下一张表之前，避免把其他表误判成映射。
    if !mandatory && (count == 0 || size_position + 8 + map_size as u64 > bulk_map_limit(summary)) {
        return fallback_without_map(cursor, map_start, false, "");
    }
    let entries = match read_bulk_entries(cursor, count) {
        Ok(entries) => entries,
        Err(error) => {
            return fallback_without_map(cursor, map_start, mandatory, &error.to_string());
        }
    };
    if cursor.position() > u64::from(summary.header_size) {
        return fallback_without_map(cursor, map_start, mandatory, "Zen bulk data 映射越过包头");
    }
    // 只在探测模式校验条目：强制场景沿用原有行为，避免版本判定的包出现回归。
    if !mandatory && !bulk_entries_plausible(&entries) {
        return fallback_without_map(cursor, map_start, false, "");
    }
    Ok(entries)
}

/// 探测失败时回退：强制场景报错，其余回退为“无映射”并复位游标。
fn fallback_without_map(
    cursor: &mut Cursor<&[u8]>,
    position: u64,
    mandatory: bool,
    message: &str,
) -> Result<Vec<BulkDataEntry>, PackageError> {
    if mandatory {
        return Err(invalid(message));
    }
    cursor
        .seek(SeekFrom::Start(position))
        .map_err(|_| invalid("Zen bulk data 复位失败"))?;
    Ok(Vec::new())
}

/// bulk data 映射必须位于下一张表（导入公共导出哈希表）之前。
fn bulk_map_limit(summary: &Summary) -> u64 {
    if summary.imported_public_export_hashes_offset >= 0 {
        summary.imported_public_export_hashes_offset as u64
    } else {
        u64::from(summary.header_size)
    }
}

/// 读取 bulk data 映射条目。
fn read_bulk_entries(
    cursor: &mut Cursor<&[u8]>,
    count: usize,
) -> Result<Vec<BulkDataEntry>, PackageError> {
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        entries.push(BulkDataEntry {
            serial_offset: read_i64(cursor)?,
            duplicate_serial_offset: read_i64(cursor)?,
            serial_size: read_i64(cursor)?,
            flags: read_u32(cursor)?,
            cooked_index: read_bytes(cursor, 1)?[0],
        });
        let _padding = read_bytes(cursor, 3)?;
    }
    Ok(entries)
}

/// 探测模式下校验映射条目是否像真实数据：至少一条有效载荷，且长度/偏移合理。
fn bulk_entries_plausible(entries: &[BulkDataEntry]) -> bool {
    let mut valid = 0usize;
    for entry in entries {
        if entry.serial_size == 0 {
            continue;
        }
        if entry.serial_size < 0
            || entry.serial_size > MAX_BULK_ENTRY_BYTES
            || entry.serial_offset < 0
            || entry.cooked_index > 8
        {
            return false;
        }
        valid += 1;
    }
    valid > 0
}

/// 按导出 bundle 顺序计算每个导出数据在包内的实际偏移。
///
/// UE5.0–5.2 的 Zen 头在 `GraphDataOffset` 处存放 `FExportBundleHeader`
/// （`u64 SerialOffset` + `u32 FirstEntryIndex` + `u32 EntryCount`），在
/// `ExportBundleEntriesOffset` 处存放 `FExportBundleEntry`（8 字节：本地导出
/// 索引 + 命令类型）。导出数据按 bundle 顺序从 `HeaderSize` 起紧凑排列，
/// `Serialize` 命令决定顺序，长度取导出表的 `CookedSerialSize`。
///
/// 结构不自洽时返回 `None`，调用方回退到 `CookedSerialOffset` 换算。
fn bundle_data_offsets(
    data: &[u8],
    summary: &Summary,
    exports_count: usize,
    entries_count: usize,
    export_sizes: &[usize],
) -> Option<Vec<usize>> {
    if summary.graph_data_offset < 0 || summary.export_bundle_entries_offset < 0 {
        return None;
    }
    let header_size = summary.header_size as usize;
    let graph = summary.graph_data_offset as usize;
    let entries_start = summary.export_bundle_entries_offset as usize;
    if graph < entries_start || graph + 16 > header_size {
        return None;
    }
    let read_u32_at = |position: usize| -> Option<u32> {
        let bytes: [u8; 4] = data.get(position..position + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes))
    };
    // bundle 头依次覆盖全部 bundle 条目，`FirstEntryIndex` 必须连续。
    let mut headers = Vec::new();
    let mut total = 0usize;
    let mut position = graph;
    loop {
        if position + 16 > header_size || headers.len() > 256 {
            return None;
        }
        let first_entry = read_u32_at(position + 8)? as usize;
        let entry_count = read_u32_at(position + 12)? as usize;
        if first_entry != total || entry_count == 0 || total + entry_count > entries_count {
            return None;
        }
        headers.push((first_entry, entry_count));
        total += entry_count;
        position += 16;
        if total == entries_count {
            break;
        }
    }
    if entries_start + entries_count * 8 > data.len() {
        return None;
    }
    let mut offsets = vec![usize::MAX; exports_count];
    let mut current = header_size;
    for (first_entry, entry_count) in headers {
        for entry_index in first_entry..first_entry + entry_count {
            let base = entries_start + entry_index * 8;
            let local = read_u32_at(base)? as usize;
            let command = read_u32_at(base + 4)?;
            if local >= exports_count || command > 1 {
                return None;
            }
            if command == 0 {
                continue;
            }
            // 每个导出只允许序列化一次。
            if offsets[local] != usize::MAX {
                return None;
            }
            offsets[local] = current;
            current = current.checked_add(*export_sizes.get(local)?)?;
            if current > data.len() {
                return None;
            }
        }
    }
    if offsets.contains(&usize::MAX) {
        return None;
    }
    Some(offsets)
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

    /// 构造一个最小 Initial 布局包：44 字节摘要、GraphData 表收尾 bundle 表、
    /// 一个导出和 4 字节导出数据。
    ///
    /// `bulk_map` 为真时在名称表后追加一条指向 `.ubulk` 的 bulk data 映射条目，
    /// 模拟 UE5.2（对象版本 `DATA_RESOURCES`）的 cooked 布局。
    fn initial_layout_package_with(bulk_map: bool) -> Vec<u8> {
        const NAMES: [&str; 2] = ["TestPackage", "TestObject"];
        let mut data = vec![0u8; 44];
        data[8..12].copy_from_slice(&0u32.to_le_bytes()); // 包名索引 0
        data[12..16].copy_from_slice(&0u32.to_le_bytes()); // 包名序号 0

        // 名称表：2 个 ASCII 名称。
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&21u32.to_le_bytes());
        data.extend_from_slice(&0xC164_0000u64.to_le_bytes());
        data.extend_from_slice(&[0u8; 16]);
        data.extend_from_slice(&(NAMES[0].len() as i16).to_be_bytes());
        data.extend_from_slice(&(NAMES[1].len() as i16).to_be_bytes());
        data.extend_from_slice(NAMES[0].as_bytes());
        data.extend_from_slice(NAMES[1].as_bytes());
        if bulk_map {
            // 一条外部 bulk 条目：SerialOffset 64、SerialSize 16、载荷在 .ubulk。
            data.extend_from_slice(&32i64.to_le_bytes());
            data.extend_from_slice(&64i64.to_le_bytes());
            data.extend_from_slice(&0i64.to_le_bytes());
            data.extend_from_slice(&16i64.to_le_bytes());
            data.extend_from_slice(&0x1_0501u32.to_le_bytes());
            data.push(0);
            data.extend_from_slice(&[0u8; 3]);
        }
        let import_map_offset = data.len() as i32;

        // 一个导出记录：数据偏移稍后按 CookedHeaderSize 回填、长度 4、
        // 类为 PackageImport。
        let mut export = Vec::new();
        export.extend_from_slice(&0u64.to_le_bytes());
        export.extend_from_slice(&4u64.to_le_bytes());
        export.extend_from_slice(&1u32.to_le_bytes());
        export.extend_from_slice(&0u32.to_le_bytes());
        export.extend_from_slice(&(3u64 << 62).to_le_bytes());
        export.extend_from_slice(&(2u64 << 62).to_le_bytes());
        export.extend_from_slice(&0u64.to_le_bytes());
        export.extend_from_slice(&0u64.to_le_bytes());
        export.extend_from_slice(&0u64.to_le_bytes());
        export.extend_from_slice(&0u32.to_le_bytes());
        export.extend_from_slice(&[0u8; 4]);
        assert_eq!(export.len(), 72);
        data.extend_from_slice(&export);
        let export_bundle_entries_offset = data.len() as i32;
        data.extend_from_slice(&[0u8; 16]);
        let graph_data_offset = data.len() as i32;
        data.extend_from_slice(&[0u8; 8]);
        let header_size = data.len() as i32;
        data.extend_from_slice(&[0xAA; 4]);

        // 模拟真实 UE5.2 包：CookedSerialOffset 以 CookedHeaderSize 为基准，
        // 而实际数据紧跟在 HeaderSize 后；两者相差 13 字节。
        let cooked_header_size = header_size + 13;
        data[4..8].copy_from_slice(&(header_size as u32).to_le_bytes());
        data[20..24].copy_from_slice(&(cooked_header_size as u32).to_le_bytes());
        data[24..28].copy_from_slice(&import_map_offset.to_le_bytes());
        data[28..32].copy_from_slice(&import_map_offset.to_le_bytes());
        data[32..36].copy_from_slice(&import_map_offset.to_le_bytes());
        data[36..40].copy_from_slice(&export_bundle_entries_offset.to_le_bytes());
        data[40..44].copy_from_slice(&graph_data_offset.to_le_bytes());
        // 导出记录的 CookedSerialOffset 相对 CookedHeaderSize，实际数据偏移为 0。
        let export_record_offset = import_map_offset as usize;
        data[export_record_offset..export_record_offset + 8]
            .copy_from_slice(&(cooked_header_size as u64).to_le_bytes());
        data
    }

    /// Initial 布局的 bundle 表止于 GraphDataOffset，而不是 bundle 表自身。
    #[test]
    fn parses_initial_layout_with_graph_data_bundle_end() {
        let data = initial_layout_package_with(false);
        let package = parse_package(data.clone(), None, Some(2)).expect("Initial 布局应解析成功");
        assert_eq!(package.info().header_version, 2);
        assert_eq!(package.info().package_name, "TestPackage");
        assert_eq!(package.info().exports.len(), 1);
        assert_eq!(package.info().exports[0].name, "TestObject");
        assert_eq!(package.export_data(0), Some([0xAA; 4].as_slice()));
        // 没有 bulk data 映射的包里映射为空。
        assert!(package.info().bulk_data.is_empty());

        let detected = parse_package(data, None, None).expect("自动版本探测应成功");
        assert_eq!(detected.info().header_version, 2);
    }

    /// UE5.2 的 Initial 布局在名称表后带 bulk data 映射（UE5.4 才有对齐 pad）。
    #[test]
    fn parses_initial_layout_bulk_data_map() {
        let data = initial_layout_package_with(true);
        let package = parse_package(data, None, Some(2)).expect("带 bulk 映射的包应解析成功");
        let bulk = &package.info().bulk_data;
        assert_eq!(bulk.len(), 1);
        assert_eq!(bulk[0].serial_offset, 64);
        assert_eq!(bulk[0].serial_size, 16);
        assert_eq!(bulk[0].flags, 0x1_0501);
        assert_eq!(bulk[0].cooked_index, 0);
    }

    /// UE5.0–5.2 的导出数据按 bundle 顺序存放：`Serialize` 顺序可以与导出索引相反。
    ///
    /// 构造一个两导出的包：bundle 条目先序列化导出 1、再序列化导出 0，
    /// 而导出表里的虚拟 `CookedSerialOffset` 仍按索引顺序排列。
    fn bundle_order_package() -> Vec<u8> {
        const NAMES: [&str; 3] = ["TestPackage", "ExportA", "ExportB"];
        let mut data = vec![0u8; 44];
        data[8..12].copy_from_slice(&0u32.to_le_bytes()); // 包名索引 0
        data.extend_from_slice(&(NAMES.len() as u32).to_le_bytes());
        data.extend_from_slice(&21u32.to_le_bytes());
        data.extend_from_slice(&0xC164_0000u64.to_le_bytes());
        // 名称哈希表长度是 count + 1。
        data.extend_from_slice(&[0u8; 24]);
        for name in NAMES {
            data.extend_from_slice(&(name.len() as i16).to_be_bytes());
        }
        for name in NAMES {
            data.extend_from_slice(name.as_bytes());
        }
        let export_map_offset = data.len() as i32;
        // 两个导出：对象名索引 1/2、长度 4、虚拟偏移按索引顺序。
        for (object_index, virtual_offset) in [(1u32, 0u64), (2u32, 4u64)] {
            data.extend_from_slice(&virtual_offset.to_le_bytes());
            data.extend_from_slice(&4u64.to_le_bytes());
            data.extend_from_slice(&object_index.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&(3u64 << 62).to_le_bytes()); // OuterIndex = None
            data.extend_from_slice(&(3u64 << 62).to_le_bytes()); // ClassIndex = None
            data.extend_from_slice(&0u64.to_le_bytes());
            data.extend_from_slice(&0u64.to_le_bytes());
            data.extend_from_slice(&0u64.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&[0u8; 4]);
        }
        let export_bundle_entries_offset = data.len() as i32;
        // Serialize(1)、Create(1)、Serialize(0)、Create(0)：导出 1 的数据在前。
        for (local, command) in [(1u32, 1u32), (1, 0), (0, 1), (0, 0)] {
            data.extend_from_slice(&local.to_le_bytes());
            data.extend_from_slice(&command.to_le_bytes());
        }
        let graph_data_offset = data.len() as i32;
        // 一个 bundle 头：SerialOffset 0、FirstEntryIndex 0、EntryCount 4。
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        let header_size = data.len() as i32;
        data.extend_from_slice(&[0xBB; 4]); // 导出 1 的载荷
        data.extend_from_slice(&[0xAA; 4]); // 导出 0 的载荷

        let cooked_header_size = header_size + 13;
        data[4..8].copy_from_slice(&(header_size as u32).to_le_bytes());
        data[20..24].copy_from_slice(&(cooked_header_size as u32).to_le_bytes());
        data[24..28].copy_from_slice(&export_map_offset.to_le_bytes()); // 公共导出哈希表为空
        data[28..32].copy_from_slice(&export_map_offset.to_le_bytes()); // ImportMap 为空
        data[32..36].copy_from_slice(&export_map_offset.to_le_bytes());
        data[36..40].copy_from_slice(&export_bundle_entries_offset.to_le_bytes());
        data[40..44].copy_from_slice(&graph_data_offset.to_le_bytes());
        data
    }

    /// 导出数据按 bundle 的 `Serialize` 顺序定位，而不是按 `CookedSerialOffset`。
    #[test]
    fn lays_out_export_data_in_bundle_order() {
        let package =
            parse_package(bundle_order_package(), None, Some(2)).expect("bundle 顺序包应解析成功");
        let info = package.info();
        assert_eq!(info.exports.len(), 2);
        assert_eq!(info.exports[0].name, "ExportA");
        assert_eq!(info.exports[1].name, "ExportB");
        assert_eq!(package.export_data(0), Some([0xAA; 4].as_slice()));
        assert_eq!(package.export_data(1), Some([0xBB; 4].as_slice()));
    }

    /// 头版本和偏移校验拒绝明显损坏的输入。
    #[test]
    fn corrupt_zen_inputs_are_rejected() {
        let data = vec![0u8; 128];
        assert!(parse_package(data, None, Some(5)).is_err());
    }
}
