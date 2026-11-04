//! 媒体播放窗口（M-4/M-5）：ffmpeg 探测 + 按时间抽帧预览、播放/暂停与跳转。

use super::super::*;

use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::RenderImage;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, TitleBar};
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div, px};
use lumark_media::{
    FfmpegTools, MediaFrame, MediaInfo, extract_frame_with, probe_with, spawn_audio,
};

use crate::app::LumarkApp;
use crate::i18n::tr;

/// 媒体播放窗口状态。
pub(crate) struct MediaPlayerWindowView {
    path: PathBuf,
    name: String,
    tools: Option<FfmpegTools>,
    info: Option<MediaInfo>,
    image: Option<Arc<RenderImage>>,
    position: f64,
    playing: bool,
    loading: bool,
    error: Option<String>,
    audio: Option<Child>,
    audio_error: Option<String>,
    generation: Arc<AtomicU64>,
}

impl Drop for MediaPlayerWindowView {
    /// 关闭窗口时停止音频子进程。
    fn drop(&mut self) {
        self.stop_audio();
    }
}

impl MediaPlayerWindowView {
    /// 创建窗口并异步探测/加载首帧。
    pub(crate) fn new(path: PathBuf, name: String, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            path,
            name,
            tools: None,
            info: None,
            image: None,
            position: 0.0,
            playing: false,
            loading: true,
            error: None,
            audio: None,
            audio_error: None,
            generation: Arc::new(AtomicU64::new(0)),
        };
        view.initialize(cx);
        view
    }

    /// 探测媒体并加载首帧。
    fn initialize(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let tools = FfmpegTools::locate()?;
                    let info = probe_with(&tools, &path)?;
                    let frame = extract_frame_with(&tools, &path, 0.0, Some(1280))?;
                    Ok::<_, String>((tools, info, frame))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((tools, info, frame)) => {
                        view.tools = Some(tools);
                        view.info = Some(info);
                        view.image = Some(frame_to_image(frame));
                        view.loading = false;
                    }
                    Err(error) => {
                        view.loading = false;
                        view.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 请求加载当前位置的帧；旧请求会被 generation 丢弃。
    fn load_frame(&mut self, cx: &mut Context<Self>) {
        let Some(tools) = self.tools.clone() else {
            return;
        };
        let path = self.path.clone();
        let position = self.position.max(0.0);
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let counter = self.generation.clone();
        self.loading = true;
        cx.spawn(async move |this, cx| {
            let frame = cx
                .background_executor()
                .spawn(async move { extract_frame_with(&tools, &path, position, Some(1280)) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if counter.load(Ordering::Relaxed) != generation {
                    return;
                }
                view.loading = false;
                match frame {
                    Ok(frame) => {
                        view.image = Some(frame_to_image(frame));
                        view.error = None;
                    }
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 播放/暂停。
    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.info.is_none() {
            return;
        }
        self.playing = !self.playing;
        if self.playing {
            self.start_audio();
            self.spawn_playback(cx);
        } else {
            self.stop_audio();
        }
        cx.notify();
    }

    /// 从当前位置启动 ffplay 音频；无音轨/无 ffplay 时静默跳过。
    fn start_audio(&mut self) {
        self.stop_audio();
        let Some(info) = self.info.as_ref() else {
            return;
        };
        if !info.has_audio() {
            return;
        }
        let Some(tools) = self.tools.as_ref() else {
            return;
        };
        if !tools.has_audio_backend() {
            self.audio_error =
                Some(tr("未找到 ffplay，仅播放画面", "ffplay not found; video only").to_string());
            return;
        }
        match spawn_audio(tools, &self.path, self.position) {
            Ok(child) => {
                self.audio = Some(child);
                self.audio_error = None;
            }
            Err(error) => self.audio_error = Some(error),
        }
    }

    /// 停止音频子进程。
    fn stop_audio(&mut self) {
        if let Some(mut child) = self.audio.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// 播放循环：按帧率推进时间并请求抽帧。
    fn spawn_playback(&mut self, cx: &mut Context<Self>) {
        let fps = self
            .info
            .as_ref()
            .map(|info| info.fps)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(30.0);
        let interval = std::time::Duration::from_secs_f64((1.0 / fps).clamp(0.033, 1.0));
        cx.spawn(async move |this, cx| {
            loop {
                let state = this.update(cx, |view, cx| {
                    if !view.playing {
                        return None;
                    }
                    let duration = view
                        .info
                        .as_ref()
                        .map(|info| info.duration_seconds)
                        .unwrap_or(0.0);
                    view.position += 1.0 / fps;
                    if duration > 0.0 && view.position >= duration {
                        view.position = duration;
                        view.playing = false;
                        view.stop_audio();
                        cx.notify();
                        return None;
                    }
                    view.load_frame(cx);
                    cx.notify();
                    Some(())
                });
                if !matches!(state, Ok(Some(()))) {
                    break;
                }
                cx.background_executor()
                    .spawn(async move { std::thread::sleep(interval) })
                    .await;
            }
        })
        .detach();
    }

    /// 跳转到指定时间。
    fn seek(&mut self, target: f64, cx: &mut Context<Self>) {
        let duration = self
            .info
            .as_ref()
            .map(|info| info.duration_seconds)
            .unwrap_or(0.0);
        self.position = clamp_position(target, duration);
        self.load_frame(cx);
        if self.playing {
            self.start_audio();
        }
        cx.notify();
    }

    /// 逐帧步进。
    fn step_frame(&mut self, direction: i32, cx: &mut Context<Self>) {
        let fps = self
            .info
            .as_ref()
            .map(|info| info.fps)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(30.0);
        let target = self.position + f64::from(direction) / fps;
        self.seek(target, cx);
    }
}

impl Render for MediaPlayerWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = self.info.clone();
        let duration = info
            .as_ref()
            .map(|info| info.duration_seconds)
            .unwrap_or(0.0);
        let time = format!("{} / {}", format_time(self.position), format_time(duration));
        let canvas = if let Some(image) = &self.image {
            gpui_kit::img(image.clone())
                .size_full()
                .object_fit(gpui_kit::ObjectFit::Contain)
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if let Some(error) = &self.error {
                    error.clone()
                } else if self.loading {
                    tr("正在加载媒体…", "Loading media…").to_string()
                } else {
                    tr("无画面", "No frame").to_string()
                })
                .into_any_element()
        };
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
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .child(div().text_sm().font_semibold().child(format!(
                                "{} - {}",
                                self.name,
                                tr("媒体播放器", "Media player")
                            )))
                            .child(
                                div()
                                    .ml_auto()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(time.clone()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("media-play")
                            .ghost()
                            .label(if self.playing {
                                tr("暂停", "Pause")
                            } else {
                                tr("播放", "Play")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
                    )
                    .child(
                        Button::new("media-back-5")
                            .ghost()
                            .label("-5s")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.seek(this.position - 5.0, cx);
                            })),
                    )
                    .child(
                        Button::new("media-forward-5")
                            .ghost()
                            .label("+5s")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.seek(this.position + 5.0, cx);
                            })),
                    )
                    .child(
                        Button::new("media-prev-frame")
                            .ghost()
                            .label("-1")
                            .on_click(cx.listener(|this, _, _, cx| this.step_frame(-1, cx))),
                    )
                    .child(
                        Button::new("media-next-frame")
                            .ghost()
                            .label("+1")
                            .on_click(cx.listener(|this, _, _, cx| this.step_frame(1, cx))),
                    )
                    .child(
                        Button::new("media-restart")
                            .ghost()
                            .label(tr("回到开头", "Restart"))
                            .on_click(cx.listener(|this, _, _, cx| this.seek(0.0, cx))),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(time),
                    ),
            )
            .child(
                div()
                    .id("media-canvas")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .bg(cx.theme().background)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(canvas),
            )
            .child(
                h_flex()
                    .h(px(26.))
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        info.map(|info| info.summary())
                            .unwrap_or_else(|| tr("未探测", "Not probed").to_string()),
                    )
                    .when(self.loading, |this| {
                        this.child(tr("抽帧中…", "Extracting frame…").to_string())
                    })
                    .when(self.audio.is_some(), |this| {
                        this.child(tr("音频播放中", "Audio playing").to_string())
                    })
                    .when_some(self.audio_error.clone(), |this, error| this.child(error)),
            )
    }
}

