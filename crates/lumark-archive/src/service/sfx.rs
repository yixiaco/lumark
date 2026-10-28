use super::*;

/// 组装自解压包 v2 尾部页脚。
///
/// 布局：魔数 8 字节 + 格式 1 字节 + 标志 1 字节 + 保留 6 字节 +
/// 数据段偏移 8 字节 + 数据段长度 8 字节。格式 0 为 ZIP，1 为 7Z；
/// 标志 bit0 表示载荷使用密码。
pub(super) fn sfx_footer(
    offset: u64,
    length: u64,
    format: ArchiveFormat,
    encrypted: bool,
) -> [u8; SFX_FOOTER_LEN] {
    let mut footer = [0u8; SFX_FOOTER_LEN];
    footer[..8].copy_from_slice(&SFX_MAGIC_V2);
    footer[8] = match format {
        ArchiveFormat::SevenZip => 1,
        _ => 0,
    };
    footer[9] = u8::from(encrypted);
    footer[16..24].copy_from_slice(&offset.to_le_bytes());
    footer[24..32].copy_from_slice(&length.to_le_bytes());
    footer
}
