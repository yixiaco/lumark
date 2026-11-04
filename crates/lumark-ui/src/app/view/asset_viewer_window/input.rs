//! 查看器快捷键：F 重置、Space 播放、←/→ 帧步进、L/G/B/N 着色与叠加层。

use super::*;

impl AssetViewerWindowView {
    /// 处理快捷键；返回是否消费该按键。
    pub(super) fn handle_shortcut(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key.to_ascii_lowercase().as_str() {
            "f" => {
                self.yaw = 35.0;
                self.pitch = 25.0;
                self.zoom = 1.0;
                self.pan = [0.0, 0.0];
                self.request_preview(cx);
            }
            "space" => {
                if self.has_animation() {
                    self.toggle_animation(cx);
                }
                return true;
            }
            "arrowleft" => {
                if self.has_animation() {
                    self.step_animation(-1, cx);
                }
                return true;
            }
            "arrowright" => {
                if self.has_animation() {
                    self.step_animation(1, cx);
                }
                return true;
            }
            "l" => {
                self.shading = match self.shading {
                    ShadingMode::Lit => ShadingMode::Unlit,
                    ShadingMode::Unlit => ShadingMode::Normals,
                    ShadingMode::Normals => ShadingMode::Lit,
                };
                self.request_preview(cx);
            }
            "g" => {
                self.show_grid = !self.show_grid;
                self.request_preview(cx);
            }
            "w" => {
                self.wireframe = !self.wireframe;
                self.request_preview(cx);
            }
            "b" => {
                self.show_bones = !self.show_bones;
                self.request_preview(cx);
            }
            "n" => {
                self.shading = ShadingMode::Normals;
                self.request_preview(cx);
            }
            _ => return false,
        }
        cx.notify();
        true
    }
}