/// 帧转 RenderImage（BGRA）。
fn frame_to_image(frame: MediaFrame) -> Arc<RenderImage> {
    let mut bgra = frame.rgba;
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let image = image::RgbaImage::from_raw(frame.width, frame.height, bgra)
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
}

/// 时间格式（mm:ss.mmm）。
fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let remaining = seconds - minutes as f64 * 60.0;
    format!("{minutes:02}:{remaining:06.3}")
}

/// 时间点限制在 [0, duration]。
fn clamp_position(position: f64, duration: f64) -> f64 {
    if duration > 0.0 {
        position.clamp(0.0, duration)
    } else {
        position.max(0.0)
    }
}

impl LumarkApp {
    /// 打开媒体播放窗口；ffmpeg 不可用时回退到外部打开。
    pub(crate) fn open_media_player(
        &mut self,
        path: PathBuf,
        name: String,
        parent: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if FfmpegTools::locate().is_err() {
            self.status = tr(
                "未找到 ffmpeg/ffprobe，使用系统默认播放器",
                "ffmpeg/ffprobe not found; using system player",
            )
            .to_string();
            let _ = lumark_platform_windows::open_default(&path);
            return;
        }
        let size = gpui_kit::size(px(900.), px(680.));
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
                title: Some(tr("媒体播放器 - Lumark", "Media player - Lumark").into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(560.), px(360.))),
            ..TitleBar::window_options()
        };
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            window.on_next_frame(|window, _| crate::window_focus::activate_and_raise(window));
            cx.new(|cx| MediaPlayerWindowView::new(path.clone(), name.clone(), cx))
        });
        match result {
            Ok(_) => self.status = tr("已打开媒体播放器", "Media player opened").to_string(),
            Err(error) => self.status = format!("无法打开媒体播放器：{error}"),
        }
    }
}

