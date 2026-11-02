//! `GameAssetSource` 测试：直接使用 `lumark-archive` 的 Unreal PAK 样本。
//!
//! 样本目录见 `crates/lumark-archive/tests/fixtures/unreal/NOTICE.txt`；测试只读。

use std::io::Write;

use lumark_unreal::AssetContainerKind;
use lumark_unreal::source::{AssetSourceError, GameAssetOptions, GameAssetSource};

/// 返回 Unreal PAK 样本路径。
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("lumark-archive")
        .join("tests")
        .join("fixtures")
        .join("unreal")
        .join(name)
}

/// 加密样本使用的 AES-256 密钥。
const UNREAL_KEY: &str = "94d25bc3aeb420e0be914edc9d5435a1eaab5f2864e09e94019ac205b727a7de";

/// 普通 PAK：列表、存在性判断与读取。
#[test]
fn lists_and_reads_plain_pak() {
    let mut source =
        GameAssetSource::open(fixture("pack_v11.pak"), GameAssetOptions::default()).expect("打开");
    assert_eq!(source.kind(), AssetContainerKind::Pak);
    assert!(!source.encrypted());
    assert!(!source.needs_oodle());
    assert!(source.warnings().is_empty());
    assert!(source.entry_count() >= 4);
    assert!(source.list().iter().any(|entry| entry.path == "test.txt"));
    assert!(source.contains("test.txt"));
    assert!(!source.contains("missing.txt"));
    let text = source.read_bytes("test.txt").expect("读取");
    assert!(!text.is_empty());
    // 不以 .uasset 结尾的路径没有侧车文件。
    let inputs = source.read_package("test.txt").expect("读取资产包");
    assert_eq!(inputs.uasset, text);
    assert!(inputs.uexp.is_none());
    assert!(inputs.ubulk.is_none());
}

/// 压缩 PAK：声明 Zlib 且内容与普通样本一致。
#[test]
fn reads_compressed_pak() {
    let mut compressed = GameAssetSource::open(
        fixture("pack_v11_compress.pak"),
        GameAssetOptions::default(),
    )
    .expect("打开压缩 PAK");
    assert!(
        compressed
            .compression_methods()
            .iter()
            .any(|method| method == "Zlib")
    );
    let mut plain =
        GameAssetSource::open(fixture("pack_v11.pak"), GameAssetOptions::default()).expect("打开");
    assert_eq!(
        compressed.read_bytes("test.txt").expect("读取压缩条目"),
        plain.read_bytes("test.txt").expect("读取普通条目")
    );
}

/// 加密索引：缺少密钥和错误密钥分别返回可分类错误。
#[test]
fn encrypted_index_reports_password_errors() {
    let path = fixture("pack_v11_encrypt_encryptindex.pak");
    assert!(matches!(
        GameAssetSource::open(&path, GameAssetOptions::default()),
        Err(AssetSourceError::PasswordRequired)
    ));
    assert!(matches!(
        GameAssetSource::open(
            &path,
            GameAssetOptions {
                key: Some("not-a-key"),
                ..Default::default()
            }
        ),
        Err(AssetSourceError::WrongPassword)
    ));
    let mut source = GameAssetSource::open(
        &path,
        GameAssetOptions {
            key: Some(UNREAL_KEY),
            ..Default::default()
        },
    )
    .expect("正确密钥打开");
    assert!(source.encrypted());
    assert!(!source.read_bytes("test.txt").expect("读取").is_empty());
}

/// 仅数据加密：打开不需要密钥，读取加密条目时才要求密码。
#[test]
fn data_encrypted_pak_requires_password_on_read() {
    let path = fixture("pack_v11_encrypt.pak");
    let mut source = GameAssetSource::open(&path, GameAssetOptions::default()).expect("打开");
    assert!(matches!(
        source.read_bytes("test.txt"),
        Err(AssetSourceError::PasswordRequired)
    ));
}

/// 不支持的扩展名与不存在的文件返回可分类错误。
#[test]
fn unsupported_and_missing_inputs() {
    let directory = tempfile::tempdir().expect("临时目录");
    let other = directory.path().join("assets.bin");
    let mut file = std::fs::File::create(&other).expect("创建文件");
    file.write_all(b"not a container").expect("写入");
    drop(file);
    assert!(matches!(
        GameAssetSource::open(&other, GameAssetOptions::default()),
        Err(AssetSourceError::Unsupported(_))
    ));
    assert!(matches!(
        GameAssetSource::open(
            directory.path().join("missing.pak"),
            GameAssetOptions::default()
        ),
        Err(AssetSourceError::Io(_))
    ));
}
