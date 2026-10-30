//! Unreal 容器共用的 AES 密钥、ECB 解密和分块解压。
//!
//! PAK 与 IoStore 使用同一套 AES-256-ECB 和压缩算法，但两者的索引结构
//! 完全不同；把纯算法部分集中在这里，避免两条读取路径各自复制一套实现。

use std::io::Read;

use base64::Engine as _;

/// 解码用户提供的 AES-256 密钥：64 位十六进制、Base64 或 32 字节原文。
///
/// 游戏提取器输出十六进制，手动输入也可能来自其它工具的 Base64；三种
/// 形式都要接受，但除此之外不做模糊猜测，避免把错误格式当成有效密钥。
pub(crate) fn decode_key(password: &str) -> Result<[u8; 32], String> {
    let value = password.trim();
    if value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        let mut output = [0u8; 32];
        for (index, chunk) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let text = std::str::from_utf8(chunk).map_err(|error| error.to_string())?;
            output[index] = u8::from_str_radix(text, 16).map_err(|error| error.to_string())?;
        }
        return Ok(output);
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(value)
        && bytes.len() == 32
    {
        let mut output = [0u8; 32];
        output.copy_from_slice(&bytes);
        return Ok(output);
    }
    let bytes = value.as_bytes();
    if bytes.len() == 32 {
        let mut output = [0u8; 32];
        output.copy_from_slice(bytes);
        return Ok(output);
    }
    Err("Unreal 密钥必须是 64 位十六进制、Base64 或 32 字节原文".to_string())
}

/// 就地执行 AES-256-ECB 解密。
///
/// Unreal 的索引和块数据都按 16 字节分组加密，且没有额外 IV；调用方负责
/// 保证缓冲区长度是 16 的倍数，并在需要时截断解密结果。
pub(crate) fn decrypt_ecb(key: &[u8; 32], bytes: &mut [u8]) -> Result<(), String> {
    if !bytes.len().is_multiple_of(16) {
        return Err("Unreal 加密数据长度不是 16 的倍数".to_string());
    }
    use aes::cipher::{BlockDecrypt, KeyInit};
    let cipher = aes::Aes256::new_from_slice(key).map_err(|error| error.to_string())?;
    for chunk in bytes.as_chunks_mut::<16>().0 {
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(&chunk[..]);
        cipher.decrypt_block(&mut block);
        chunk.copy_from_slice(&block);
    }
    Ok(())
}

/// 解压单个数据块。
///
/// 方法名来自容器元数据，大小写不固定；未知算法必须显式失败，不能把
/// 未解压的数据当成结果返回。Oodle 由用户提供的动态库完成。
pub(crate) fn decompress_block(
    compression: &str,
    input: &[u8],
    output: &mut [u8],
) -> Result<(), String> {
    if compression.eq_ignore_ascii_case("Store")
        || compression.eq_ignore_ascii_case("None")
        || compression.is_empty()
    {
        if input.len() != output.len() {
            return Err("Unreal 存储块大小不一致".to_string());
        }
        output.copy_from_slice(input);
        return Ok(());
    }
    if compression.eq_ignore_ascii_case("Zlib") {
        let mut decoder = flate2::read::ZlibDecoder::new(input);
        return decoder
            .read_exact(output)
            .map_err(|error| format!("Unreal Zlib 解压失败：{error}"));
    }
    if compression.eq_ignore_ascii_case("Gzip") {
        let mut decoder = flate2::read::GzDecoder::new(input);
        return decoder
            .read_exact(output)
            .map_err(|error| format!("Unreal Gzip 解压失败：{error}"));
    }
    if compression.eq_ignore_ascii_case("LZ4") {
        return lz4_flex::block::decompress_into(input, output)
            .map(|_| ())
            .map_err(|error| format!("Unreal LZ4 解压失败：{error}"));
    }
    if compression.eq_ignore_ascii_case("Zstd") {
        let mut decoder = zstd::stream::read::Decoder::new(input)
            .map_err(|error| format!("Unreal Zstd 解压失败：{error}"))?;
        return decoder
            .read_exact(output)
            .map_err(|error| format!("Unreal Zstd 解压失败：{error}"));
    }
    if compression.eq_ignore_ascii_case("Oodle") {
        return super::oodle::decompress(input, output);
    }
    Err(format!("Unreal 压缩算法不支持：{compression}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 密钥三种输入形式都能解析成同样的 32 字节。
    #[test]
    fn key_formats_are_accepted() {
        let key = [0x5Au8; 32];
        let hex = key
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(decode_key(&hex).unwrap(), key);
        let base64 = base64::engine::general_purpose::STANDARD.encode(key);
        assert_eq!(decode_key(&base64).unwrap(), key);
        assert!(decode_key("not-a-key").is_err());
    }

    /// 加密数据长度不是 16 的倍数时返回错误而不是崩溃。
    #[test]
    fn decrypt_rejects_unaligned_input() {
        assert!(decrypt_ecb(&[0; 32], &mut [0; 17]).is_err());
    }

    /// 存储块的输入输出长度必须一致。
    #[test]
    fn store_block_requires_matching_sizes() {
        let mut output = [0u8; 3];
        assert!(decompress_block("Store", &[1, 2, 3], &mut output).is_ok());
        assert_eq!(output, [1, 2, 3]);
        assert!(decompress_block("Store", &[1, 2], &mut output).is_err());
        assert!(decompress_block("Unknown", &[1, 2, 3], &mut output).is_err());
    }
}
