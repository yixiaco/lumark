use super::*;

/// 动画时间轴显示状态。
///
/// 把进度、帧号和帧步进计算集中到纯函数，工具栏渲染与逐帧按钮共用，
/// 也便于在无窗口环境下测试。
#[derive(Debug, Clone, Copy)]
pub(super) struct AnimationTimeline {
    pub(super) length: f32,
    pub(super) total_frames: u32,
    pub(super) time: f32,
}

impl AnimationTimeline {
    /// 创建时间轴状态；时长为 0 时进度固定为 0。
    pub(super) fn new(length: f32, total_frames: u32, time: f32) -> Self {
        let total_frames = total_frames.max(1);
        let time = if length > 0.0 {
            time.clamp(0.0, length)
        } else {
            0.0
        };
        Self {
            length,
            total_frames,
            time,
        }
    }

    /// 当前播放进度（0–1）。
    pub(super) fn ratio(&self) -> f32 {
        if self.length > 0.0 {
            (self.time / self.length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// 当前帧号（1 起）。
    pub(super) fn frame(&self) -> u32 {
        let last_frame = self.total_frames.saturating_sub(1) as f32;
        let frame = (self.ratio() * last_frame).round() as u32 + 1;
        frame.min(self.total_frames)
    }

    /// 帧号/时间标签。
    pub(super) fn label(&self) -> String {
        format!(
            "{}/{} · {:.2}s / {:.2}s",
            self.frame(),
            self.total_frames,
            self.time,
            self.length
        )
    }

    /// 按帧步进后的时间（按总帧数换算帧间隔，并夹在时长范围内）。
    pub(super) fn stepped(&self, frames: i32) -> f32 {
        if self.length <= 0.0 {
            return 0.0;
        }
        let frame_time = self.length / self.total_frames.saturating_sub(1).max(1) as f32;
        (self.time + frames as f32 * frame_time).clamp(0.0, self.length)
    }
}

impl AssetViewerWindowView {
    pub(super) fn step_animation(&mut self, frames: i32, cx: &mut Context<Self>) {
        let Some((length, total)) = self.animation_info() else {
            return;
        };
        if length <= 0.0 {
            return;
        }
        let timeline = AnimationTimeline::new(length, total, self.animation_time);
        self.animation_playing = false;
        self.animation_time = timeline.stepped(frames);
        self.render_preview();
        cx.notify();
    }

    /// 回到动画开头
    pub(super) fn reset_animation(&mut self, cx: &mut Context<Self>) {
        self.animation_playing = false;
        self.animation_time = 0.0;
        self.render_preview();
        cx.notify();
    }

    /// 播放/暂停动画
    pub(super) fn toggle_animation(&mut self, cx: &mut Context<Self>) {
        if !self.has_animation() {
            return;
        }
        self.animation_playing = !self.animation_playing;
        if !self.animation_playing {
            self.animation_time = 0.0;
        }
        if self.animation_playing {
            self.spawn_animation_loop(cx);
        }
        self.render_preview();
        cx.notify();
    }

    /// 以约 20fps 推进动画时间并重绘
    pub(super) fn spawn_animation_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .spawn(async {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    })
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if !this.animation_playing {
                            return false;
                        }
                        let Some(length) = this.animation_length() else {
                            this.animation_playing = false;
                            return false;
                        };
                        if length <= 0.0 {
                            this.animation_playing = false;
                            return false;
                        }
                        this.animation_time = (this.animation_time + 0.05) % length;
                        this.render_preview();
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }
}
