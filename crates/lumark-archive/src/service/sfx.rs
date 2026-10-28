use super::*;

/// 组装自解压包尾部页脚：魔数 + 数据段偏移 + 数据段长度。
pub(super) fn sfx_footer(offset: u64, length: u64) -> [u8; SFX_FOOTER_LEN] {
    let mut footer = [0u8; SFX_FOOTER_LEN];
    footer[..8].copy_from_slice(&SFX_MAGIC);
    footer[8..16].copy_from_slice(&offset.to_le_bytes());
    footer[16..24].copy_from_slice(&length.to_le_bytes());
    footer
}
