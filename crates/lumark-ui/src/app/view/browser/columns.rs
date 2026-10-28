/// 归档视图在名称、大小、修改时间之外追加的列。
#[derive(Clone, Copy, Default)]
pub(super) struct ArchiveColumns {
    pub(super) compressed: bool,
    pub(super) algorithm: bool,
    pub(super) crc32: bool,
    pub(super) path: bool,
}

pub(super) const COMPRESSED_WIDTH: f32 = 80.;
pub(super) const ALGORITHM_WIDTH: f32 = 76.;
pub(super) const CRC_WIDTH: f32 = 72.;
pub(super) const PATH_WIDTH: f32 = 150.;

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
