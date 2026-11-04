//! 游戏编辑窗口：独立窗口创建、窗口句柄轮询和窗口尺寸。
//!
//! 编辑窗口是一个最小的 `LumarkApp` 实例：它复用共享设置、密码库和扫描动作，
//! 但只渲染设置表单。保存后窗口自行关闭，主窗口轮询句柄并在关闭后刷新游戏库。

use super::super::super::*;
use crate::i18n::tr;
use lumark_settings::GameLibraryEntry;

impl LumarkApp {
    /// 在独立窗口打开游戏编辑页；`entry` 为空表示新增。
    ///
    /// 编辑窗口复用同一套共享存储和表单动作，但只渲染设置页；保存成功后
    /// 窗口自行关闭，主窗口通过轮询句柄刷新游戏库列表。
    pub(crate) fn spawn_game_editor(
        &mut self,
        entry: Option<GameLibraryEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.game_editor_window.is_some() {
            if let Some(handle) = self.game_editor_window
                && handle
                    .update(cx, |_, window, _| {
                        crate::window_focus::activate_and_raise(window)
                    })
                    .is_ok()
            {
                self.status = tr(
                    "游戏编辑窗口已打开",
                    "The game editor window is already open",
                )
                .to_string();
                return;
            }
            self.game_editor_window = None;
        }
        let stores = self.shared_stores();
        let title = if entry.is_some() {
            tr("编辑游戏 - Lumark", "Edit game - Lumark")
        } else {
            tr("添加游戏 - Lumark", "Add game - Lumark")
        };
        let options = game_editor_window_options(window, title);
        let entry_for_window = entry.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            window.on_next_frame(|window, _| crate::window_focus::activate_and_raise(window));
            cx.new(move |cx| {
                let mut app = Self::new_shared(window, cx, stores, None, None);
                app.game_editor_only = true;
                app.start_game_setup(entry_for_window.as_ref(), window, cx);
                app
            })
        });
        match result {
            Ok((handle, _view)) => {
                self.game_editor_window = Some(handle);
                self.watch_game_editor_window(cx);
            }
            Err(error) => {
                self.status = format!(
                    "{}：{error}",
                    tr(
                        "无法打开游戏编辑窗口",
                        "Unable to open the game editor window"
                    )
                );
            }
        }
    }

    /// 轮询编辑窗口句柄；窗口关闭后清理主窗口记录并刷新游戏库列表。
    ///
    /// 编辑窗口是独立的 `LumarkApp` 实例，保存时直接写共享设置；主窗口只在
    /// 窗口关闭后重新渲染列表，因此用一个短轮询代替跨窗口消息通道。
    fn watch_game_editor_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.game_editor_window else {
            return;
        };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(350))
                    .await;
                let closed = this.update(cx, |app, cx| {
                    if handle.update(cx, |_, _, _| ()).is_err() {
                        app.game_editor_window = None;
                        cx.notify();
                        true
                    } else {
                        false
                    }
                });
                if !matches!(closed, Ok(false)) {
                    break;
                }
            }
        })
        .detach();
    }
}

/// 生成游戏编辑窗口的初始选项。
///
/// 编辑窗口位于主窗口中央、可缩放，最小尺寸保证表单字段和保存按钮不被裁切；
/// 标题和透明标题栏与其它独立窗口保持一致。
fn game_editor_window_options(parent: &Window, title: &'static str) -> gpui_kit::WindowOptions {
    let size = gpui_kit::size(px(920.), px(780.));
    let parent_bounds = parent.inner_window_bounds().get_bounds();
    let origin = gpui_kit::point(
        px((parent_bounds.origin.x.as_f32()
            + (parent_bounds.size.width.as_f32() - size.width.as_f32()) / 2.)
            .max(40.)),
        px((parent_bounds.origin.y.as_f32()
            + (parent_bounds.size.height.as_f32() - size.height.as_f32()) / 2.)
            .max(30.)),
    );
    gpui_kit::WindowOptions {
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
            origin,
            size,
        })),
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        kind: gpui_kit::WindowKind::Normal,
        focus: true,
        show: true,
        is_resizable: true,
        is_minimizable: true,
        is_movable: true,
        window_min_size: Some(gpui_kit::size(px(760.), px(600.))),
        ..gpui_kit::component::TitleBar::window_options()
    }
}