/// 是否为支持的媒体扩展名。
pub(crate) fn is_media_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| {
            matches!(
                extension.as_str(),
                "mp4" | "m4v" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "flv" | "ts" | "m2ts"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::test::TestWindowExt;

    /// 时间格式与位置限制。
    #[test]
    fn formats_time_and_clamps_position() {
        assert_eq!(format_time(0.0), "00:00.000");
        assert_eq!(format_time(65.25), "01:05.250");
        assert_eq!(format_time(-1.0), "00:00.000");
        assert_eq!(clamp_position(-3.0, 10.0), 0.0);
        assert_eq!(clamp_position(12.0, 10.0), 10.0);
        assert_eq!(clamp_position(5.0, 0.0), 5.0);
    }

    /// 扩展名识别。
    #[test]
    fn recognizes_media_extensions() {
        assert!(is_media_path(std::path::Path::new("movie.MP4")));
        assert!(is_media_path(std::path::Path::new("clip.webm")));
        assert!(!is_media_path(std::path::Path::new("photo.png")));
        assert!(!is_media_path(std::path::Path::new("archive.zip")));
    }

    /// 播放窗口渲染控件（探测在后台，不等待结果）。
    #[gpui_kit::test]
    fn media_player_renders_controls(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let path = std::path::PathBuf::from("missing.mp4");
        let (_view, cx) =
            cx.add_window_view(|_, cx| MediaPlayerWindowView::new(path, "missing".to_string(), cx));
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(window.find("media-play").visible());
            assert!(window.find("media-back-5").visible());
            assert!(window.find("media-forward-5").visible());
            assert!(window.find("media-prev-frame").visible());
            assert!(window.find("media-next-frame").visible());
        });
    }
}
