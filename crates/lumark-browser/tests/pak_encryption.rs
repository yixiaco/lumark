//! 游戏容器加密标志回归测试。
//!
//! 加密 PAK 的列表结果必须带加密标志；浏览器据此在打开条目、解压和复制时
//! 自动补上游戏会话的 AES 密钥，否则游戏容器里的资产会被误报成“需要密码”。

use lumark_browser::BrowserModel;

/// 加密 PAK 打开后应标记为加密归档，普通 PAK 保持未加密。
#[test]
fn encrypted_pak_archive_reports_encryption() {
    const KEY: &str = "94d25bc3aeb420e0be914edc9d5435a1eaab5f2864e09e94019ac205b727a7de";
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lumark-archive/tests/fixtures/unreal");
    let encrypted = fixtures.join("pack_v11_encrypt_encryptindex.pak");
    let plain = fixtures.join("pack_v11.pak");
    if !encrypted.is_file() || !plain.is_file() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let mut browser = BrowserModel::new(directory.path()).unwrap();
    browser
        .open_archive(&encrypted, Some(KEY))
        .unwrap_or_else(|error| panic!("加密 PAK 列表失败：{error}"));
    assert!(browser.archive_encrypted(), "加密 PAK 必须标记为加密");
    assert!(
        browser.entries.iter().any(|entry| entry.name == "test.txt"),
        "加密 PAK 应能列出条目"
    );

    browser
        .open_archive(&plain, None)
        .unwrap_or_else(|error| panic!("普通 PAK 列表失败：{error}"));
    assert!(!browser.archive_encrypted());
}
