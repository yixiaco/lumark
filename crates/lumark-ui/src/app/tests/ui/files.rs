//! 文件动作与拖入归档场景测试。

use super::*;

/// 验证拖入归档的文件会进入添加确认弹窗，而不是直接覆盖。
#[gpui_kit::test]
fn dropped_files_open_archive_add_confirmation(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let source = directory.path().join("inside.txt");
    std::fs::write(&source, b"inside").unwrap();
    let archive = directory.path().join("target.zip");
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
    let dropped = directory.path().join("dropped.txt");
    std::fs::write(&dropped, b"dropped").unwrap();
    let mut app_slot = None;
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let app = cx.new(|cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        app_slot = Some(app.clone());
        gpui_kit::base::Root::new(app, window, cx)
    });
    let app = app_slot.expect("测试窗口应创建应用视图");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.browser.open_archive(&archive, None).unwrap();
            app.handle_external_drop(std::slice::from_ref(&dropped), window, cx);
            assert_eq!(
                app.pending_archive_additions,
                vec![("dropped.txt".to_string(), dropped)]
            );
        });
        window.draw(cx).clear(cx);
        assert!(window.find("archive-add-confirm").visible());
    });
    let _ = app;
}
