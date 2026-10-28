//! 文件表列宽和归档附加列可见性。

/// 归档视图在名称、大小、修改时间之外追加的列。
#[derive(Clone, Copy, Default)]
pub(super) struct ArchiveColumns {
    /// 是否显示压缩后大小列。
    pub(super) compressed: bool,
    /// 是否显示压缩算法列。
    pub(super) algorithm: bool,
    /// 是否显示 CRC32 列。
    pub(super) crc32: bool,
    /// 是否显示归档内路径列。
    pub(super) path: bool,
}

/// 按文件表可用宽度选择归档列；窄窗口优先隐藏路径、压缩算法和 CRC32。
///
/// 阈值按“名称至少保留 160 像素”估算，保证 960 像素最小窗口下仍能显示
/// 压缩后大小和 CRC32；隐藏哪些列只影响显示，不影响条目数据。
pub(super) fn archive_columns(width: f32) -> ArchiveColumns {
    ArchiveColumns {
        compressed: width >= 530.,
        crc32: width >= 600.,
        algorithm: width >= 680.,
        path: width >= 840.,
    }
}
