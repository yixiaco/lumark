//! 传统格式完整性测试结果类型。
//!
//! 本地适配层只保留 CPIO 需要的 [`TestReport`]；上游其余的 ZIP/TAR/单流
//! 验证函数不会参与编译。

use crate::detect::ArchiveFormat;

/// 一次完整性测试的结果。
#[derive(Debug, Clone)]
pub struct TestReport {
    /// 归档格式。
    pub format: ArchiveFormat,
    /// 已验证的条目数。
    pub entry_count: u64,
    /// 读取的未压缩字节数。
    pub bytes_read: u64,
    /// 是否验证了逐条目 CRC-32。
    pub crc32_verified: bool,
}
