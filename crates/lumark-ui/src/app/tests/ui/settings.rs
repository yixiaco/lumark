//! 设置窗口新增通知页的 UI 渲染测试。

use super::*;

/// 验证设置窗口可以切到“通知”页并渲染 SMTP 配置控件。
#[gpui_kit::test]
fn notification_tab_renders_smtp_fields(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let directory = tempdir().unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        LumarkApp::new_with_stores(
            window,
            cx,
            directory.path().join("vault.dat"),
            directory.path().join("settings.json"),
            Some(directory.path().to_path_buf()),
        )
    });
    let handle = cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_settings_window(window, cx));
        app.read(cx).settings_window.expect("设置窗口")
    });
    let _ = cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click(("settings-window-tab", 5usize), cx);
        window.draw(cx).clear(cx);
        assert!(window.find("settings-smtp-host").visible());
        assert!(window.find("settings-smtp-port").visible());
        assert!(window.find("settings-smtp-password").visible());
    });
    let _ = app;
}
