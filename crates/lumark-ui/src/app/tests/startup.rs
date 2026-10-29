//! 启动参数和单实例请求测试。

mod startup_tests {
    use crate::app::{StartupMode, StartupRequest};
    use std::path::PathBuf;

    /// 验证各种启动开关都能解析，并且路径原样保留。
    #[test]
    fn startup_request_parses_mode_and_paths() {
        let request = StartupRequest::parse(vec![
            "--extract".to_string(),
            r"C:\downloads\report.zip".to_string(),
        ])
        .expect("extract request");
        assert_eq!(request.mode, StartupMode::Extract);
        assert_eq!(
            request.paths,
            vec![PathBuf::from(r"C:\downloads\report.zip")]
        );

        let request = StartupRequest::parse(vec![
            "--compress".to_string(),
            r"C:\work\images".to_string(),
        ])
        .expect("compress request");
        assert_eq!(request.mode, StartupMode::Compress);

        let request = StartupRequest::parse(vec![
            "--disguise".to_string(),
            r"C:\work\images".to_string(),
        ])
        .expect("disguise request");
        assert_eq!(request.mode, StartupMode::Disguise);

        let request = StartupRequest::parse(vec![
            "--mount-iso".to_string(),
            r"C:\images\setup.iso".to_string(),
        ])
        .expect("mount iso request");
        assert_eq!(request.mode, StartupMode::MountIso);

        let request = StartupRequest::parse(vec![
            "--unmount-iso".to_string(),
            r"C:\images\setup.iso".to_string(),
        ])
        .expect("unmount iso request");
        assert_eq!(request.mode, StartupMode::UnmountIso);

        let request = StartupRequest::parse(vec![
            "--restore".to_string(),
            r"C:\downloads\cover.mp4".to_string(),
        ])
        .expect("restore request");
        assert_eq!(request.mode, StartupMode::Restore);
    }

    /// 验证一次请求包含多个归档时 targets 保留全部有效路径。
    #[test]
    fn multi_open_request_keeps_all_archives() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.zip");
        let second = directory.path().join("second.7z");
        let ignored = directory.path().join("notes.txt");
        std::fs::write(&first, b"zip").unwrap();
        std::fs::write(&second, b"7z").unwrap();
        std::fs::write(&ignored, b"text").unwrap();
        let request = StartupRequest::parse(vec![
            "--open".to_string(),
            first.to_string_lossy().into_owned(),
            ignored.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ])
        .expect("multi open request");
        let targets = request.targets();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0], first);
        assert_eq!(targets[1], second);
    }

    /// 验证还原请求接受任意存在的文件，目标缺失时返回空。
    #[test]
    fn restore_request_accepts_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cover.mp4");
        std::fs::write(&path, b"not a disguise").unwrap();
        let request = StartupRequest::parse(vec![
            "--restore".to_string(),
            path.to_string_lossy().into_owned(),
        ])
        .expect("restore request");
        assert_eq!(request.mode, StartupMode::Restore);
        assert_eq!(request.target(), Some(path.as_path()));

        let request = StartupRequest::parse(vec![
            "--restore".to_string(),
            directory
                .path()
                .join("missing.mp4")
                .to_string_lossy()
                .into_owned(),
        ])
        .expect("restore request");
        assert!(request.target().is_none());
    }

    /// 伪装归档必须被识别为可打开归档，`--open` 能接受它。
    #[test]
    fn open_request_accepts_disguised_archive() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("hello.txt");
        std::fs::write(&source, b"hello disguise").unwrap();
        let output = directory.path().join("cover.jpg");
        let options = lumark_archive::CompressionOptions {
            disguise: Some(lumark_archive::DisguiseOptions {
                mask: lumark_archive::DisguiseMask::PresetImage,
                original_name: None,
            }),
            ..Default::default()
        };
        lumark_archive::ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();
        assert_eq!(
            lumark_archive::ArchiveService::detect(&output),
            lumark_archive::ArchiveFormat::Disguised
        );
        let request = StartupRequest::parse(vec![
            "--open".to_string(),
            output.to_string_lossy().into_owned(),
        ])
        .expect("open request");
        assert_eq!(request.target(), Some(output.as_path()));
    }

    /// 验证 PE 可执行文件作为只读归档被 `--open` 和 `--extract` 接受。
    #[test]
    fn open_request_accepts_pe_archive() {
        let directory = tempfile::tempdir().unwrap();
        let pe = directory.path().join("tool.exe");
        let mut bytes = vec![0u8; 0x100];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        std::fs::write(&pe, bytes).unwrap();
        let request = StartupRequest::parse(vec![
            "--open".to_string(),
            pe.to_string_lossy().into_owned(),
        ])
        .expect("open PE request");
        assert_eq!(request.mode, StartupMode::Open);
        assert_eq!(request.target(), Some(pe.as_path()));

        let request = StartupRequest::parse(vec![
            "--extract".to_string(),
            pe.to_string_lossy().into_owned(),
        ])
        .expect("extract PE request");
        assert_eq!(request.target(), Some(pe.as_path()));
    }

    /// 验证 ISO 挂载请求只接受真实存在的 .iso 文件。
    #[test]
    fn iso_request_requires_existing_iso_file() {
        let directory = tempfile::tempdir().unwrap();
        let iso = directory.path().join("image.iso");
        std::fs::write(&iso, b"iso").unwrap();
        let request = StartupRequest::parse(vec![
            "--mount-iso".to_string(),
            iso.to_string_lossy().into_owned(),
        ])
        .expect("mount iso request");
        assert_eq!(request.target(), Some(iso.as_path()));

        let request = StartupRequest::parse(vec![
            "--mount-iso".to_string(),
            r"C:\images\missing.iso".to_string(),
        ])
        .expect("mount iso request");
        assert!(request.target().is_none());
    }

    /// 嵌套压缩包请求由进程内部构造，并携带子窗口独占的临时目录。
    #[test]
    fn nested_archive_request_keeps_owned_temp() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("inner.bin");
        std::fs::write(&archive, b"not a real archive").unwrap();
        let owned = directory.path().join("owned-temp");
        let request = StartupRequest::for_nested_archive(archive.clone(), owned.clone());
        assert_eq!(request.mode, StartupMode::Open);
        assert!(request.force_open);
        assert_eq!(request.owned_temp.as_deref(), Some(owned.as_path()));
        assert_eq!(request.target(), Some(archive.as_path()));
    }

    /// 验证没有路径时不会创建窗口，未知开关按默认浏览处理。
    #[test]
    fn startup_request_ignores_empty_and_unknown_arguments() {
        assert!(StartupRequest::parse(vec!["--open".to_string()]).is_none());
        assert!(StartupRequest::parse(vec!["--register-shell".to_string()]).is_none());
        let request = StartupRequest::parse(vec![
            "--unknown".to_string(),
            r"C:\downloads\a.zip".to_string(),
        ])
        .expect("default open request");
        assert_eq!(request.mode, StartupMode::Open);
    }
}
