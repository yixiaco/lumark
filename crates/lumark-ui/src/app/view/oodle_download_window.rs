//! Oodle 下载/安装窗口（O-5）：URL/SHA/缓存目录、进度、下载后配置后端。

use super::super::*;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::Entity;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, TitleBar};
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div, px};
use lumark_archive::{
    OodleDownloadOptions, OodleDownloadResult, configure_oodle_library, download_and_cache_oodle,
};

use crate::app::LumarkApp;
use crate::i18n::tr;

/// 下载进度快照。
type Progress = Arc<Mutex<(u64, Option<u64>)>>;

/// Oodle 下载窗口状态。
pub(crate) struct OodleDownloadWindowView {
    url: Entity<InputState>,
    sha256: Entity<InputState>,
    cache_dir: PathBuf,
    running: bool,
    status: String,
    progress: Progress,
    result: Option<OodleDownloadResult>,
    error: Option<String>,
}

impl OodleDownloadWindowView {
    /// 创建下载窗口。
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://example.com/oodle.zip".to_string())
        });
        let sha256 = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("可选：zip SHA-256", "Optional: zip SHA-256").to_string())
        });
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Lumark")
            .join("oodle");
        Self {
            url,
            sha256,
            cache_dir,
            running: false,
            status: tr(
                "填写已获授权的 zip 地址后下载",
                "Enter an authorized zip URL to download",
            )
            .to_string(),
            progress: Arc::new(Mutex::new((0, None))),
            result: None,
            error: None,
        }
    }

    /// 开始下载。
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let url = self.url.read(cx).value().to_string();
        if !is_http_url(&url) {
            self.error = Some(tr("请输入 http(s) 地址", "Enter an http(s) URL").to_string());
            cx.notify();
            return;
        }
        let sha = self.sha256.read(cx).value().trim().to_string();
        let sha = (!sha.is_empty()).then_some(sha);
        self.running = true;
        self.error = None;
        self.result = None;
        self.status = tr("正在连接…", "Connecting…").to_string();
        self.progress = Arc::new(Mutex::new((0, None)));
        let progress = self.progress.clone();
        let options = OodleDownloadOptions {
            url,
            cache_dir: self.cache_dir.clone(),
            expected_sha256: sha,
            preferred_library: Some("oo2core_9_win64.dll".to_string()),
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    download_and_cache_oodle(&options, |done, total| {
                        if let Ok(mut progress) = progress.lock() {
                            *progress = (done, total);
                        }
                    })
                    .and_then(|result| {
                        configure_oodle_library(Some(result.library.clone()))
                            .map(|()| result)
                            .map_err(|error| format!("下载完成但配置后端失败：{error}"))
                    })
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.running = false;
                match result {
                    Ok(result) => {
                        view.status = format!(
                            "{}：{}",
                            tr("已下载并配置", "Downloaded and configured"),
                            result.library.display()
                        );
                        view.result = Some(result);
                    }
                    Err(error) => {
                        view.status = tr("下载失败", "Download failed").to_string();
                        view.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .spawn(async move { std::thread::sleep(std::time::Duration::from_millis(120)) })
                    .await;
                let running = this.update(cx, |view, cx| {
                    if !view.running {
                        return false;
                    }
                    let (done, total) = view
                        .progress
                        .lock()
                        .map(|progress| *progress)
                        .unwrap_or((0, None));
                    view.status = match total {
                        Some(total) if total > 0 => format!(
                            "{} {:.1}% · {} / {}",
                            tr("下载中", "Downloading"),
                            done as f64 / total as f64 * 100.0,
                            format_bytes(done),
                            format_bytes(total)
                        ),
                        _ => format!("{} {}", tr("下载中", "Downloading"), format_bytes(done)),
                    };
                    cx.notify();
                    true
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
                // 下载结束后主任务会更新 running=false，这里再等一拍退出。
                cx.background_executor()
                    .spawn(async move { std::thread::sleep(std::time::Duration::from_millis(120)) })
                    .await;
            }
        })
        .detach();
    }
}

