//! 自解压界面的无头窗口测试。
//!
//! 测试直接使用伪造的 stub 字节 + 真实 ZIP 载荷 + v2 页脚组成 bundle，
//! 不需要启动真实 exe，即可覆盖按钮可见性、表单校验、解压和密码错误路径。

#![cfg(windows)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use lumark_sfx::{SfxFooter, SfxPlan, read_footer};
use lumark_ui_shared::progress::ProgressState;
use zeroize::Zeroizing;

use super::view::{SfxView, Stage};

/// 组装 v2 页脚。
fn footer_bytes(offset: u64, length: u64, encrypted: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LMSFX002");
    bytes.push(0);
    bytes.push(u8::from(encrypted));
    bytes.extend_from_slice(&[0u8; 6]);
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes
}

/// 生成含单个文本条目的 ZIP 数据，可选用 AES 密码。
fn zip_payload(path: &Path, name: &str, contents: &[u8], password: Option<&str>) -> Vec<u8> {
    let file = File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = match password {
        Some(password) => zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, password),
        None => zip::write::SimpleFileOptions::default(),
    };
    writer.start_file(name, options).unwrap();
    writer.write_all(contents).unwrap();
    writer.finish().unwrap();
    fs::read(path).unwrap()
}

/// 用伪造 stub、ZIP 载荷和页脚组成可被运行时解析的 bundle。
fn bundle(directory: &Path, payload: &[u8], encrypted: bool) -> (PathBuf, SfxFooter) {
    let path = directory.join("bundle.exe");
    let stub = b"MZfake-sfx-stub-for-tests";
    let mut file = File::create(&path).unwrap();
    file.write_all(stub).unwrap();
    file.write_all(payload).unwrap();
    file.write_all(&footer_bytes(
        stub.len() as u64,
        payload.len() as u64,
        encrypted,
    ))
    .unwrap();
    drop(file);
    let mut file = File::open(&path).unwrap();
    let footer = read_footer(&mut file).unwrap().unwrap();
    (path, footer)
}

/// 创建一个使用伪造 bundle 的自解压视图，并返回测试窗口上下文。
fn view_for<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    directory: &Path,
    payload: &[u8],
    encrypted: bool,
    password: Option<&str>,
) -> (
    gpui_kit::Entity<SfxView>,
    &'a mut gpui_kit::VisualTestContext,
    PathBuf,
) {
    let (exe, footer) = bundle(directory, payload, encrypted);
    let target = directory.join("out");
    let password = password.map(|value| Zeroizing::new(value.to_string()));
    let (view, cx) = cx.add_window_view(move |window, cx| {
        SfxView::new(
            exe,
            footer,
            SfxPlan::default(),
            target,
            password,
            window,
            cx,
        )
    });
    (view, cx, directory.join("out"))
}

/// 直接驱动一次事件消费，避免测试依赖后台计时器。
fn pump(cx: &mut gpui_kit::VisualTestContext, view: &gpui_kit::Entity<SfxView>) -> bool {
    cx.update(|window, cx| view.update(cx, |view, cx| view.drain_events(window, cx)))
}

/// 等待视图进入指定阶段。
fn wait_for_stage(
    cx: &mut gpui_kit::VisualTestContext,
    view: &gpui_kit::Entity<SfxView>,
    stage: Stage,
) -> bool {
    for _ in 0..200 {
        cx.run_until_parked();
        let _ = pump(cx, view);
        if view.update(cx, |view, _| view.stage == stage) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// 设置阶段必须渲染目录、浏览、解压和取消四个操作的可见按钮。
#[gpui_kit::test]
fn setup_renders_visible_actions(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    lumark_ui_shared::i18n::set_english(false);
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(
        &directory.path().join("payload.zip"),
        "hello.txt",
        b"hello",
        None,
    );
    let (view, cx, _) = view_for(cx, directory.path(), &payload, false, None);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("sfx-target").visible());
        assert!(window.find("sfx-browse").visible());
        assert!(window.find("sfx-extract").visible());
        assert!(window.find("sfx-cancel").visible());
    });
    let _ = view;
}

/// 点击解压后应写出文件并进入完成阶段。
#[gpui_kit::test]
fn extracts_through_the_window(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    lumark_ui_shared::i18n::set_english(false);
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(
        &directory.path().join("payload.zip"),
        "hello.txt",
        b"through ui",
        None,
    );
    let (view, cx, target) = view_for(cx, directory.path(), &payload, false, None);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.click("sfx-extract", cx);
    });
    assert!(
        wait_for_stage(cx, &view, Stage::Completed),
        "点击解压后应进入完成阶段"
    );
    assert_eq!(fs::read(target.join("hello.txt")).unwrap(), b"through ui");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("sfx-close").visible());
        assert!(window.find("sfx-open-folder").visible());
    });
}

/// 加密包未填写密码时应在表单内提示，不启动解压。
#[gpui_kit::test]
fn empty_password_is_rejected_before_extracting(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    lumark_ui_shared::i18n::set_english(false);
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(
        &directory.path().join("payload.zip"),
        "hello.txt",
        b"secret",
        Some("right"),
    );
    let (view, cx, _) = view_for(cx, directory.path(), &payload, true, None);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.click("sfx-extract", cx);
    });
    view.update(cx, |view, _| {
        assert_eq!(view.stage, Stage::Setup);
        assert!(view.error.is_some());
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.find("sfx-password").visible());
    });
}

/// 密码错误后应回到设置阶段、清空密码并显示错误。
#[gpui_kit::test]
fn wrong_password_returns_to_setup(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    lumark_ui_shared::i18n::set_english(false);
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(
        &directory.path().join("payload.zip"),
        "hello.txt",
        b"secret",
        Some("right"),
    );
    let (view, cx, _) = view_for(cx, directory.path(), &payload, true, Some("wrong"));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.click("sfx-extract", cx);
    });
    let mut returned = false;
    for _ in 0..200 {
        cx.run_until_parked();
        let _ = pump(cx, &view);
        if view.update(cx, |view, _| {
            view.stage == Stage::Setup && view.error.is_some()
        }) {
            returned = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(returned, "密码错误后应回到设置阶段并显示错误");
    view.update(cx, |view, cx| {
        assert!(view.password_value(cx).is_empty(), "密码错误后应清空密码");
    });
}

/// 解压阶段必须渲染取消按钮和进度面板。
#[gpui_kit::test]
fn progress_stage_renders_cancel_action(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    lumark_ui_shared::i18n::set_english(false);
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(
        &directory.path().join("payload.zip"),
        "hello.txt",
        b"hello",
        None,
    );
    let (view, cx, _) = view_for(cx, directory.path(), &payload, false, None);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.stage = Stage::Extracting;
            view.progress_state = ProgressState::Running;
            view.progress.bytes_done = 50;
            view.progress.bytes_total = 100;
            view.progress.current = "hello.txt".to_string();
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert!(window.find("sfx-cancel-run").visible());
    });
}
