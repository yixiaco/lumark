//! 按资产类型搜索的回归测试。

use lumark_browser::{AssetCategory, BrowserModel, SearchMode, SearchQuery};

/// 空搜索文本 + 类型过滤应只返回该类型文件。
#[test]
fn search_filters_by_asset_category() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for (relative, bytes) in [
        ("photo.png", b"png".as_slice()),
        ("hero.fbx", b"fbx"),
        ("readme.txt", b"txt"),
        ("Model/hero.uasset", b"uasset"),
        ("Texture/T_hero.uasset", b"texture"),
        ("Anim/Idle.uasset", b"anim"),
    ] {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }
    let browser = BrowserModel::new(root).unwrap();
    let search = |category: Option<AssetCategory>| {
        browser.search(&SearchQuery {
            pattern: String::new(),
            recursive: true,
            case_sensitive: false,
            mode: SearchMode::Contains,
            category,
            ..Default::default()
        })
    };

    let images = search(Some(AssetCategory::Image));
    assert!(
        images.iter().any(|entry| entry.name == "photo.png"),
        "PNG 应归为图片"
    );
    assert!(
        images.iter().any(|entry| entry.name == "T_hero.uasset"),
        "Texture 目录下的 uasset 应归为图片"
    );
    assert!(!images.iter().any(|entry| entry.name == "hero.fbx"));

    let models = search(Some(AssetCategory::Model));
    assert!(models.iter().any(|entry| entry.name == "hero.fbx"));
    assert!(models.iter().any(|entry| entry.name == "hero.uasset"));

    let animations = search(Some(AssetCategory::Animation));
    assert!(animations.iter().any(|entry| entry.name == "Idle.uasset"));

    let texts = search(Some(AssetCategory::Text));
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].name, "readme.txt");

    // 没有类型过滤且没有搜索文本时仍保持“无搜索”语义。
    assert!(search(None).is_empty());
}