impl Render for OodleDownloadWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cache_dir = self.cache_dir.display().to_string();
        let mut content = v_flex()
            .p_4()
            .gap_3()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(tr("下载 Oodle 解压库", "Download Oodle runtime")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(
                                "仅使用你有权使用的 Oodle zip；下载后会自动校验并配置后端。",
                                "Use only an Oodle zip you are authorized to use; it will be verified and configured.",
                            )),
                    ),
            )
            .child(field(
                tr("zip 地址", "Zip URL"),
                Input::new(&self.url).id("oodle-download-url").w_full(),
            ))
            .child(field(
                tr("SHA-256（可选）", "SHA-256 (optional)"),
                Input::new(&self.sha256)
                    .id("oodle-download-sha")
                    .w_full(),
            ))
            .child(field(
                tr("缓存目录", "Cache directory"),
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(cache_dir),
            ))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(div().text_xs().text_color(cx.theme().danger).child(error))
                    }),
            );
        if let Some(result) = &self.result {
            content = content.child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "{} {}",
                        tr("缓存 zip：", "Cached zip:"),
                        result.archive.display()
                    )))
                    .child(div().text_xs().child(format!(
                        "{} {}",
                        if result.cache_hit {
                            tr("缓存命中；库：", "Cache hit; library:")
                        } else {
                            tr("已解压；库：", "Extracted; library:")
                        },
                        result.library.display()
                    ))),
            );
        }
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    .h(px(38.))
                    .bg(cx.theme().title_bar)
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .child(
                        h_flex().w_full().items_center().gap_2().px_3().child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(tr("下载 Oodle - Lumark", "Download Oodle - Lumark")),
                        ),
                    ),
            )
            .child(content)
            .child(
                h_flex()
                    .w_full()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("oodle-download-start")
                            .primary()
                            .label(tr("开始下载", "Start download"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start(cx);
                            })),
                    )
                    .when(self.running, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("下载中…", "Downloading…")),
                        )
                    }),
            )
    }
}

/// 表单字段。
fn field(label: &str, input: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_xs().font_semibold().child(label.to_string()))
        .child(input)
}

/// 仅接受 http/https URL。
fn is_http_url(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("https://") || value.starts_with("http://")
}

/// 字节格式化。
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

impl LumarkApp {
    /// 打开 Oodle 下载窗口。
    pub(crate) fn open_oodle_download(&mut self, parent: &mut Window, cx: &mut Context<Self>) {
        let size = gpui_kit::size(px(620.), px(480.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - size.height.as_f32()) / 2.0),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some(tr("下载 Oodle - Lumark", "Download Oodle - Lumark").into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(520.), px(380.))),
            ..TitleBar::window_options()
        };
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| OodleDownloadWindowView::new(window, cx))
        });
        self.status = match result {
            Ok(_) => tr("已打开 Oodle 下载窗口", "Oodle download window opened").to_string(),
            Err(error) => format!("无法打开 Oodle 下载窗口：{error}"),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::test::TestWindowExt;

    /// URL 校验与字节格式化。
    #[test]
    fn validates_urls_and_formats_bytes() {
        assert!(is_http_url("https://example.com/oodle.zip"));
        assert!(is_http_url(" http://localhost/oodle.zip "));
        assert!(!is_http_url("file:///tmp/oodle.zip"));
        assert!(!is_http_url(""));
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(0), "0.0 B");
    }

    /// 下载窗口渲染 URL/SHA/启动控件。
    #[gpui_kit::test]
    fn oodle_download_window_renders(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let (_view, cx) = cx.add_window_view(OodleDownloadWindowView::new);
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(window.find("oodle-download-start").visible());
            assert!(window.find("oodle-download-url").visible());
            assert!(window.find("oodle-download-sha").visible());
        });
    }
}
