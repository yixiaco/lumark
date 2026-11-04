//! 资产属性 JSON 的独立只读查看器窗口。
//!
//! FModel 在主窗口以 JSON 文本页展示导出表与属性树；Lumark 沿用自身
//! 独立查看器窗口的交互，提供语法高亮、选择/复制与只读浏览。

use std::sync::Arc;

use gpui_kit::ClipboardItem;
use gpui_kit::component::TitleBar;
use gpui_kit::component::input::{Editor, EditorState};

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;

/// JSON 属性查看器窗口。
pub(crate) struct PropertiesViewerWindowView {
    /// 资产显示名。
    name: String,
    /// JSON 文本。
    json: Arc<str>,
    /// 只读代码编辑器。
    editor: gpui_kit::Entity<EditorState>,
}

impl PropertiesViewerWindowView {
    /// 创建只读 JSON 编辑器。
    pub(crate) fn new(
        name: String,
        json: Arc<str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = json.to_string();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(text)
                .language("JSON")
        });
        Self { name, json, editor }
    }

    /// 渲染标题栏。
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .rounded(cx.theme().radius)
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
                            .text_ellipsis()
                            .text_sm()
                            .font_semibold()
                            .child(format!(
                                "{} · {}",
                                self.name,
                                tr("属性 JSON", "Properties JSON")
                            )),
                    )
                    .child(
                        button("properties-copy")
                            .primary()
                            .icon(AppIcon::Copy.element(13.))
                            .label(tr("复制", "Copy"))
                            .tooltip(tr("复制全部 JSON", "Copy all JSON"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.json.to_string(),
                                ));
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 渲染只读正文。
    fn render_body(&self) -> impl IntoElement {
        div()
            .id("properties-viewer-body")
            .debug_selector(|| "properties-viewer-body".into())
            .flex_1()
            .min_h_0()
            .w_full()
            .p_2()
            .child(
                Editor::new(&self.editor)
                    .readonly(true)
                    .appearance(true)
                    .bordered(false)
                    .size_full(),
            )
    }

    /// 状态栏：字节数、行数与只读标记。
    fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let lines = self.json.lines().count();
        h_flex()
            .h(px(26.))
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(format!(
                "{} · {} {} · {} {}",
                tr("只读 JSON", "Read-only JSON"),
                lines,
                tr("行", "lines"),
                self.json.len(),
                tr("字节", "bytes")
            ))
            .child(
                div()
                    .ml_auto()
                    .child(tr("Ctrl+C 复制选中内容", "Ctrl+C copies the selection")),
            )
    }
}

impl Render for PropertiesViewerWindowView {
    /// 组合标题栏、JSON 编辑器与状态栏。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(self.render_body())
            .child(self.render_status_bar(cx))
    }
}

/// 打开只读 JSON 属性窗口。
pub(crate) fn open_properties_window(
    name: String,
    json: Arc<str>,
    parent: &Window,
    cx: &mut gpui_kit::App,
) -> Result<(), String> {
    let options = properties_window_options(parent);
    let display = name.clone();
    gpui_kit::open_window(options, cx, move |window, cx| {
        window.on_next_frame(|window, _| crate::window_focus::activate_and_raise(window));
        cx.new(|cx| PropertiesViewerWindowView::new(display.clone(), json.clone(), window, cx))
    })
    .map(|_| ())
    .map_err(|error| error.to_string())
}

impl LumarkApp {
    /// 从详情面板/查看器打开当前资产的 JSON 属性窗口。
    pub(crate) fn open_properties_viewer(
        &mut self,
        name: String,
        json: Arc<str>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        match open_properties_window(name.clone(), json, parent, cx) {
            Ok(()) => {
                self.status = format!(
                    "{} {name}",
                    tr("已打开属性 JSON：", "Opened properties JSON:")
                )
            }
            Err(error) => {
                self.status = format!(
                    "{}{error}",
                    tr("无法打开属性 JSON：", "Cannot open properties JSON: ")
                )
            }
        }
        cx.notify();
    }
}

/// 与父窗口居中的 JSON 查看器窗口选项。
fn properties_window_options(parent: &Window) -> gpui_kit::WindowOptions {
    let window_size = gpui_kit::size(px(980.), px(720.));
    let parent_bounds = parent.inner_window_bounds().get_bounds();
    let origin = gpui_kit::point(
        px(parent_bounds.origin.x.as_f32()
            + (parent_bounds.size.width.as_f32() - window_size.width.as_f32()) / 2.0),
        px(parent_bounds.origin.y.as_f32()
            + (parent_bounds.size.height.as_f32() - window_size.height.as_f32()) / 2.0),
    );
    gpui_kit::WindowOptions {
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
            origin,
            size: window_size,
        })),
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: Some("属性 JSON - Lumark".into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        kind: gpui_kit::WindowKind::Normal,
        focus: true,
        show: true,
        is_resizable: true,
        is_minimizable: true,
        is_movable: true,
        window_min_size: Some(gpui_kit::size(px(480.), px(360.))),
        ..TitleBar::window_options()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::test::TestWindowExt;

    /// 只读窗口应保留 JSON 文本并统计行数。
    #[test]
    fn json_viewer_keeps_text() {
        let json: Arc<str> = Arc::from("{\n  \"Name\": \"NanaF6_2\"\n}");
        assert_eq!(json.lines().count(), 3);
        assert!(json.contains("NanaF6_2"));
    }

    /// 窗口实际渲染只读编辑器正文与复制按钮。
    #[gpui_kit::test]
    fn json_viewer_renders_readonly_editor(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let json: Arc<str> =
            Arc::from("{\n  \"Name\": \"NanaF6_2\",\n  \"Class\": \"SkeletalMesh\"\n}");
        let (_view, cx) = cx.add_window_view(|window, cx| {
            PropertiesViewerWindowView::new("NanaF6_2".to_string(), json, window, cx)
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(window.find("properties-copy").visible());
        });
    }
}
