//! `.pak` 家族识别与 Quake/Chromium 读取测试。

use std::fs;
use std::path::PathBuf;

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};
use tempfile::tempdir;

/// 写出一个包含两个条目的 Quake PACK。
fn write_quake_pak(path: &std::path::Path) {
    let files = [
        ("maps/test.txt", b"quake-data".as_slice()),
        ("readme.txt", b"hello".as_slice()),
    ];
    let directory_offset = 12u32;
    let directory_length = (files.len() * 64) as u32;
    let mut data_offset = directory_offset + directory_length;
    let mut directory = Vec::new();
    let mut payload = Vec::new();
    for (name, contents) in files {
        let mut item = [0u8; 64];
        item[..name.len()].copy_from_slice(name.as_bytes());
        item[56..60].copy_from_slice(&data_offset.to_le_bytes());
        item[60..64].copy_from_slice(&(contents.len() as u32).to_le_bytes());
        directory.extend_from_slice(&item);
        payload.extend_from_slice(contents);
        data_offset += contents.len() as u32;
    }
    let mut output = Vec::new();
    output.extend_from_slice(b"PACK");
    output.extend_from_slice(&directory_offset.to_le_bytes());
    output.extend_from_slice(&directory_length.to_le_bytes());
    output.extend_from_slice(&directory);
    output.extend_from_slice(&payload);
    fs::write(path, output).unwrap();
}

/// 写出一个 Chromium data pack（v4 或 v5），包含一个别名。
fn write_chromium_pak(path: &std::path::Path, version: u32) {
    let resources = [
        (100u16, b"PNGDATA".as_slice()),
        (200u16, b"text".as_slice()),
    ];
    let header_len = if version == 4 { 9 } else { 12 };
    let mut table_end = header_len + (resources.len() + 1) * 6 + 4;
    let mut table = Vec::new();
    let mut payload = Vec::new();
    for (id, contents) in resources {
        table.extend_from_slice(&id.to_le_bytes());
        table.extend_from_slice(&(table_end as u32).to_le_bytes());
        payload.extend_from_slice(contents);
        table_end += contents.len();
    }
    // 哨兵项给出最后一个资源的结束偏移。
    table.extend_from_slice(&0u16.to_le_bytes());
    table.extend_from_slice(&(table_end as u32).to_le_bytes());
    // 别名 300 -> 资源 0。
    let mut aliases = Vec::new();
    aliases.extend_from_slice(&300u16.to_le_bytes());
    aliases.extend_from_slice(&0u16.to_le_bytes());

    let mut output = Vec::new();
    output.extend_from_slice(&version.to_le_bytes());
    if version == 4 {
        output.extend_from_slice(&(resources.len() as u32).to_le_bytes());
        output.push(1);
    } else {
        output.push(1);
        output.extend_from_slice(&[0u8; 3]);
        output.extend_from_slice(&(resources.len() as u16).to_le_bytes());
        output.extend_from_slice(&1u16.to_le_bytes());
    }
    output.extend_from_slice(&table);
    output.extend_from_slice(&aliases);
    output.extend_from_slice(&payload);
    fs::write(path, output).unwrap();
}

/// 返回测试目录中的归档路径。
fn prep(name: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempdir().unwrap();
    let path = directory.path().join(name);
    (directory, path)
}

/// Unreal 测试样本目录。
fn unreal_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("unreal")
        .join(name)
}

/// 加密样本使用的 AES-256 密钥。
const UNREAL_KEY: &str = "94d25bc3aeb420e0be914edc9d5435a1eaab5f2864e09e94019ac205b727a7de";

