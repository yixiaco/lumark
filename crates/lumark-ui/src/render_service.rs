//! 共享渲染服务：缓存 GPU 设备与管线，无 GPU 时回退 CPU 光栅化。
//!
//! 输出直接转换为 GPUI `RenderImage`（BGRA），供详情预览、资产查看器等 UI
//! 使用；调用方不再需要写临时 PNG 文件再交给 GPUI 解码。

// V-3b 会把详情面板/查看器切到 RenderImage；在此之前部分 API 仅由测试使用。
#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use gpui_kit::RenderImage;
use lumark_render::SceneStats;
use lumark_render::{GpuRenderer, RenderSettings, RenderTarget, render_scene};
use lumark_scene::Scene;

/// 实际使用的渲染后端。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenderBackend {
    /// GPU 后端与适配器名称。
    Gpu(String),
    /// CPU 参考渲染器。
    Cpu,
}

impl RenderBackend {
    /// 状态栏/详情面板显示的标签。
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Gpu(name) => format!("GPU: {name}"),
            Self::Cpu => "CPU".to_string(),
        }
    }

    /// 是否使用 GPU。
    pub(crate) fn is_gpu(&self) -> bool {
        matches!(self, Self::Gpu(_))
    }
}

/// 一次场景渲染的结果。
pub(crate) struct RenderedImage {
    /// GPUI 可直接显示的图像。
    pub image: Arc<RenderImage>,
    /// 实际后端。
    pub backend: RenderBackend,
    /// 场景规模统计。
    pub stats: SceneStats,
}

/// 共享渲染服务。
pub(crate) struct RenderService {
    gpu: Option<GpuRenderer>,
    backend: RenderBackend,
}

impl RenderService {
    /// 创建服务：优先 GPU，失败时回退 CPU。
    pub(crate) fn new() -> Self {
        match GpuRenderer::new() {
            Ok(gpu) => {
                let backend = RenderBackend::Gpu(gpu.adapter_name().to_string());
                Self {
                    gpu: Some(gpu),
                    backend,
                }
            }
            Err(_) => Self {
                gpu: None,
                backend: RenderBackend::Cpu,
            },
        }
    }

    /// 当前后端。
    pub(crate) fn backend(&self) -> &RenderBackend {
        &self.backend
    }

    /// 后端标签。
    pub(crate) fn backend_label(&self) -> String {
        self.backend.label()
    }

    /// 渲染场景为 RGBA8 帧缓冲（保留给仍需文件输出的路径）。
    pub(crate) fn render_target(
        &self,
        scene: &Scene,
        settings: &RenderSettings,
    ) -> Result<RenderTarget, String> {
        match &self.gpu {
            Some(gpu) => gpu
                .render(scene, settings)
                .map_err(|error| format!("GPU 渲染失败：{error}")),
            None => render_scene(scene, settings).map_err(|error| format!("CPU 渲染失败：{error}")),
        }
    }

    /// 渲染场景并转换为 `RenderImage`。
    pub(crate) fn render(
        &self,
        scene: &Scene,
        settings: &RenderSettings,
    ) -> Result<RenderedImage, String> {
        let target = self.render_target(scene, settings)?;
        let image = render_target_to_image(&target)
            .ok_or_else(|| "渲染尺寸与像素数量不一致".to_string())?;
        Ok(RenderedImage {
            image,
            backend: self.backend.clone(),
            stats: lumark_render::scene_stats(scene),
        })
    }
}

/// 全局渲染服务（首次使用时初始化 GPU 设备）。
pub(crate) fn render_service() -> &'static RenderService {
    static SERVICE: OnceLock<RenderService> = OnceLock::new();
    SERVICE.get_or_init(RenderService::new)
}

/// 渲染场景为 GPUI 图像。
pub(crate) fn render_scene_image(
    scene: &Scene,
    settings: &RenderSettings,
) -> Result<RenderedImage, String> {
    render_service().render(scene, settings)
}

/// RGBA8 `RenderTarget` → GPUI `RenderImage`（BGRA）。
pub(crate) fn render_target_to_image(target: &RenderTarget) -> Option<Arc<RenderImage>> {
    let width = target.width;
    let height = target.height;
    if width == 0 || height == 0 {
        return None;
    }
    let mut bgra = target.to_bgra();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    let frame = image::Frame::new(buffer);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_scene::{MeshData, MeshSection, NodeKind, SceneNode, Transform};

    /// 构造红色三角形场景。
    fn triangle_scene() -> Scene {
        let mut scene = Scene::new("Test");
        let material = scene.add_material(lumark_scene::MaterialData {
            name: "Red".to_string(),
            base_color: [1.0, 0.0, 0.0, 1.0],
            ..lumark_scene::MaterialData::default()
        });
        let mut mesh = MeshData::new(
            "Triangle",
            vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 0.0]],
            vec![0, 1, 2],
        );
        mesh.sections = vec![MeshSection {
            material_index: 0,
            first_index: 0,
            index_count: 3,
        }];
        let mesh = scene.add_mesh(mesh);
        scene.add_node(SceneNode {
            name: "Triangle".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh),
            materials: vec![Some(material)],
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        scene
    }

    /// RenderTarget 转换为 RenderImage 时交换红蓝通道并保持尺寸。
    #[test]
    fn converts_render_target_to_image() {
        let target = RenderTarget::new(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).expect("目标");
        let image = render_target_to_image(&target).expect("RenderImage");
        assert_eq!(image.size(0).width.0, 2);
        assert_eq!(image.size(0).height.0, 1);
        assert_eq!(image.frame_count(), 1);
        let bytes = image.as_bytes(0).expect("像素");
        assert_eq!(&bytes[..8], &[3, 2, 1, 255, 7, 6, 5, 255]);
    }

    /// 渲染服务能把场景渲染成 RenderImage；无 GPU 时使用 CPU。
    #[test]
    fn renders_scene_to_render_image() {
        let settings = RenderSettings {
            width: 48,
            height: 48,
            supersample: 1,
            ..RenderSettings::default()
        };
        let rendered = render_scene_image(&triangle_scene(), &settings).expect("渲染");
        assert_eq!(rendered.image.size(0).width.0, 48);
        assert_eq!(rendered.image.size(0).height.0, 48);
        assert_eq!(rendered.image.frame_count(), 1);
        assert!(
            rendered.backend.is_gpu() || rendered.backend.label() == "CPU",
            "未知后端：{}",
            rendered.backend.label()
        );
        let bytes = rendered.image.as_bytes(0).expect("像素");
        assert_eq!(bytes.len(), 48 * 48 * 4);
    }
}
