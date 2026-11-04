//! 查看器帧泵：单飞行帧 + 输入合并 + 静止高质量帧。
//!
//! UI 线程只把当前视角整理成 [`RenderSnapshot`]（Arc 共享场景）并置 dirty；
//! 场景构建与离屏渲染都在后台线程执行。同一时刻只渲染一帧，拖拽/滚轮期间的
//! 输入只更新 dirty，完成后立即用最新状态补一帧；静止超过 `SETTLE_DELAY` 后
//! 再补一帧高质量画面。

use std::time::Instant;

use super::frame_pump::SETTLE_DELAY;
use super::render::{RenderQuality, ViewerFrame, render_snapshot};
use super::*;

impl AssetViewerWindowView {
    /// 请求渲染当前视角；已有帧在渲染时只置 dirty，完成后自动补最新一帧。
    pub(super) fn request_preview(&mut self, cx: &mut Context<Self>) {
        let should_start = self.frame_pump.request();
        self.sync_rendering();
        if should_start {
            self.start_render(cx, RenderQuality::Dragging);
        }
    }

    /// 启动一帧后台渲染（调用方保证当前没有飞行帧）。
    fn start_render(&mut self, cx: &mut Context<Self>, quality: RenderQuality) {
        self.frame_pump.start();
        self.sync_rendering();
        self.render_generation = self.render_generation.wrapping_add(1);
        let generation = self.render_generation;
        let snapshot = self.snapshot(quality);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { render_snapshot(&snapshot) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.complete_render(generation, result, cx);
            });
        })
        .detach();
    }

    /// 应用后台结果并决定下一帧：优先补最新输入，其次安排静止高质量帧。
    fn complete_render(
        &mut self,
        generation: u64,
        result: Result<ViewerFrame, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.render_generation {
            // 场景/代号已过期：丢弃结果，保持帧泵自身状态一致。
            self.frame_pump.finish(Instant::now());
            self.sync_rendering();
            return;
        }
        let has_pending = self.frame_pump.finish(Instant::now());
        self.sync_rendering();
        match result {
            Ok(frame) => {
                // 旧帧的图集瓦片由 GPUI 按 ImageId 缓存，必须显式释放。
                if let Some(previous) = self.apply_frame(frame) {
                    cx.drop_image(previous, None);
                }
            }
            Err(error) => self.error = Some(error),
        }
        if has_pending {
            self.start_render(cx, RenderQuality::Dragging);
        } else if self.frame_pump.wants_settled() {
            self.schedule_settled(cx);
        }
        cx.notify();
    }

    /// 静止阈值到达后补一帧高质量画面（SS=2 + 4×MSAA）。
    fn schedule_settled(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async {
                    std::thread::sleep(SETTLE_DELAY);
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.frame_pump.take_settled(Instant::now()) {
                    view.start_render(cx, RenderQuality::Settled);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 把帧泵状态同步到状态栏使用的 `rendering` 标记。
    fn sync_rendering(&mut self) {
        self.rendering = self.frame_pump.in_flight();
    }
}
