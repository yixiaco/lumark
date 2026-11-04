//! 查看器帧泵的纯状态机：单飞行帧、输入合并与静止高质量帧。
//!
//! 这一层不接触 GPUI/GPU/时间源，只根据 `request`/`start`/`finish`/`take_settled`
//! 的调用顺序维护状态，便于用合成时间做单元测试。UI 线程的输入只会调用
//! [`FramePump::request`]，渲染完成后再由 [`FramePump::finish`] 决定是否立即
//! 补最新一帧；静止超过 [`SETTLE_DELAY`] 后补一帧高质量画面。

use std::time::{Duration, Instant};

/// 静止判定阈值：最后一次输入后超过该时长补一帧高质量画面。
pub(super) const SETTLE_DELAY: Duration = Duration::from_millis(180);

/// 帧泵状态机。
#[derive(Debug, Default)]
pub(super) struct FramePump {
    /// 是否已有帧在后台渲染。
    in_flight: bool,
    /// 渲染期间是否收到新输入；只保留最新状态。
    dirty: bool,
    /// 是否已经补过静止高质量帧。
    settled_done: bool,
    /// 进入空闲的时刻，用于静止判定。
    idle_since: Option<Instant>,
}

impl FramePump {
    /// 创建空闲帧泵。
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// 是否有帧在渲染。
    pub(super) fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// 交互输入到达：记录 dirty、重置静止计时；返回是否应立即启动渲染。
    pub(super) fn request(&mut self) -> bool {
        self.dirty = true;
        self.idle_since = None;
        self.settled_done = false;
        !self.in_flight
    }

    /// 启动一帧：清除 dirty 并标记飞行中。
    pub(super) fn start(&mut self) {
        self.in_flight = true;
        self.dirty = false;
    }

    /// 一帧完成：返回是否还有最新输入需要立即再渲染一帧。
    pub(super) fn finish(&mut self, now: Instant) -> bool {
        self.in_flight = false;
        if self.dirty {
            true
        } else {
            self.idle_since = Some(now);
            false
        }
    }

    /// 是否需要在静止阈值后补一帧高质量画面。
    pub(super) fn wants_settled(&self) -> bool {
        !self.in_flight && !self.dirty && !self.settled_done
    }

    /// 静止计时到达后消费一次高质量帧请求；未到时间或已有帧在渲染时返回 false。
    pub(super) fn take_settled(&mut self, now: Instant) -> bool {
        if !self.wants_settled() {
            return false;
        }
        let Some(since) = self.idle_since else {
            return false;
        };
        if now.saturating_duration_since(since) < SETTLE_DELAY {
            return false;
        }
        self.settled_done = true;
        true
    }

    /// 是否已经补过静止高质量帧（测试用）。
    #[cfg(test)]
    pub(super) fn has_settled_frame(&self) -> bool {
        self.settled_done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空闲时第一次请求立即启动渲染，飞行中重复请求只置 dirty。
    #[test]
    fn coalesces_inputs_into_one_follow_up_frame() {
        let mut pump = FramePump::new();
        let now = Instant::now();
        assert!(pump.request(), "空闲时的请求应立即启动渲染");
        pump.start();
        assert!(!pump.request(), "飞行帧期间的请求不应再启动渲染");
        assert!(!pump.request(), "多次输入仍只合并为一帧");
        assert!(pump.finish(now), "存在待渲染输入时应立即补最新一帧");
        pump.start();
        assert!(!pump.finish(now), "没有新输入时不应继续渲染");
        assert!(!pump.in_flight());
    }

    /// 静止帧只有在超过 180ms 且完全空闲时才补一次。
    #[test]
    fn settled_frame_waits_for_delay_and_runs_once() {
        let mut pump = FramePump::new();
        let now = Instant::now();
        assert!(pump.request());
        pump.start();
        assert!(!pump.finish(now));
        assert!(pump.wants_settled(), "空闲后应等待补高质量帧");
        assert!(
            !pump.take_settled(now + Duration::from_millis(100)),
            "未到 180ms 不应补高质量帧"
        );
        assert!(
            pump.take_settled(now + SETTLE_DELAY),
            "到达静止阈值后应补一帧"
        );
        assert!(
            !pump.take_settled(now + Duration::from_secs(10)),
            "高质量帧只补一次"
        );
        assert!(!pump.wants_settled(), "补过之后不再请求");
    }

    /// 飞行帧期间不会触发静止帧；完成后重新计时。
    #[test]
    fn in_flight_frame_blocks_settled_frame() {
        let mut pump = FramePump::new();
        let now = Instant::now();
        assert!(pump.request());
        pump.start();
        assert!(
            !pump.take_settled(now + Duration::from_secs(5)),
            "渲染中不应补静止帧"
        );
        assert!(!pump.finish(now));
        assert!(
            !pump.take_settled(now + Duration::from_millis(50)),
            "刚完成的帧应重新计时"
        );
        assert!(pump.take_settled(now + SETTLE_DELAY));
    }

    /// 新的交互会重新允许一次高质量帧（例如旋转后再次静止）。
    #[test]
    fn new_input_rearms_settled_frame() {
        let mut pump = FramePump::new();
        let now = Instant::now();
        assert!(pump.request());
        pump.start();
        assert!(!pump.finish(now));
        assert!(pump.take_settled(now + SETTLE_DELAY));
        pump.start();
        assert!(!pump.finish(now));
        assert!(!pump.wants_settled(), "刚补过的静止帧不应重复");

        assert!(pump.request(), "空闲时的新输入应立即渲染");
        pump.start();
        assert!(!pump.finish(now + SETTLE_DELAY));
        assert!(pump.wants_settled(), "新输入后应重新等待静止帧");
        assert!(pump.take_settled(now + SETTLE_DELAY * 2));
    }
}
