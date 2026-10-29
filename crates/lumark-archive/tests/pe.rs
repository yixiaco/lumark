//! PE 只读归档的列表、图标重建和解压测试。

use lumark_archive::{ArchiveFormat, ArchiveService, ExtractionOptions};

/// 构造一个带 `.text`、`.rsrc`（一个 RT_ICON）和覆盖数据的最小 PE32。
fn pe_with_resources() -> Vec<u8> {
    let mut bytes = vec![0u8; 0x900];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    let coff = 0x84;
    bytes[coff..coff + 2].copy_from_slice(&0x014cu16.to_le_bytes());
    bytes[coff + 2..coff + 4].copy_from_slice(&2u16.to_le_bytes());
    bytes[coff + 16..coff + 18].copy_from_slice(&0xe0u16.to_le_bytes());
    let optional = coff + 20;
    bytes[optional..optional + 2].copy_from_slice(&0x10bu16.to_le_bytes());
    bytes[optional + 16..optional + 20].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[optional + 28..optional + 32].copy_from_slice(&0x400000u32.to_le_bytes());
    bytes[optional + 68..optional + 70].copy_from_slice(&3u16.to_le_bytes());
    bytes[optional + 92..optional + 96].copy_from_slice(&16u32.to_le_bytes());
    let resource_dir = optional + 96 + 16;
    bytes[resource_dir..resource_dir + 4].copy_from_slice(&0x2000u32.to_le_bytes());
    bytes[resource_dir + 4..resource_dir + 8].copy_from_slice(&0x100u32.to_le_bytes());
    let section = optional + 0xe0;
    bytes[section..section + 5].copy_from_slice(b".text");
    bytes[section + 8..section + 12].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[section + 16..section + 20].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[section + 20..section + 24].copy_from_slice(&0x400u32.to_le_bytes());
    let rsrc = section + 40;
    bytes[rsrc..rsrc + 5].copy_from_slice(b".rsrc");
    bytes[rsrc + 8..rsrc + 12].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[rsrc + 12..rsrc + 16].copy_from_slice(&0x2000u32.to_le_bytes());
    bytes[rsrc + 16..rsrc + 20].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[rsrc + 20..rsrc + 24].copy_from_slice(&0x600u32.to_le_bytes());

    // 资源目录：类型 ICON(3) -> 名称 1 -> 语言 0x409 -> 数据。
    let root = 0x600;
    bytes[root + 14..root + 16].copy_from_slice(&1u16.to_le_bytes());
    bytes[root + 16..root + 20].copy_from_slice(&3u32.to_le_bytes());
    bytes[root + 20..root + 24].copy_from_slice(&0x8000_0018u32.to_le_bytes());
    let name = root + 0x18;
    bytes[name + 14..name + 16].copy_from_slice(&1u16.to_le_bytes());
    bytes[name + 16..name + 20].copy_from_slice(&1u32.to_le_bytes());
    bytes[name + 20..name + 24].copy_from_slice(&0x8000_0030u32.to_le_bytes());
    let language = root + 0x30;
    bytes[language + 14..language + 16].copy_from_slice(&1u16.to_le_bytes());
    bytes[language + 16..language + 20].copy_from_slice(&0x409u32.to_le_bytes());
    bytes[language + 20..language + 24].copy_from_slice(&0x48u32.to_le_bytes());
    let data = root + 0x48;
    bytes[data..data + 4].copy_from_slice(&0x2060u32.to_le_bytes());
    bytes[data + 4..data + 8].copy_from_slice(&44u32.to_le_bytes());

    // 1x1 32bpp DIB：biHeight 为图标高度两倍。
    let icon = root + 0x60;
    bytes[icon..icon + 4].copy_from_slice(&40u32.to_le_bytes());
    bytes[icon + 4..icon + 8].copy_from_slice(&1i32.to_le_bytes());
    bytes[icon + 8..icon + 12].copy_from_slice(&2i32.to_le_bytes());
    bytes[icon + 12..icon + 14].copy_from_slice(&1u16.to_le_bytes());
    bytes[icon + 14..icon + 16].copy_from_slice(&32u16.to_le_bytes());
    bytes[icon + 20..icon + 24].copy_from_slice(&4u32.to_le_bytes());

    bytes[0x400..0x600].fill(0x11);
    bytes[0x800..0x900].fill(0xaa);
    bytes
}

#[test]
fn pe_lists_sections_resources_and_overlay() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, pe_with_resources()).unwrap();

    assert_eq!(ArchiveService::detect(&file), ArchiveFormat::Pe);
    let info = ArchiveService::list(&file, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Pe));
    assert!(
        info.entries
            .iter()
            .any(|entry| { entry.path == ".text" && entry.is_dir && entry.size == 0x200 })
    );
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == ".text/[0]" && entry.size == 0x200)
    );
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == ".rsrc/ICON/1.ico" && entry.size == 66),
        "entries: {:?}",
        info.entries
    );
    assert!(
        info.entries
            .iter()
            .any(|entry| { entry.path == ".rsrc/ICON" && entry.is_dir && entry.size == 66 })
    );
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "[0]" && entry.size == 0x100)
    );
    assert_eq!(info.total_size, 0x900);
}

#[test]
fn pe_extracts_icons_and_sections() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, pe_with_resources()).unwrap();
    let output = directory.path().join("out");
    let options = ExtractionOptions {
        output_dir: output.clone(),
        overwrite: true,
        ..Default::default()
    };
    let report = ArchiveService::extract(&file, &options, |_| {}).unwrap();
    assert_eq!(report.files, 3);
    assert_eq!(report.bytes, 0x342);
    let icon = std::fs::read(output.join(".rsrc/ICON/1.ico")).unwrap();
    assert_eq!(&icon[..6], &[0, 0, 1, 0, 1, 0]);
    assert_eq!(icon.len(), 66);
    assert_eq!(
        std::fs::read(output.join(".text/[0]")).unwrap().len(),
        0x200
    );
    assert_eq!(std::fs::read(output.join("[0]")).unwrap().len(), 0x100);
}

#[test]
fn pe_test_reports_all_entries() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("sample.exe");
    std::fs::write(&file, pe_with_resources()).unwrap();
    let report = ArchiveService::test(&file, None, |_| {}).unwrap();
    assert_eq!(report.files, 3);
    assert_eq!(report.bytes, 0x342);
}
