//! 解压窗口 UI 测试。

use super::*;

/// 在解压窗口点击开始解压，文件应写入指定目录并关闭窗口。
#[gpui_kit::test]
fn extract_window_extracts_archive(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("inside.txt");
    std::fs::write(&source, b"inside").unwrap();
    let archive = directory.path().join("sample.zip");
    lumark_archive::ArchiveService::create(
        &archive,
        std::slice::from_ref(&source),
        &lumark_archive::CompressionOptions {
            format: lumark_archive::ArchiveFormat::Zip,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let output = directory.path().join("out");
    std::fs::create_dir_all(&output).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            directory.path().join("vault.dat"),
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            crate::app::util::set_input(
                &app.extract_output,
                output.to_str().expect("输出目录"),
                window,
                cx,
            );
            app.open_extract_window(archive.clone(), window, cx);
        });
    });
    let handle = cx.update(|_, cx| app.read(cx).extract_window.expect("应打开解压窗口"));
    cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("extract-window-output").visible());
        assert!(window.find("extract-window-start").visible());
        window.click("extract-window-start", cx);
    })
    .unwrap();
    let mut extracted = false;
    for _ in 0..200 {
        cx.run_until_parked();
        if output.join("inside.txt").is_file() {
            extracted = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(extracted, "解压窗口启动后应写出文件");
    assert_eq!(std::fs::read(output.join("inside.txt")).unwrap(), b"inside");
    app.update(cx, |app, _| {
        assert!(app.extract_window.is_none(), "解压启动后应关闭窗口")
    });
    let _ = app;
}
