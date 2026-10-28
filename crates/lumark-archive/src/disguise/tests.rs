use super::*;

/// 生成指定长度的伪随机测试载荷。
fn payload(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| ((index * 37 + 11) % 251) as u8)
        .collect()
}

/// 内置面具必须是可识别的媒体文件，且体积受控。
#[test]
fn preset_assets_are_valid_media() {
    assert!(
        (1024..=64 * 1024).contains(&PRESET_VIDEO.len()),
        "MP4 面具大小异常：{}",
        PRESET_VIDEO.len()
    );
    assert_eq!(&PRESET_VIDEO[4..8], b"ftyp", "MP4 面具缺少 ftyp box");
    assert!(
        (1024..=64 * 1024).contains(&PRESET_IMAGE.len()),
        "JPG 面具大小异常：{}",
        PRESET_IMAGE.len()
    );
    assert_eq!(&PRESET_IMAGE[..3], &[0xff, 0xd8, 0xff], "JPG 面具缺少 SOI");
    assert_eq!(
        DisguiseMask::PresetVideo.extension().unwrap(),
        "mp4",
        "视频面具扩展名"
    );
    assert_eq!(
        DisguiseMask::PresetImage.extension().unwrap(),
        "jpg",
        "图片面具扩展名"
    );
}

/// 尾部信息必须能完整读回原名、类型、偏移和长度。
#[test]
fn trailer_round_trips() {
    let salt = [7u8; 32];
    let trailer = build_trailer(1024, 4096, "报告.zip", DisguisePayloadKind::Archive, &salt);
    let info = parse_trailer(&trailer, 1024 + 4096 + DISGUISE_TRAILER_LEN as u64)
        .unwrap()
        .expect("尾部信息");
    assert_eq!(info.mask_len, 1024);
    assert_eq!(info.payload_len, 4096);
    assert_eq!(info.original_name, "报告.zip");
    assert_eq!(info.payload_kind, DisguisePayloadKind::Archive);
}

/// 魔数不匹配时视为普通文件；魔数匹配但字段损坏时必须报错。
#[test]
fn trailer_detects_corruption() {
    let salt = [1u8; 32];
    let mut trailer = build_trailer(512, 8, "a.bin", DisguisePayloadKind::RawFile, &salt);
    assert!(parse_trailer(&trailer, 512 + 8 + 512).unwrap().is_some());
    trailer[0] = b'X';
    assert!(parse_trailer(&trailer, 1032).unwrap().is_none());
    let mut trailer = build_trailer(512, 8, "a.bin", DisguisePayloadKind::RawFile, &salt);
    trailer[8] = 9;
    assert!(parse_trailer(&trailer, 1032).is_err());
    let mut trailer = build_trailer(512, 8, "a.bin", DisguisePayloadKind::RawFile, &salt);
    trailer[20] = 0xff;
    assert!(parse_trailer(&trailer, 1032).is_err());
}

/// 包含路径分隔符的原文件名必须被拒绝，避免还原时写出目标目录。
#[test]
fn unsafe_names_are_rejected() {
    assert!(safe_file_name("../escape.zip").is_err());
    assert!(safe_file_name("nested/file.zip").is_err());
    assert!(safe_file_name("CON").is_err());
    assert!(safe_file_name("ok.zip").is_ok());
}

/// 密钥流必须按逻辑位置确定性生成，随机偏移读取结果与写入一致。
#[test]
fn payload_view_supports_random_access() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("payload.bin");
    let bytes = payload(300 * 1024 + 17);
    fs::write(&source, &bytes).unwrap();
    let output = directory.path().join("cover.jpg");
    wrap(
        &source,
        &output,
        &DisguiseMask::PresetImage,
        "payload.bin",
        DisguisePayloadKind::RawFile,
        None,
        |_, _| {},
    )
    .unwrap();
    let info = probe(&output).unwrap().expect("伪装文件");
    assert_eq!(info.payload_len, bytes.len() as u64);
    let mut reader = open_payload(&output).unwrap();
    let mut buffer = vec![0u8; 300 * 1024 + 17];
    reader.seek(SeekFrom::Start(12345)).unwrap();
    reader.read_exact(&mut buffer[..4096]).unwrap();
    assert_eq!(&buffer[..4096], &bytes[12345..12345 + 4096]);
    reader.seek(SeekFrom::Start(0)).unwrap();
    let mut all = Vec::new();
    reader.read_to_end(&mut all).unwrap();
    assert_eq!(all, bytes);
}

