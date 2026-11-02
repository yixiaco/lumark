//! 异步帧泵（V-16）：拖拽/滚轮时把离屏渲染放到后台线程，只应用最新一帧。

use super::*;
use crate::render_service::render_scene_image;

impl AssetViewerWindowView {
    /// 请求异步渲染当前视角；旧请求的结果会被代号检查丢弃。
    pub(super) fn request_preview(&mut self, cx: &mut Context<Self>) {
        let Some((scene, settings)) = self.scene_and_settings() else {
            // 骨骼/材质查看器走轻量同步光栅。
            self.render_preview();
            return;
        };
        self.render_generation = self.render_generation.wrapping_add(1);
        let generation = self.render_generation;
        self.rendering = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { render_scene_image(&scene, &settings) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.render_generation != generation {
                    return;
                }
                view.rendering = false;
                match result {
                    Ok(rendered) => view.apply_rendered(rendered),
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
