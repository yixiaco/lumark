//! 验证本地 xz2 兼容层覆盖 geezipx 使用的 XZ/LZMA 路径。

use std::io::Cursor;

#[test]
fn xz_and_lzma_round_trip_through_compat_layer() {
    let data = b"local xz2 shim round trip".repeat(1024);
    let mut compressed = Vec::new();
    geezipx_core::archive::xz::xz_compress(&mut data.as_slice(), &mut compressed).unwrap();
    let mut decoded = Vec::new();
    geezipx_core::archive::xz::xz_decompress(&mut Cursor::new(compressed), &mut decoded).unwrap();
    assert_eq!(decoded, data);

    let mut lzma = Vec::new();
    geezipx_core::archive::xz::lzma_compress(&mut data.as_slice(), &mut lzma).unwrap();
    let mut decoded_lzma = Vec::new();
    geezipx_core::archive::xz::lzma_decompress(&mut Cursor::new(lzma), &mut decoded_lzma).unwrap();
    assert_eq!(decoded_lzma, data);
}