/// 载荷区域不应出现 ZIP 头等明文特征。
#[test]
fn payload_is_obfuscated() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("payload.zip");
    let bytes = payload(64 * 1024);
    fs::write(&source, &bytes).unwrap();
    let output = directory.path().join("cover.mp4");
    wrap(
        &source,
        &output,
        &DisguiseMask::PresetVideo,
        "payload.zip",
        DisguisePayloadKind::Archive,
        None,
        |_, _| {},
    )
    .unwrap();
    let info = probe(&output).unwrap().expect("伪装文件");
    let raw = fs::read(&output).unwrap();
    let start = info.mask_len as usize;
    assert_ne!(&raw[start..start + 4], b"PK\x03\x04");
    assert_ne!(&raw[start..start + bytes.len()], bytes.as_slice());
}

/// 还原必须写回原名和原字节；同名文件使用唯一名称。
#[test]
fn restore_round_trips_and_avoids_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("payload.bin");
    let bytes = payload(1024);
    fs::write(&source, &bytes).unwrap();
    let output = directory.path().join("cover.jpg");
    wrap(
        &source,
        &output,
        &DisguiseMask::PresetImage,
        "restored.bin",
        DisguisePayloadKind::RawFile,
        None,
        |_, _| {},
    )
    .unwrap();
    let target = directory.path().join("out");
    let report = restore(&output, &target, None, |_, _| {}).unwrap();
    assert_eq!(report.output, target.join("restored.bin"));
    assert_eq!(fs::read(&report.output).unwrap(), bytes);
    fs::write(target.join("restored.bin"), b"existing").unwrap();
    let report = restore(&output, &target, None, |_, _| {}).unwrap();
    assert_eq!(report.output, target.join("restored (1).bin"));
}

/// 自定义面具必须存在、有扩展名且不能是伪装文件。
#[test]
fn custom_mask_validation() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.jpg");
    assert!(DisguiseMask::File(missing).validate().is_err());
    let tiny = directory.path().join("tiny.jpg");
    fs::write(&tiny, b"tiny").unwrap();
    assert!(DisguiseMask::File(tiny).validate().is_err());
    let no_extension = directory.path().join("mask");
    fs::write(&no_extension, vec![0x42; 4096]).unwrap();
    let error = DisguiseMask::File(no_extension).validate().unwrap_err();
    assert!(matches!(error, ArchiveError::InvalidRequest(_)));
    let valid = directory.path().join("mask.jpg");
    fs::write(&valid, vec![0x42; 4096]).unwrap();
    assert!(DisguiseMask::File(valid).validate().is_ok());
}

/// 物化缓存应复用同一路径，并在文件变化后失效。
#[test]
fn materialize_reuses_cache() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("payload.bin");
    fs::write(&source, payload(2048)).unwrap();
    let output = directory.path().join("cover.jpg");
    wrap(
        &source,
        &output,
        &DisguiseMask::PresetImage,
        "payload.bin",
        DisguisePayloadKind::RawFile,
        None,
        |_, _| {},
    )
    .unwrap();
    clear_materialized_cache();
    let first = materialize(&output).unwrap();
    let second = materialize(&output).unwrap();
    assert_eq!(first, second);
    assert_eq!(fs::read(&first).unwrap(), payload(2048));
    // 重写伪装文件后盐/时间变化，旧缓存不能继续命中。
    std::thread::sleep(Duration::from_millis(20));
    let mut handle = OpenOptions::new().write(true).open(&output).unwrap();
    handle.write_all(b"changed").unwrap();
    handle.sync_all().unwrap();
    drop(handle);
    let third = materialize(&output);
    assert!(third.is_err() || third.unwrap() != first);
    clear_materialized_cache();
}
