//! 自解压自定义图标的真实运行时模块集成测试。

#![cfg(windows)]

use std::fs;

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions};
use tempfile::tempdir;

/// 生成一个 1x1、32 位的最小 ICO。
fn tiny_ico() -> Vec<u8> {
    let mut image = Vec::new();
    image.extend_from_slice(&40u32.to_le_bytes());
    image.extend_from_slice(&1i32.to_le_bytes());
    image.extend_from_slice(&2i32.to_le_bytes());
    image.extend_from_slice(&1u16.to_le_bytes());
    image.extend_from_slice(&32u16.to_le_bytes());
    image.extend_from_slice(&0u32.to_le_bytes());
    image.extend_from_slice(&4u32.to_le_bytes());
    image.extend_from_slice(&[0u8; 16]);
    image.extend_from_slice(&[0u8; 4]);
    image.extend_from_slice(&[0u8; 4]);
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&[1, 1, 0, 0]);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend(image);
    ico
}

/// 验证真实 PE 模块替换图标后，页脚仍能定位并解压载荷。
#[test]
fn sfx_icon_replacement_keeps_payload_addressable() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("hello.txt");
    fs::write(&source, b"icon payload").unwrap();
    let stub = directory.path().join("stub.exe");
    fs::copy(std::env::current_exe().unwrap(), &stub).unwrap();
    let icon = directory.path().join("icon.ico");
    fs::write(&icon, tiny_ico()).unwrap();
    let output = directory.path().join("bundle.exe");
    ArchiveService::create_sfx_with_icon(
        &output,
        std::slice::from_ref(&source),
        &CompressionOptions {
            format: ArchiveFormat::Zip,
            sfx: true,
            sfx_icon: Some(icon),
            ..Default::default()
        },
        &stub,
        |_| {},
        |path, icon| {
            lumark_platform_windows::icon::replace_icon(path, icon)
                .map_err(|error| lumark_archive::ArchiveError::Corrupt(error.to_string()))
        },
    )
    .unwrap();
    let mut file = fs::File::open(&output).unwrap();
    let footer = lumark_sfx::read_footer(&mut file).unwrap().expect("页脚");
    assert!(footer.offset >= fs::metadata(&stub).unwrap().len());
    let target = directory.path().join("out");
    let files = lumark_sfx::extract_payload(file, footer, &target).unwrap();
    assert_eq!(files, 1);
    assert_eq!(
        fs::read_to_string(target.join("hello.txt")).unwrap(),
        "icon payload"
    );
}
