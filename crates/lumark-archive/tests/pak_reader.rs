//! Unreal PAK 按条目读取器测试。
//!
//! 覆盖普通索引、压缩条目、加密索引、仅数据加密和缺失条目；样本来自
//! `tests/fixtures/unreal/`（见该目录 NOTICE）。

use std::path::PathBuf;

use lumark_archive::{ArchiveError, PakAssetReader};

/// 测试样本目录中的 Unreal PAK。
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("unreal")
        .join(name)
}

/// 加密样本使用的 AES-256 密钥。
const UNREAL_KEY: &str = "94d25bc3aeb420e0be914edc9d5435a1eaab5f2864e09e94019ac205b727a7de";

/// 普通与压缩 PAK 都能按路径读取，且内容一致。
#[test]
fn reads_plain_and_compressed_entries() {
    let mut plain = PakAssetReader::open(fixture("pack_v11.pak"), None).expect("打开普通 PAK");
    assert!(plain.entry_count() >= 4);
    let paths = plain.paths();
    for expected in ["test.txt", "test.png", "zeros.bin", "directory/nested.txt"] {
        assert!(paths.iter().any(|path| path == expected), "缺少 {expected}");
    }
    assert!(!plain.encrypted());
    assert!(!plain.needs_oodle());
    assert!(plain.compression_methods().is_empty());

    let text = plain.read_entry("test.txt").expect("读取 test.txt");
    assert!(!text.is_empty());
    assert_eq!(plain.entry_size("test.txt"), Some(text.len() as u64));

    let mut compressed =
        PakAssetReader::open(fixture("pack_v11_compress.pak"), None).expect("打开压缩 PAK");
    assert!(
        compressed
            .compression_methods()
            .iter()
            .any(|method| method == "Zlib"),
        "压缩样本应声明 Zlib：{:?}",
        compressed.compression_methods()
    );
    assert_eq!(
        compressed.read_entry("test.txt").expect("读取压缩条目"),
        text,
        "普通与压缩样本的 test.txt 内容应一致"
    );

    assert!(matches!(
        plain.read_entry("missing.txt"),
        Err(ArchiveError::InvalidRequest(_))
    ));
    assert_eq!(plain.entry_size("missing.txt"), None);
}

/// 加密索引：缺少密钥、错误密钥和正确密钥的行为。
#[test]
fn encrypted_index_requires_correct_password() {
    let path = fixture("pack_v11_encrypt_encryptindex.pak");
    assert!(matches!(
        PakAssetReader::open(&path, None),
        Err(ArchiveError::PasswordRequired)
    ));
    assert!(matches!(
        PakAssetReader::open(&path, Some("not-a-key")),
        Err(ArchiveError::WrongPassword)
    ));
    let mut reader = PakAssetReader::open(&path, Some(UNREAL_KEY)).expect("正确密钥打开");
    assert!(reader.encrypted());
    assert!(!reader.read_entry("test.txt").expect("读取条目").is_empty());
}

/// 仅数据加密：打开不需要密钥，读取加密条目时才要求密码。
#[test]
fn data_encrypted_pak_requires_password_on_read() {
    let path = fixture("pack_v11_encrypt.pak");
    let mut without_key = PakAssetReader::open(&path, None).expect("索引未加密应可打开");
    assert!(!without_key.encrypted());
    assert!(matches!(
        without_key.read_entry("test.txt"),
        Err(ArchiveError::PasswordRequired)
    ));

    let mut with_key =
        PakAssetReader::open(&path, Some(UNREAL_KEY)).expect("正确密钥打开数据加密 PAK");
    let mut plain = PakAssetReader::open(fixture("pack_v11.pak"), None).expect("打开普通 PAK");
    assert_eq!(
        with_key.read_entry("test.txt").expect("读取加密条目"),
        plain.read_entry("test.txt").expect("读取普通条目"),
        "加密与普通样本内容应一致"
    );
}

/// 不存在的文件返回可读的损坏错误，而不是 panic。
#[test]
fn missing_file_returns_corrupt_error() {
    assert!(matches!(
        PakAssetReader::open(fixture("does-not-exist.pak"), None),
        Err(ArchiveError::Corrupt(_))
    ));
}
