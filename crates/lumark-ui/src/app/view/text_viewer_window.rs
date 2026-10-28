//! 归档内文本查看窗口。

use super::super::*;
use super::components::AppIcon;
use gpui_kit::component::input::{Editor, EditorState};

/// 内置只读文本查看器窗口。
///
/// 文本在打开窗口前一次性读取，窗口只持有解码后的字符串和编辑器状态，
/// 不再持有文件句柄；内容过大或包含二进制数据时由打开动作提前拒绝。
pub(crate) struct TextViewerWindowView {
    name: String,
    path: String,
    size: u64,
    state: gpui_kit::Entity<EditorState>,
    line_count: usize,
}

impl TextViewerWindowView {
    /// 创建只读文本视图，内容已按 UTF-8 有损解码。
    pub(crate) fn new(
        content: String,
        name: String,
        path: String,
        size: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let line_count = content.lines().count().max(1);
        let state = cx.new(|cx| EditorState::new(window, cx).default_value(content));
        Self {
            name,
            path,
            size,
            state,
            line_count,
        }
    }

    /// 渲染查看器标题栏：图标、文件名和只读标记。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(38.))
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(px(7.))
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Doc.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .text_sm()
                            .font_semibold()
                            .child(format!("{} - 文本查看器 - Lumark", self.name)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .rounded(cx.theme().radius / 2.)
                            .bg(cx.theme().muted)
                            .px_2()
                            .py(px(1.))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("只读"),
                    ),
            )
    }
}

impl Render for TextViewerWindowView {
    /// 组合标题栏、只读编辑器和底部状态行。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                div().flex_1().min_h_0().w_full().p_2().child(
                    Editor::new(&self.state)
                        .readonly(true)
                        .bordered(false)
                        .h(gpui_kit::relative(1.)),
                ),
            )
            .child(
                h_flex()
                    .h(px(26.))
                    .w_full()
                    .flex_shrink_0()
                    .px_3()
                    .gap_3()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .child(self.path.clone()),
                    )
                    .child(format!("{} 行", self.line_count))
                    .child(format_size(self.size)),
            )
    }
}

impl LumarkApp {
    /// 在内置只读文本查看器中打开文件；超限或二进制内容给出状态提示。
    pub(crate) fn open_text_viewer(
        &mut self,
        target: PathBuf,
        display: String,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        /// 文本预览的最大读取字节数。
        const LIMIT: u64 = 4 * 1024 * 1024;
        let size = std::fs::metadata(&target)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if size > LIMIT {
            self.status = format!("文件超过 4 MB，内置查看器只支持较小的文本文件：{display}");
            return;
        }
        let bytes = match std::fs::read(&target) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.status = format!("无法读取文件：{error}");
                return;
            }
        };
        if bytes.iter().take(8192).any(|byte| *byte == 0) {
            self.status = format!("{display} 不是文本文件，内置查看器无法显示");
            return;
        }
        let content = String::from_utf8_lossy(&bytes).to_string();
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| display.clone());
        let path = target.display().to_string();
        let window_size = gpui_kit::size(px(760.), px(560.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - window_size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - window_size.height.as_f32()) / 2.0),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size: window_size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("文本查看器 - Lumark".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(420.), px(300.))),
            ..TitleBar::window_options()
        };
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                TextViewerWindowView::new(
                    content.clone(),
                    name.clone(),
                    path.clone(),
                    size,
                    window,
                    cx,
                )
            })
        });
        match result {
            Ok(_) => self.status = format!("已在内置查看器中打开 {display}"),
            Err(error) => self.status = format!("无法打开文本查看器：{error}"),
        }
    }
}