/// 验证各版本 Unreal PAK 的识别、列表和解压。
#[test]
fn unreal_pak_versions_roundtrip() {
    for name in [
        "pack_v5.pak",
        "pack_v5_compress.pak",
        "pack_v7.pak",
        "pack_v8a.pak",
        "pack_v8b.pak",
        "pack_v9.pak",
        "pack_v11.pak",
        "pack_v11_compress.pak",
    ] {
        let path = unreal_fixture(name);
        assert_eq!(
            ArchiveService::detect(&path),
            ArchiveFormat::UnrealPak,
            "{name}"
        );
        let info = ArchiveService::list(&path, None).unwrap_or_else(|error| {
            panic!("{name} 列表失败：{error}");
        });
        assert_eq!(info.format, Some(ArchiveFormat::UnrealPak), "{name}");
        let paths: Vec<&str> = info
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        for expected in ["test.txt", "test.png", "zeros.bin", "directory/nested.txt"] {
            assert!(
                paths.contains(&expected),
                "{name} 缺少 {expected}，实际 {paths:?}"
            );
        }
        ArchiveService::test(&path, None, |_| {})
            .unwrap_or_else(|error| panic!("{name} 测试失败：{error}"));
        let directory = tempdir().unwrap();
        ArchiveService::extract(
            &path,
            &ExtractionOptions {
                output_dir: directory.path().to_path_buf(),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap_or_else(|error| panic!("{name} 解压失败：{error}"));
        let zeros = fs::read(directory.path().join("zeros.bin")).unwrap();
        assert!(
            zeros.iter().all(|byte| *byte == 0),
            "{name} zeros.bin 内容异常"
        );
        assert_eq!(
            fs::read(directory.path().join("test.txt")).unwrap().len(),
            info.entries
                .iter()
                .find(|entry| entry.path == "test.txt")
                .unwrap()
                .size as usize
        );
    }
}

/// 验证加密索引和条目：无密钥提示、正确密钥可读、错误密钥报错。
#[test]
fn unreal_pak_encrypted_roundtrip() {
    let path = unreal_fixture("pack_v11_encrypt_encryptindex.pak");
    assert_eq!(ArchiveService::detect(&path), ArchiveFormat::UnrealPak);
    let probe = ArchiveService::probe_encryption(&path).unwrap();
    assert!(probe.encrypted);
    assert!(probe.header_encrypted);
    assert!(matches!(
        ArchiveService::list(&path, None),
        Err(lumark_archive::ArchiveError::PasswordRequired)
    ));
    assert!(matches!(
        ArchiveService::list(&path, Some("not-a-key")),
        Err(lumark_archive::ArchiveError::WrongPassword)
    ));
    assert!(ArchiveService::verify_password(&path, UNREAL_KEY).unwrap());
    assert!(!ArchiveService::verify_password(&path, &"0".repeat(64)).unwrap());
    let info = ArchiveService::list(&path, Some(UNREAL_KEY)).unwrap();
    assert!(info.entries.iter().any(|entry| entry.path == "test.txt"));
    let directory = tempdir().unwrap();
    ArchiveService::extract(
        &path,
        &ExtractionOptions {
            output_dir: directory.path().to_path_buf(),
            password: Some(zeroize::Zeroizing::new(UNREAL_KEY.to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(directory.path().join("test.txt").is_file());
}

/// 验证数据加密但索引未加密的 PAK 可用同一密钥解压。
#[test]
fn unreal_pak_encrypted_data() {
    let path = unreal_fixture("pack_v11_encrypt.pak");
    let plain = unreal_fixture("pack_v11.pak");
    let directory = tempdir().unwrap();
    ArchiveService::extract(
        &path,
        &ExtractionOptions {
            output_dir: directory.path().join("encrypted"),
            password: Some(zeroize::Zeroizing::new(UNREAL_KEY.to_string())),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    ArchiveService::extract(
        &plain,
        &ExtractionOptions {
            output_dir: directory.path().join("plain"),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    for name in ["test.txt", "test.png", "zeros.bin", "directory/nested.txt"] {
        assert_eq!(
            fs::read(directory.path().join("encrypted").join(name)).unwrap(),
            fs::read(directory.path().join("plain").join(name)).unwrap(),
            "{name}"
        );
    }
}

/// 验证 Quake PACK 识别、列表、测试和解压。
#[test]
fn quake_pak_roundtrip() {
    let (directory, path) = prep("quake.pak");
    write_quake_pak(&path);
    assert_eq!(ArchiveService::detect(&path), ArchiveFormat::QuakePak);
    let info = ArchiveService::list(&path, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::QuakePak));
    assert_eq!(info.entries.len(), 2);
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "maps/test.txt")
    );
    ArchiveService::test(&path, None, |_| {}).unwrap();
    let output = directory.path().join("out");
    ArchiveService::extract(
        &path,
        &ExtractionOptions {
            output_dir: output.clone(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(output.join("maps/test.txt")).unwrap(),
        b"quake-data"
    );
}

/// 验证 Chromium v4/v5 识别与资源解压。
#[test]
fn chromium_pak_versions() {
    for version in [4u32, 5] {
        let (directory, path) = prep(&format!("resources-v{version}.pak"));
        write_chromium_pak(&path, version);
        assert_eq!(ArchiveService::detect(&path), ArchiveFormat::ChromiumPak);
        let info = ArchiveService::list(&path, None).unwrap();
        assert_eq!(info.format, Some(ArchiveFormat::ChromiumPak));
        assert_eq!(info.entries.len(), 2);
        assert!(
            info.entries
                .iter()
                .any(|entry| entry.path == "resources/100")
        );
        let output = directory.path().join("out");
        ArchiveService::extract(
            &path,
            &ExtractionOptions {
                output_dir: output.clone(),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(output.join("resources/100")).unwrap(), b"PNGDATA");
        assert_eq!(fs::read(output.join("resources/200")).unwrap(), b"text");
    }
}

/// 验证损坏的 PAK 家族头部不会 panic。
#[test]
fn corrupt_pak_headers_fail_cleanly() {
    let (_directory, path) = prep("broken.pak");
    fs::write(&path, b"PACK").unwrap();
    assert_eq!(ArchiveService::detect(&path), ArchiveFormat::QuakePak);
    assert!(ArchiveService::list(&path, None).is_err());

    let (_directory, path) = prep("broken-chromium.pak");
    fs::write(&path, [5u8, 0, 0, 0, 1, 0, 0, 0, 9, 0, 9, 0]).unwrap();
    // 头部明显越界时检测可以拒绝该变体；无论落到哪种格式，列表必须返回
    // 错误而不是 panic。
    assert!(ArchiveService::list(&path, None).is_err());
}
