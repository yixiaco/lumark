//! wgpu 30 离屏渲染后端（V-2b）。
//!
//! 优先选择独立显卡（`PowerPreference::HighPerformance`），失败时回退默认适配器
//! 与软件适配器；渲染到离屏纹理后回读 RGBA8，供 GPUI `RenderImage` 或截图使用。
//! 着色模型与 CPU 参考渲染器一致（双方向光 + 环境光、基础色/法线贴图、超采样）。

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use lumark_scene::{MaterialData, MeshData, OrmLayout, Scene};

use crate::camera::{Camera, normalize};
use crate::skin;
use crate::{LightingMode, NormalMapConvention, RenderSettings, RenderTarget};

/// WGSL 着色器。
const SHADER: &str = include_str!("shader.wgsl");
/// 叠加层线段着色器。
const LINE_SHADER: &str = include_str!("line.wgsl");
const BACKGROUND_SHADER: &str = include_str!("background.wgsl");
const SNOOPER_SSAO_SHADER: &str = include_str!("snooper_ssao.wgsl");
const SNOOPER_SHADOW_SHADER: &str = include_str!("snooper_shadow.wgsl");
const SNOOPER_FXAA_SHADER: &str = include_str!("snooper_fxaa.wgsl");
const SNOOPER_SSAO_BLUR_SHADER: &str = include_str!("snooper_ssao_blur.wgsl");
const SNOOPER_GBUFFER_SHADER: &str = include_str!("snooper_gbuffer.wgsl");
const SNOOPER_DEFERRED_SHADER: &str = include_str!("snooper_deferred.wgsl");
const SNOOPER_GRID_SHADER: &str = include_str!("snooper_grid.wgsl");

/// 颜色目标格式（着色器手动 gamma 编码）。
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// G-buffer 颜色格式。
const GBUFFER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// 深度格式。
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// 每个顶点的 f32 数量：position3 + normal3 + uv2 + color4 + tangent4 + joints4 + weights4。
const VERTEX_FLOATS: usize = 24;
/// 相机 uniform 大小（字节）。
const CAMERA_UNIFORM_SIZE: u64 = 592;
/// 材质 uniform 大小（字节）。
const MATERIAL_UNIFORM_SIZE: u64 = 112;

/// GPU 渲染错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuError {
    message: String,
}

impl GpuError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 错误描述。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GpuError {}

/// wgpu 渲染器（设备、管线与共享资源）。
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    adapter_backend: String,
    pipeline_single: wgpu::RenderPipeline,
    pipeline_double: wgpu::RenderPipeline,
    pipeline_single_4x: wgpu::RenderPipeline,
    pipeline_double_4x: wgpu::RenderPipeline,
    pipeline_blend_single: wgpu::RenderPipeline,
    pipeline_blend_double: wgpu::RenderPipeline,
    pipeline_blend_single_4x: wgpu::RenderPipeline,
    pipeline_blend_double_4x: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    line_pipeline_4x: wgpu::RenderPipeline,
    background_pipeline: wgpu::RenderPipeline,
    background_pipeline_4x: wgpu::RenderPipeline,
    background_layout: wgpu::BindGroupLayout,
    camera_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    ssao_layout: wgpu::BindGroupLayout,
    ssao_pipeline: wgpu::RenderPipeline,
    ssao_blur_layout: wgpu::BindGroupLayout,
    ssao_blur_pipeline: wgpu::RenderPipeline,
    gbuffer_pipeline: wgpu::RenderPipeline,
    deferred_layout: wgpu::BindGroupLayout,
    deferred_pipeline: wgpu::RenderPipeline,
    grid_layout: wgpu::BindGroupLayout,
    grid_pipeline: wgpu::RenderPipeline,
    ssao_sampler: wgpu::Sampler,
    shadow_camera_layout: wgpu::BindGroupLayout,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_sampler: wgpu::Sampler,
    fxaa_layout: wgpu::BindGroupLayout,
    fxaa_pipeline: wgpu::RenderPipeline,
    fxaa_sampler: wgpu::Sampler,
    sampler: wgpu::Sampler,
    sky_lut_texture: wgpu::Texture,
    sky_lut_view: wgpu::TextureView,
    sky_lut_sun: std::sync::Mutex<Option<[f32; 3]>>,
    sky_sampler: wgpu::Sampler,
    white_view: wgpu::TextureView,
    flat_normal_view: wgpu::TextureView,
    /// 纹理 LRU：跨帧复用已上传的 GPU 纹理。
    texture_cache: std::sync::Mutex<TextureCache>,
    /// 离屏渲染目标缓存：同尺寸/采样数时复用颜色、深度与回读缓冲。
    targets: std::sync::Mutex<Option<RenderTargets>>,
}

/// 纹理缓存键：名称/尺寸/采样格式/内容指纹（材质批次每帧克隆）。
type TextureKey = (String, u32, u32, bool, u64);

/// 采样内容指纹：避免同名不同内容的纹理错误复用。
fn texture_fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    let step = (bytes.len() / 256).max(1);
    for byte in bytes.iter().step_by(step) {
        byte.hash(&mut hasher);
    }
    hasher.finish()
}

/// 缓存中的 GPU 纹理（视图持有纹理引用）。
struct CachedTexture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// 简单 LRU 纹理缓存。
#[derive(Default)]
struct TextureCache {
    entries: HashMap<TextureKey, CachedTexture>,
    order: std::collections::VecDeque<TextureKey>,
}

impl TextureCache {
    /// 命中并把键移到队尾。
    fn get(&mut self, key: &TextureKey) -> Option<wgpu::TextureView> {
        let view = self.entries.get(key)?.view.clone();
        if let Some(position) = self.order.iter().position(|entry| entry == key) {
            self.order.remove(position);
        }
        self.order.push_back(key.clone());
        Some(view)
    }

    /// 插入并淘汰最久未使用的纹理。
    fn insert(&mut self, key: TextureKey, texture: CachedTexture, capacity: usize) {
        self.entries.insert(key.clone(), texture);
        self.order.push_back(key);
        while self.order.len() > capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
}

/// 纹理缓存容量。
const TEXTURE_CACHE_CAPACITY: usize = 64;

/// 离屏渲染目标缓存键：`(宽, 高, MSAA 采样数)`。
type TargetKey = (u32, u32, u32);

/// 一次离屏渲染需要的 GPU 目标；尺寸与采样数一致时跨帧复用。
struct RenderTargets {
    /// 缓存键。
    key: TargetKey,
    /// 单采样颜色目标（也是回读来源）。
    color: wgpu::Texture,
    /// 颜色目标视图。
    color_view: wgpu::TextureView,
    /// 多重采样颜色视图（`samples > 1` 时存在）。
    msaa_view: Option<wgpu::TextureView>,
    /// 深度纹理（SSAO 采样）。
    _depth_texture: wgpu::Texture,
    /// 深度视图。
    depth_view: wgpu::TextureView,
    /// G-buffer：位置/法线/基础色/高光。
    _gbuffer_position: wgpu::Texture,
    gbuffer_position_view: wgpu::TextureView,
    _gbuffer_normal: wgpu::Texture,
    gbuffer_normal_view: wgpu::TextureView,
    _gbuffer_albedo: wgpu::Texture,
    gbuffer_albedo_view: wgpu::TextureView,
    _gbuffer_specular: wgpu::Texture,
    gbuffer_specular_view: wgpu::TextureView,
    _gbuffer_depth: wgpu::Texture,
    gbuffer_depth_view: wgpu::TextureView,
    /// SSAO 原始 AO（R=ao,G=depth,B/A=normal.xy）。
    _ssao_ao: wgpu::Texture,
    ssao_ao_view: wgpu::TextureView,
    /// SSAO H 模糊。
    _ssao_blur: wgpu::Texture,
    ssao_blur_view: wgpu::TextureView,
    /// SSAO 后处理颜色目标。
    ssao_color: wgpu::Texture,
    /// SSAO 后处理视图。
    ssao_view: wgpu::TextureView,
    /// FXAA 输出纹理。
    fxaa_color: wgpu::Texture,
    /// FXAA 输出视图。
    fxaa_view: wgpu::TextureView,
    /// 太阳阴影贴图。
    _shadow_texture: wgpu::Texture,
    /// 太阳阴影视图（2 级联数组）。
    shadow_view: wgpu::TextureView,
    /// 每个级联的渲染视图。
    shadow_layer_views: [wgpu::TextureView; 4],
    /// 回读缓冲；行字节数补齐到 256 的倍数。
    readback: wgpu::Buffer,
    /// 回读缓冲的补齐行宽（字节）。
    padded_row: u32,
}

impl RenderTargets {
    /// 按尺寸与采样数创建颜色/深度/回读资源。
    fn new(device: &wgpu::Device, key: TargetKey) -> Self {
        let (width, height, samples) = key;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let half_size = wgpu::Extent3d {
            width: (width / 2).max(1),
            height: (height / 2).max(1),
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let ssao_color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-ssao-color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let create_gbuffer = |label: &str| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: GBUFFER_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        };
        let (gbuffer_position, gbuffer_position_view) = create_gbuffer("lumark-gbuffer-position");
        let (gbuffer_normal, gbuffer_normal_view) = create_gbuffer("lumark-gbuffer-normal");
        let (gbuffer_albedo, gbuffer_albedo_view) = create_gbuffer("lumark-gbuffer-albedo");
        let (gbuffer_specular, gbuffer_specular_view) = create_gbuffer("lumark-gbuffer-specular");
        let gbuffer_depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-gbuffer-depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let gbuffer_depth_view = gbuffer_depth.create_view(&wgpu::TextureViewDescriptor::default());
        let ssao_ao = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-ssao-ao"),
            size: half_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let ssao_ao_view = ssao_ao.create_view(&wgpu::TextureViewDescriptor::default());
        let ssao_blur = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-ssao-blur"),
            size: half_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let ssao_blur_view = ssao_blur.create_view(&wgpu::TextureViewDescriptor::default());
        let ssao_view = ssao_color.create_view(&wgpu::TextureViewDescriptor::default());
        let fxaa_color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-fxaa-color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let fxaa_view = fxaa_color.create_view(&wgpu::TextureViewDescriptor::default());
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        // MSAA：先渲染到多重采样纹理，再 resolve 到上面的单采样目标。
        let msaa_view = (samples > 1).then(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("lumark-msaa-color"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            texture.create_view(&wgpu::TextureViewDescriptor::default())
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-depth"),
            size,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-shadow-map"),
            size: wgpu::Extent3d {
                width: 2048,
                height: 2048,
                depth_or_array_layers: 4,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow.create_view(&wgpu::TextureViewDescriptor {
            label: Some("lumark-shadow-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let shadow_layer_view = |layer| {
            shadow.create_view(&wgpu::TextureViewDescriptor {
                label: Some("lumark-shadow-layer"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        };
        let shadow_layer_views = [
            shadow_layer_view(0),
            shadow_layer_view(1),
            shadow_layer_view(2),
            shadow_layer_view(3),
        ];
        // 回读：copy_texture_to_buffer 的每行字节数必须是 256 的倍数。
        let padded_row = width.div_ceil(64) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumark-readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            key,
            color,
            color_view,
            msaa_view,
            _depth_texture: depth,
            depth_view,
            _gbuffer_position: gbuffer_position,
            gbuffer_position_view,
            _gbuffer_normal: gbuffer_normal,
            gbuffer_normal_view,
            _gbuffer_albedo: gbuffer_albedo,
            gbuffer_albedo_view,
            _gbuffer_specular: gbuffer_specular,
            gbuffer_specular_view,
            _gbuffer_depth: gbuffer_depth,
            gbuffer_depth_view,
            _ssao_ao: ssao_ao,
            ssao_ao_view,
            _ssao_blur: ssao_blur,
            ssao_blur_view,
            ssao_color,
            ssao_view,
            fxaa_color,
            fxaa_view,
            _shadow_texture: shadow,
            shadow_view,
            shadow_layer_views,
            readback,
            padded_row,
        }
    }
}

impl fmt::Debug for GpuRenderer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GpuRenderer")
            .field("adapter_name", &self.adapter_name)
            .field("adapter_backend", &self.adapter_backend)
            .finish()
    }
}

impl GpuRenderer {
    /// 创建 GPU 渲染器；优先独显，其次默认/软件适配器。
    pub fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::default();
        let preferences = [
            (wgpu::PowerPreference::HighPerformance, false),
            (wgpu::PowerPreference::None, false),
            (wgpu::PowerPreference::None, true),
        ];
        let mut last_error = String::new();
        for (power_preference, force_fallback_adapter) in preferences {
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference,
                    force_fallback_adapter,
                    compatible_surface: None,
                    ..Default::default()
                }));
            let Ok(adapter) = adapter else {
                last_error = format!("请求适配器失败（{power_preference:?}）");
                continue;
            };
            let info = adapter.get_info();
            let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("lumark-render"),
                ..Default::default()
            }));
            let Ok((device, queue)) = device else {
                last_error = format!("创建设备失败：{device:?}");
                continue;
            };
            let resources = create_resources(&device, &queue)?;
            return Ok(Self {
                device,
                queue,
                adapter_name: info.name,
                adapter_backend: format!("{:?}", info.backend),
                pipeline_single: resources.pipeline_single,
                pipeline_double: resources.pipeline_double,
                pipeline_single_4x: resources.pipeline_single_4x,
                pipeline_double_4x: resources.pipeline_double_4x,
                pipeline_blend_single: resources.pipeline_blend_single,
                pipeline_blend_double: resources.pipeline_blend_double,
                pipeline_blend_single_4x: resources.pipeline_blend_single_4x,
                pipeline_blend_double_4x: resources.pipeline_blend_double_4x,
                line_pipeline: resources.line_pipeline,
                line_pipeline_4x: resources.line_pipeline_4x,
                background_pipeline: resources.background_pipeline,
                background_pipeline_4x: resources.background_pipeline_4x,
                background_layout: resources.background_layout,
                camera_layout: resources.camera_layout,
                material_layout: resources.material_layout,
                ssao_layout: resources.ssao_layout,
                ssao_pipeline: resources.ssao_pipeline,
                ssao_blur_layout: resources.ssao_blur_layout,
                ssao_blur_pipeline: resources.ssao_blur_pipeline,
                gbuffer_pipeline: resources.gbuffer_pipeline,
                deferred_layout: resources.deferred_layout,
                deferred_pipeline: resources.deferred_pipeline,
                grid_layout: resources.grid_layout,
                grid_pipeline: resources.grid_pipeline,
                ssao_sampler: resources.ssao_sampler,
                shadow_camera_layout: resources.shadow_camera_layout,
                shadow_pipeline: resources.shadow_pipeline,
                shadow_sampler: resources.shadow_sampler,
                fxaa_layout: resources.fxaa_layout,
                fxaa_pipeline: resources.fxaa_pipeline,
                fxaa_sampler: resources.fxaa_sampler,
                sampler: resources.sampler,
                sky_lut_texture: resources.sky_lut_texture,
                sky_lut_view: resources.sky_lut_view,
                sky_lut_sun: std::sync::Mutex::new(None),
                sky_sampler: resources.sky_sampler,
                white_view: resources.white_view,
                flat_normal_view: resources.flat_normal_view,
                texture_cache: std::sync::Mutex::new(TextureCache::default()),
                targets: std::sync::Mutex::new(None),
            });
        }
        Err(GpuError::new(format!(
            "没有可用的 wgpu 适配器：{last_error}"
        )))
    }

    /// 适配器名称。
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// 后端名称（如 `Dx12`/`Vulkan`）。
    pub fn adapter_backend(&self) -> &str {
        &self.adapter_backend
    }

    /// 渲染场景到 RGBA8（与 CPU 后端同一输出类型）。
    pub fn render(
        &self,
        scene: &Scene,
        settings: &RenderSettings,
    ) -> Result<RenderTarget, GpuError> {
        if !(1..=4096).contains(&settings.width) || !(1..=4096).contains(&settings.height) {
            return Err(GpuError::new("渲染尺寸必须在 1..=4096 之间"));
        }
        let supersample = settings.supersample.clamp(1, 4);
        let use_ssao =
            settings.lighting_mode == LightingMode::Snooper && settings.snooper.ssao.enabled;
        let use_fxaa = settings.lighting_mode == LightingMode::Snooper && settings.snooper.fxaa;
        let use_deferred = settings.lighting_mode == LightingMode::Snooper
            && settings.shading == crate::ShadingMode::Lit;
        self.update_sky_lut_for_settings(settings);
        // SSAO 需要单采样颜色/深度纹理。
        let samples = if use_ssao {
            1
        } else if settings.msaa_samples >= 4 {
            4
        } else {
            1
        };
        let width = settings.width * supersample;
        let height = settings.height * supersample;
        let bounds = crate::render_bounds(scene);
        let camera = Camera::fit(&bounds, settings);
        let batches = build_batches(scene, settings);
        // 渲染目标按「尺寸 + MSAA 采样数」缓存，尺寸不变时跨帧复用。
        let key = (width, height, samples);
        let mut target_cache = self
            .targets
            .lock()
            .map_err(|_| GpuError::new("渲染目标缓存锁失败"))?;
        if target_cache.as_ref().map(|targets| targets.key) != Some(key) {
            *target_cache = Some(RenderTargets::new(&self.device, key));
        }
        let targets = target_cache.as_ref().expect("渲染目标已创建");

        let camera_buffer = self.create_buffer(
            "lumark-camera",
            CAMERA_UNIFORM_SIZE,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        self.queue
            .write_buffer(&camera_buffer, 0, &camera_uniform(&camera, settings));
        let camera_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lumark-camera"),
            layout: &self.camera_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&targets.shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumark-render"),
            });
        if settings.lighting_mode == LightingMode::Snooper && settings.snooper.shadows.enabled {
            let fallback_material = MaterialData::default();
            for (layer, projection) in camera.shadow_view_projections.iter().enumerate() {
                let mut shadow_camera = Vec::with_capacity(16);
                for column in projection {
                    shadow_camera.extend_from_slice(column);
                }
                let shadow_camera_buffer = self.create_buffer(
                    "lumark-shadow-camera",
                    64,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                );
                self.queue
                    .write_buffer(&shadow_camera_buffer, 0, &f32_bytes(&shadow_camera));
                let shadow_camera_group =
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("lumark-shadow-camera"),
                        layout: &self.shadow_camera_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: shadow_camera_buffer.as_entire_binding(),
                        }],
                    });
                let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("lumark-shadow-pass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.shadow_layer_views[layer],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                shadow_pass.set_bind_group(0, &shadow_camera_group, &[]);
                for batch in &batches {
                    let material = batch.material(scene, &fallback_material);
                    if material.translucent {
                        continue;
                    }
                    self.draw_batch(
                        &mut shadow_pass,
                        settings,
                        batch,
                        material,
                        &self.shadow_pipeline,
                    );
                }
            }
        }
        if use_deferred {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-gbuffer-pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_position_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_normal_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_albedo_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_specular_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.gbuffer_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &camera_bind_group, &[]);
            let fallback_material = MaterialData::default();
            for batch in &batches {
                let material = batch.material(scene, &fallback_material);
                if material.translucent || material.name == "SnooperGround" {
                    continue;
                }
                self.draw_batch(&mut pass, settings, batch, material, &self.gbuffer_pipeline);
            }
        }
        {
            let (render_view, resolve_target) = match &targets.msaa_view {
                Some(view) => (view, Some(&targets.color_view)),
                None => (&targets.color_view, None),
            };
            let line_pipeline = if samples > 1 {
                &self.line_pipeline_4x
            } else {
                &self.line_pipeline
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: render_view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color(
                            settings.background,
                            settings.lighting_mode == LightingMode::Snooper,
                        )),
                        store: if samples > 1 {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let background_data = background_uniform(settings, &camera);
            let background_buffer = self.create_buffer(
                "lumark-background",
                (background_data.len() * 4) as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            self.queue
                .write_buffer(&background_buffer, 0, &f32_bytes(&background_data));
            let background_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lumark-background"),
                layout: &self.background_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: background_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.sky_lut_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                    },
                ],
            });
            pass.set_pipeline(if samples > 1 {
                &self.background_pipeline_4x
            } else {
                &self.background_pipeline
            });
            pass.set_bind_group(0, &background_bind_group, &[]);
            pass.draw(0..3, 0..1);
            pass.set_bind_group(0, &camera_bind_group, &[]);
            // 批次只保存材质索引，避免每帧克隆材质与贴图数据。
            let fallback_material = MaterialData::default();
            // 先绘制不透明/Masked 批次；半透明批次最后按距相机由远到近混合。
            let mut translucent: Vec<(&Batch, f32)> = Vec::new();
            for batch in &batches {
                let material = batch.material(scene, &fallback_material);
                if material.translucent {
                    let delta = [
                        batch.sort_center[0] - camera.eye[0],
                        batch.sort_center[1] - camera.eye[1],
                        batch.sort_center[2] - camera.eye[2],
                    ];
                    translucent.push((
                        batch,
                        delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2],
                    ));
                } else {
                    let pipeline =
                        self.select_material_pipeline(samples, material.two_sided, false);
                    self.draw_batch(&mut pass, settings, batch, material, pipeline);
                }
            }
            translucent.sort_by(|left, right| {
                right
                    .1
                    .partial_cmp(&left.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for (batch, _) in translucent {
                let material = batch.material(scene, &fallback_material);
                let pipeline = self.select_material_pipeline(samples, material.two_sided, true);
                self.draw_batch(&mut pass, settings, batch, material, pipeline);
            }
            let (line_data, line_count) = overlay_line_vertices(scene, settings);
            if line_count > 0 {
                let line_buffer = self.create_buffer(
                    "lumark-overlay-lines",
                    line_data.len() as u64,
                    wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                );
                self.queue.write_buffer(&line_buffer, 0, &line_data);
                pass.set_pipeline(line_pipeline);
                pass.set_bind_group(0, &camera_bind_group, &[]);
                pass.set_vertex_buffer(0, line_buffer.slice(..));
                pass.draw(0..line_count, 0..1);
            }
        }

        if use_deferred && settings.show_grid {
            let mut values = Vec::with_capacity(56);
            for column in &camera.view_projection {
                values.extend_from_slice(column);
            }
            values.extend_from_slice(&camera.eye);
            values.extend_from_slice(&[0.0]);
            values.extend_from_slice(&camera.right);
            values.extend_from_slice(&[0.0]);
            values.extend_from_slice(&camera.up);
            values.extend_from_slice(&[0.0]);
            values.extend_from_slice(&camera.forward);
            values.extend_from_slice(&[0.0]);
            values.extend_from_slice(&[camera.tan_half_fov, camera.aspect, 20.0, 100.0]);
            values.extend_from_slice(&[0.12, 0.13, 0.15, 1.0]);
            values.extend_from_slice(&[0.15, 0.16, 0.18, 1.0]);
            values.extend_from_slice(&[0.28, 0.30, 0.33, 0.35]);
            values.extend_from_slice(&[0.45, 0.48, 0.52, 0.5]);
            values.extend_from_slice(&[camera.near, camera.far, 0.0, 0.0]);
            let uniform = self.create_buffer(
                "lumark-grid",
                (values.len() * 4) as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            self.queue.write_buffer(&uniform, 0, &f32_bytes(&values));
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lumark-grid"),
                layout: &self.grid_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-grid-pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_position_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_normal_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_albedo_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &targets.gbuffer_specular_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.gbuffer_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.grid_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if use_deferred {
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lumark-deferred"),
                layout: &self.deferred_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &targets.gbuffer_position_view,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&targets.gbuffer_normal_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&targets.gbuffer_albedo_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(
                            &targets.gbuffer_specular_view,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.ssao_sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-deferred-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.deferred_pipeline);
            pass.set_bind_group(0, &camera_bind_group, &[]);
            pass.set_bind_group(1, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if use_deferred {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-deferred-forward-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.gbuffer_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &camera_bind_group, &[]);
            let fallback_material = MaterialData::default();
            let mut translucent: Vec<&Batch> = batches
                .iter()
                .filter(|batch| batch.material(scene, &fallback_material).translucent)
                .collect();
            translucent.sort_by(|left, right| {
                let left_distance = [
                    left.sort_center[0] - camera.eye[0],
                    left.sort_center[1] - camera.eye[1],
                    left.sort_center[2] - camera.eye[2],
                ];
                let right_distance = [
                    right.sort_center[0] - camera.eye[0],
                    right.sort_center[1] - camera.eye[1],
                    right.sort_center[2] - camera.eye[2],
                ];
                let left_dot = dot(left_distance, left_distance);
                let right_dot = dot(right_distance, right_distance);
                right_dot
                    .partial_cmp(&left_dot)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for batch in translucent {
                let material = batch.material(scene, &fallback_material);
                let pipeline = self.select_material_pipeline(1, material.two_sided, true);
                self.draw_batch(&mut pass, settings, batch, material, pipeline);
            }
            let mut overlay_settings = settings.clone();
            overlay_settings.show_grid = false;
            let (line_data, line_count) = overlay_line_vertices(scene, &overlay_settings);
            if line_count > 0 {
                let line_buffer = self.create_buffer(
                    "lumark-lines",
                    line_data.len() as u64,
                    wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                );
                self.queue.write_buffer(&line_buffer, 0, &line_data);
                pass.set_bind_group(0, &camera_bind_group, &[]);
                pass.set_pipeline(&self.line_pipeline);
                pass.set_vertex_buffer(0, line_buffer.slice(..));
                pass.draw(0..line_count, 0..1);
            }
        }
        if use_ssao {
            let uniform = ssao_uniform(&camera, (width / 2).max(1), (height / 2).max(1));
            let uniform_buffer = self.create_buffer(
                "lumark-ssao",
                (uniform.len() * 4) as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            self.queue
                .write_buffer(&uniform_buffer, 0, &f32_bytes(&uniform));
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lumark-ssao"),
                layout: &self.ssao_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&targets.color_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(if use_deferred {
                            &targets.gbuffer_depth_view
                        } else {
                            &targets.depth_view
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.ssao_sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-ssao-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.ssao_ao_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.ssao_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            self.draw_ssao_blur(
                &mut encoder,
                [1.0 / (width / 2).max(1) as f32, 0.0],
                0.0,
                &targets.ssao_ao_view,
                &targets.color_view,
                &targets.ssao_blur_view,
            );
            self.draw_ssao_blur(
                &mut encoder,
                [0.0, 1.0 / (height / 2).max(1) as f32],
                1.0,
                &targets.ssao_blur_view,
                &targets.color_view,
                &targets.ssao_view,
            );
        }
        if use_fxaa {
            let source_view = if use_ssao {
                &targets.ssao_view
            } else {
                &targets.color_view
            };
            let uniform = [settings.width as f32, settings.height as f32, 0.08, 0.35];
            let uniform_buffer = self.create_buffer(
                "lumark-fxaa",
                (uniform.len() * 4) as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            self.queue
                .write_buffer(&uniform_buffer, 0, &f32_bytes(&uniform));
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lumark-fxaa"),
                layout: &self.fxaa_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.fxaa_sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-fxaa-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.fxaa_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.fxaa_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: if use_fxaa {
                    &targets.fxaa_color
                } else if use_ssao {
                    &targets.ssao_color
                } else {
                    &targets.color
                },
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        // 回读只 poll 一次：提交后登记映射，等待 GPU 完成时映射回调一并就绪。
        let padded_row = targets.padded_row;
        let slice = targets.readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| GpuError::new(format!("等待回读映射失败：{error}")))?;
        receiver
            .recv()
            .map_err(|error| GpuError::new(format!("回读通道失败：{error}")))?
            .map_err(|error| GpuError::new(format!("映射回读缓冲失败：{error}")))?;
        let data = slice
            .get_mapped_range()
            .map_err(|error| GpuError::new(format!("读取映射范围失败：{error}")))?;
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in 0..height {
            let start = (row * padded_row) as usize;
            let end = start + width as usize * 4;
            rgba.extend_from_slice(&data[start..end]);
        }
        drop(data);
        targets.readback.unmap();
        let rgba = if supersample > 1 {
            crate::downsample(&rgba, width, height, settings.width, settings.height)
        } else {
            rgba
        };
        Ok(RenderTarget {
            width: settings.width,
            height: settings.height,
            rgba,
        })
    }

    /// 选择材质管线：采样数、双面与半透明混合三种维度。
    fn select_material_pipeline(
        &self,
        samples: u32,
        two_sided: bool,
        alpha_blend: bool,
    ) -> &wgpu::RenderPipeline {
        match (samples > 1, two_sided, alpha_blend) {
            (false, false, false) => &self.pipeline_single,
            (false, true, false) => &self.pipeline_double,
            (true, false, false) => &self.pipeline_single_4x,
            (true, true, false) => &self.pipeline_double_4x,
            (false, false, true) => &self.pipeline_blend_single,
            (false, true, true) => &self.pipeline_blend_double,
            (true, false, true) => &self.pipeline_blend_single_4x,
            (true, true, true) => &self.pipeline_blend_double_4x,
        }
    }

    /// 太阳高度变化时重烘大气 LUT，保持天空与光照方向一致。
    fn update_sky_lut_for_settings(&self, settings: &RenderSettings) {
        let key = normalize(settings.key_direction);
        let sun = normalize([key[0], key[2], key[1]]);
        let Ok(mut current) = self.sky_lut_sun.lock() else {
            return;
        };
        if let Some(previous) = *current {
            if (previous[0] - sun[0]).abs() < 1e-4
                && (previous[1] - sun[1]).abs() < 1e-4
                && (previous[2] - sun[2]).abs() < 1e-4
            {
                return;
            }
        }
        let (width, height) = crate::sky_lut::size();
        let pixels = crate::sky_lut::bake_for_sun(sun);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.sky_lut_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        *current = Some(sun);
    }
    /// SSAO H/V 双边模糊；第二遍顺带把 AO 乘回颜色。
    fn draw_ssao_blur(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        direction: [f32; 2],
        mode: f32,
        input: &wgpu::TextureView,
        color: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) {
        let uniform = [direction[0], direction[1], mode, 0.0];
        let buffer = self.create_buffer(
            "lumark-ssao-blur",
            (uniform.len() * 4) as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        self.queue.write_buffer(&buffer, 0, &f32_bytes(&uniform));
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lumark-ssao-blur"),
            layout: &self.ssao_blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(color),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.ssao_sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lumark-ssao-blur-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.ssao_blur_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    /// 为一个批次创建顶点/材质/蒙皮缓冲并提交绘制。
    fn draw_batch(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        settings: &RenderSettings,
        batch: &Batch,
        material: &MaterialData,
        pipeline: &wgpu::RenderPipeline,
    ) {
        pass.set_pipeline(pipeline);
        let vertex_bytes = f32_bytes(&batch.vertices);
        let vertex_buffer = self.create_buffer(
            "lumark-vertices",
            vertex_bytes.len() as u64,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        self.queue.write_buffer(&vertex_buffer, 0, &vertex_bytes);
        let material_buffer = self.create_buffer(
            "lumark-material",
            MATERIAL_UNIFORM_SIZE,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        self.queue
            .write_buffer(&material_buffer, 0, &material_uniform(material));
        let base_upload = material
            .textures
            .base_color
            .as_ref()
            .and_then(|texture| self.upload_texture(texture, settings.srgb_textures));
        let normal_upload = material
            .textures
            .normal
            .as_ref()
            .and_then(|texture| self.upload_texture(texture, false));
        let orm_upload = material
            .textures
            .metallic_roughness
            .as_ref()
            .and_then(|texture| self.upload_texture(texture, false));
        let emissive_upload = material
            .textures
            .emissive
            .as_ref()
            .and_then(|texture| self.upload_texture(texture, settings.srgb_textures));
        let ao_upload = material
            .textures
            .occlusion
            .as_ref()
            .and_then(|texture| self.upload_texture(texture, false));
        let sclera_upload = material
            .textures
            .sclera
            .as_ref()
            .and_then(|texture| self.upload_texture(texture, settings.srgb_textures));
        let base_view = base_upload.as_ref().unwrap_or(&self.white_view);
        let normal_view = normal_upload.as_ref().unwrap_or(&self.flat_normal_view);
        let orm_view = orm_upload.as_ref().unwrap_or(&self.white_view);
        let emissive_view = emissive_upload.as_ref().unwrap_or(&self.white_view);
        let ao_view = ao_upload.as_ref().unwrap_or(&self.white_view);
        let sclera_view = sclera_upload.as_ref().unwrap_or(&self.white_view);
        let skin_data = skin_bytes(&batch.skin_matrices);
        let skin_buffer = self.create_buffer(
            "lumark-skin",
            skin_data.len() as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        self.queue.write_buffer(&skin_buffer, 0, &skin_data);
        let material_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lumark-material"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: material_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(orm_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(emissive_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: skin_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(ao_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(sclera_view),
                },
            ],
        });
        pass.set_bind_group(1, &material_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        let instance_data: Vec<f32> = batch
            .instances
            .iter()
            .flat_map(|matrix| matrix.iter().flat_map(|column| column.iter().copied()))
            .collect();
        let instance_buffer = self.create_buffer(
            "lumark-instances",
            (instance_data.len() * 4) as u64,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        // f32 切片 → 字节；instance_data 在本次 write_buffer 期间保持存活。
        let instance_bytes = unsafe {
            std::slice::from_raw_parts(
                instance_data.as_ptr().cast::<u8>(),
                instance_data.len() * std::mem::size_of::<f32>(),
            )
        };
        self.queue.write_buffer(&instance_buffer, 0, instance_bytes);
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.draw(0..batch.count, 0..batch.instances.len() as u32);
    }

    fn create_buffer(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    }

    /// 上传 RGBA8 纹理；`srgb` 为真时按 sRGB 采样（基础色），否则按线性（法线）。
    fn upload_texture(
        &self,
        texture: &lumark_scene::TextureData,
        srgb: bool,
    ) -> Option<wgpu::TextureView> {
        if !texture.is_valid() {
            return None;
        }
        let format = if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let key = (
            texture.name.clone(),
            texture.width,
            texture.height,
            srgb,
            texture_fingerprint(&texture.rgba),
        );
        if let Ok(mut cache) = self.texture_cache.lock()
            && let Some(view) = cache.get(&key)
        {
            return Some(view);
        }
        let gpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&texture.name),
            size: wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture.width * 4),
                rows_per_image: Some(texture.height),
            },
            wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
        );
        let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        if let Ok(mut cache) = self.texture_cache.lock() {
            cache.insert(
                key,
                CachedTexture {
                    _texture: gpu_texture,
                    view: view.clone(),
                },
                TEXTURE_CACHE_CAPACITY,
            );
        }
        Some(view)
    }
}

/// 共享管线资源。
struct Resources {
    pipeline_single: wgpu::RenderPipeline,
    pipeline_double: wgpu::RenderPipeline,
    pipeline_single_4x: wgpu::RenderPipeline,
    pipeline_double_4x: wgpu::RenderPipeline,
    pipeline_blend_single: wgpu::RenderPipeline,
    pipeline_blend_double: wgpu::RenderPipeline,
    pipeline_blend_single_4x: wgpu::RenderPipeline,
    pipeline_blend_double_4x: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    line_pipeline_4x: wgpu::RenderPipeline,
    background_pipeline: wgpu::RenderPipeline,
    background_pipeline_4x: wgpu::RenderPipeline,
    background_layout: wgpu::BindGroupLayout,
    camera_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    ssao_layout: wgpu::BindGroupLayout,
    ssao_pipeline: wgpu::RenderPipeline,
    ssao_blur_layout: wgpu::BindGroupLayout,
    ssao_blur_pipeline: wgpu::RenderPipeline,
    gbuffer_pipeline: wgpu::RenderPipeline,
    deferred_layout: wgpu::BindGroupLayout,
    deferred_pipeline: wgpu::RenderPipeline,
    grid_layout: wgpu::BindGroupLayout,
    grid_pipeline: wgpu::RenderPipeline,
    ssao_sampler: wgpu::Sampler,
    shadow_camera_layout: wgpu::BindGroupLayout,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_sampler: wgpu::Sampler,
    fxaa_layout: wgpu::BindGroupLayout,
    fxaa_pipeline: wgpu::RenderPipeline,
    fxaa_sampler: wgpu::Sampler,
    sampler: wgpu::Sampler,
    sky_lut_texture: wgpu::Texture,
    sky_lut_view: wgpu::TextureView,
    sky_sampler: wgpu::Sampler,
    white_view: wgpu::TextureView,
    flat_normal_view: wgpu::TextureView,
}

fn create_resources(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Resources, GpuError> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
    });
    let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-camera-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
        ],
    });
    let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-material-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 7,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-layout"),
        bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
        immediate_size: 0,
    });
    let vertex_attributes = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 12,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 24,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 32,
            shader_location: 3,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 48,
            shader_location: 4,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 64,
            shader_location: 5,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 80,
            shader_location: 6,
        },
    ];
    let instance_attributes = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 0,
            shader_location: 7,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 16,
            shader_location: 8,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 32,
            shader_location: 9,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 48,
            shader_location: 10,
        },
    ];
    let create_pipeline =
        |label: &'static str, cull_mode: Option<wgpu::Face>, samples: u32, alpha_blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: (VERTEX_FLOATS * 4) as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &vertex_attributes,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &instance_attributes,
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!alpha_blend),
                    depth_compare: Some(if alpha_blend {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Less
                    }),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: alpha_blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
    let pipeline_single =
        create_pipeline("lumark-pipeline-single", Some(wgpu::Face::Back), 1, false);
    let pipeline_double = create_pipeline("lumark-pipeline-double", None, 1, false);
    let pipeline_single_4x = create_pipeline(
        "lumark-pipeline-single-4x",
        Some(wgpu::Face::Back),
        4,
        false,
    );
    let pipeline_double_4x = create_pipeline("lumark-pipeline-double-4x", None, 4, false);
    let pipeline_blend_single = create_pipeline(
        "lumark-pipeline-blend-single",
        Some(wgpu::Face::Back),
        1,
        true,
    );
    let pipeline_blend_double = create_pipeline("lumark-pipeline-blend-double", None, 1, true);
    let pipeline_blend_single_4x = create_pipeline(
        "lumark-pipeline-blend-single-4x",
        Some(wgpu::Face::Back),
        4,
        true,
    );
    let pipeline_blend_double_4x =
        create_pipeline("lumark-pipeline-blend-double-4x", None, 4, true);

    // 叠加层线段管线：LineList、深度测试但不写深度。
    let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-line-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(LINE_SHADER)),
    });
    let line_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-line-layout"),
        bind_group_layouts: &[Some(&camera_layout)],
        immediate_size: 0,
    });
    let create_line_pipeline = |label: &'static str, samples: u32| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&line_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 28,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 12,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let line_pipeline = create_line_pipeline("lumark-line-pipeline", 1);
    let line_pipeline_4x = create_line_pipeline("lumark-line-pipeline-4x", 4);

    // 背景渐变全屏三角形管线。
    let background_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-background-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(BACKGROUND_SHADER)),
    });
    let background_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-background-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let background_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lumark-background-pipeline-layout"),
            bind_group_layouts: &[Some(&background_layout)],
            immediate_size: 0,
        });
    let create_background_pipeline = |label: &'static str, samples: u32| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&background_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &background_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &background_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let background_pipeline = create_background_pipeline("lumark-background", 1);
    let background_pipeline_4x = create_background_pipeline("lumark-background-4x", 4);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("lumark-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let white_view =
        create_solid_texture(device, queue, "lumark-white", [255, 255, 255, 255], false);
    let flat_normal_view = create_solid_texture(
        device,
        queue,
        "lumark-flat-normal",
        [128, 128, 255, 255],
        false,
    );
    let ssao_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-ssao-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let ssao_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("lumark-ssao-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let ssao_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-ssao-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_SSAO_SHADER)),
    });
    let ssao_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-ssao-pipeline-layout"),
        bind_group_layouts: &[Some(&ssao_layout)],
        immediate_size: 0,
    });
    let ssao_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-ssao-pipeline"),
        layout: Some(&ssao_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &ssao_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &ssao_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let shadow_camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-shadow-camera-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("lumark-shadow-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..Default::default()
    });
    let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-shadow-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_SHADOW_SHADER)),
    });
    let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-shadow-pipeline-layout"),
        bind_group_layouts: &[Some(&shadow_camera_layout), Some(&material_layout)],
        immediate_size: 0,
    });
    let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-shadow-pipeline"),
        layout: Some(&shadow_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shadow_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: (VERTEX_FLOATS * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attributes,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attributes,
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    });
    let fxaa_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-fxaa-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let fxaa_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("lumark-fxaa-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let fxaa_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-fxaa-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_FXAA_SHADER)),
    });
    let fxaa_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-fxaa-pipeline-layout"),
        bind_group_layouts: &[Some(&fxaa_layout)],
        immediate_size: 0,
    });
    let fxaa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-fxaa-pipeline"),
        layout: Some(&fxaa_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &fxaa_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fxaa_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let (sky_width, sky_height) = crate::sky_lut::size();
    let sky_pixels = crate::sky_lut::bake();
    let sky_lut = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lumark-sky-lut"),
        size: wgpu::Extent3d {
            width: sky_width,
            height: sky_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &sky_lut,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &sky_pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(sky_width * 4),
            rows_per_image: Some(sky_height),
        },
        wgpu::Extent3d {
            width: sky_width,
            height: sky_height,
            depth_or_array_layers: 1,
        },
    );
    let sky_lut_view = sky_lut.create_view(&wgpu::TextureViewDescriptor::default());
    let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("lumark-sky-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let ssao_blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-ssao-blur-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let ssao_blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-ssao-blur-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_SSAO_BLUR_SHADER)),
    });
    let ssao_blur_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lumark-ssao-blur-pipeline-layout"),
            bind_group_layouts: &[Some(&ssao_blur_layout)],
            immediate_size: 0,
        });
    let ssao_blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-ssao-blur-pipeline"),
        layout: Some(&ssao_blur_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &ssao_blur_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &ssao_blur_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let gbuffer_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-gbuffer-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_GBUFFER_SHADER)),
    });
    let gbuffer_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-gbuffer-pipeline-layout"),
        bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
        immediate_size: 0,
    });
    let gbuffer_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-gbuffer-pipeline"),
        layout: Some(&gbuffer_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &gbuffer_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: (VERTEX_FLOATS * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attributes,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attributes,
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &gbuffer_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
        }),
        multiview_mask: None,
        cache: None,
    });
    let deferred_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-deferred-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let deferred_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-deferred-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_DEFERRED_SHADER)),
    });
    let deferred_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-deferred-pipeline-layout"),
        bind_group_layouts: &[Some(&camera_layout), Some(&deferred_layout)],
        immediate_size: 0,
    });
    let deferred_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-deferred-pipeline"),
        layout: Some(&deferred_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &deferred_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &deferred_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let grid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumark-grid-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumark-grid-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SNOOPER_GRID_SHADER)),
    });
    let grid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumark-grid-pipeline-layout"),
        bind_group_layouts: &[Some(&grid_layout)],
        immediate_size: 0,
    });
    let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-grid-pipeline"),
        layout: Some(&grid_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &grid_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &grid_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
        }),
        multiview_mask: None,
        cache: None,
    });
    Ok(Resources {
        pipeline_single,
        pipeline_double,
        pipeline_single_4x,
        pipeline_double_4x,
        pipeline_blend_single,
        pipeline_blend_double,
        pipeline_blend_single_4x,
        pipeline_blend_double_4x,
        line_pipeline,
        line_pipeline_4x,
        background_pipeline,
        background_pipeline_4x,
        background_layout,
        camera_layout,
        material_layout,
        ssao_layout,
        ssao_pipeline,
        ssao_blur_layout,
        ssao_blur_pipeline,
        gbuffer_pipeline,
        deferred_layout,
        deferred_pipeline,
        grid_layout,
        grid_pipeline,
        ssao_sampler,
        shadow_camera_layout,
        shadow_pipeline,
        shadow_sampler,
        fxaa_layout,
        fxaa_pipeline,
        fxaa_sampler,
        sampler,
        sky_lut_texture: sky_lut,
        sky_lut_view,
        sky_sampler,
        white_view,
        flat_normal_view,
    })
}

/// 创建 1×1 纯色纹理视图。
fn create_solid_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    pixel: [u8; 4],
    srgb: bool,
) -> wgpu::TextureView {
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// 一个材质批次（同材质/同蒙皮姿态的三角形合并为一次绘制）。
struct Batch {
    vertices: Vec<f32>,
    count: u32,
    /// 场景材质索引；`None` 表示默认材质。
    material_index: Option<usize>,
    /// 关节最终蒙皮矩阵；未蒙皮时为单个单位矩阵。
    skin_matrices: Vec<[[f32; 4]; 4]>,
    /// 实例世界矩阵（列主序）；静态网格可复用同一顶点缓冲。
    instances: Vec<[[f32; 4]; 4]>,
    /// 批次中心（世界空间近似），用于半透明批次前后排序。
    sort_center: [f32; 3],
}

/// 列主序 4×4 矩阵变换一个点。
fn transform_point(matrix: &[[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    [
        matrix[0][0] * point[0] + matrix[1][0] * point[1] + matrix[2][0] * point[2] + matrix[3][0],
        matrix[0][1] * point[0] + matrix[1][1] * point[1] + matrix[2][1] * point[2] + matrix[3][1],
        matrix[0][2] * point[0] + matrix[1][2] * point[1] + matrix[2][2] * point[2] + matrix[3][2],
    ]
}

impl Batch {
    /// 返回批次材质；材质缺失时回退到默认材质。
    fn material<'a>(&self, scene: &'a Scene, fallback: &'a MaterialData) -> &'a MaterialData {
        self.material_index
            .and_then(|index| scene.materials.get(index))
            .unwrap_or(fallback)
    }
}

/// GPU 顶点。
#[derive(Debug, Clone, Copy)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    tangent: [f32; 4],
    joints: [f32; 4],
    weights: [f32; 4],
}

/// 把所有可见网格按材质/网格复用为实例批次。
///
/// 静态网格共享顶点缓冲，每个节点只追加实例矩阵；蒙皮网格姿态不同，
/// 每个节点独占批次，实例矩阵为单位矩阵。
fn build_batches(scene: &Scene, settings: &RenderSettings) -> Vec<Batch> {
    let mut batches: Vec<Batch> = Vec::new();
    let mut index_by_key: HashMap<(usize, usize, Option<usize>, Option<usize>), usize> =
        HashMap::new();
    for (node_index, node) in scene.nodes.iter().enumerate() {
        if !node.visible {
            continue;
        }
        let Some(mesh_handle) = node.mesh else {
            continue;
        };
        let Some(mesh) = scene.meshes.get(mesh_handle.0) else {
            continue;
        };
        let Some(world) = scene.world_transform(lumark_scene::SceneNodeId(node_index)) else {
            continue;
        };
        let matrices = skin::skin_matrices(scene, mesh);
        let skinned = matrices.is_some();
        let vertices = gpu_bind_vertices(mesh);
        if vertices.is_empty() {
            continue;
        }
        let instance = if skinned {
            skin::IDENTITY
        } else {
            world.matrix()
        };
        let ranges = if mesh.sections.is_empty() {
            vec![(0usize, mesh.indices.len(), 0usize)]
        } else {
            mesh.sections
                .iter()
                .map(|section| {
                    (
                        section.first_index as usize,
                        section.index_count as usize,
                        section.material_index,
                    )
                })
                .collect()
        };
        let snooper_extra = node
            .metadata
            .iter()
            .any(|(key, value)| key == "snooper_extra" && value == "true");
        for (section_index, (first, count, material_slot)) in ranges.into_iter().enumerate() {
            if !snooper_extra
                && (settings.hidden_material_slots.contains(&material_slot)
                    || settings.hidden_sections.contains(&section_index))
            {
                continue;
            }
            let material_handle = node
                .materials
                .get(material_slot)
                .copied()
                .flatten()
                .filter(|handle| handle.0 < scene.materials.len());
            let key = (
                mesh_handle.0,
                material_slot,
                material_handle.map(|handle| handle.0),
                skinned.then_some(node_index),
            );
            let existing = index_by_key.get(&key).copied();
            let batch_index = if let Some(index) = existing {
                batches[index].instances.push(instance);
                index
            } else {
                // 蒙皮批次独占该节点，可直接移动关节矩阵，避免重复克隆。
                let skin_matrices = matrices.clone().unwrap_or_else(|| vec![skin::IDENTITY]);
                let bounds_center = mesh.bounds.center();
                let sort_center = if skinned {
                    bounds_center
                } else {
                    transform_point(&instance, bounds_center)
                };
                batches.push(Batch {
                    vertices: Vec::new(),
                    count: 0,
                    material_index: material_handle.map(|handle| handle.0),
                    skin_matrices,
                    instances: vec![instance],
                    sort_center,
                });
                let index = batches.len() - 1;
                index_by_key.insert(key, index);
                index
            };
            if existing.is_some() && !skinned {
                continue;
            }
            let batch = &mut batches[batch_index];
            let end = (first + count).min(mesh.indices.len());
            let mut index = first;
            while index + 3 <= end {
                let indices = [
                    mesh.indices[index] as usize,
                    mesh.indices[index + 1] as usize,
                    mesh.indices[index + 2] as usize,
                ];
                if indices.iter().all(|index| *index < vertices.len()) {
                    let triangle = [
                        vertices[indices[0]],
                        vertices[indices[1]],
                        vertices[indices[2]],
                    ];
                    let face_normal = normalize(cross(
                        sub(triangle[1].position, triangle[0].position),
                        sub(triangle[2].position, triangle[0].position),
                    ));
                    for mut vertex in triangle {
                        if dot(vertex.normal, vertex.normal) <= 1.0e-8 {
                            vertex.normal = face_normal;
                        }
                        push_vertex(&mut batch.vertices, &vertex);
                        batch.count += 1;
                    }
                }
                index += 3;
            }
        }
    }
    batches.retain(|batch| batch.count > 0);
    batches
}

/// bind 空间顶点（带关节索引/权重），供 GPU 蒙皮使用。
fn gpu_bind_vertices(mesh: &MeshData) -> Vec<GpuVertex> {
    let skin = mesh.skin.as_ref();
    let mut vertices = Vec::with_capacity(mesh.positions.len());
    for (index, position) in mesh.positions.iter().enumerate() {
        let normal = mesh.normals.get(index).copied().unwrap_or([0.0, 0.0, 0.0]);
        let uv = mesh
            .uvs
            .first()
            .and_then(|channel| channel.get(index))
            .copied()
            .unwrap_or([0.0, 0.0]);
        let color = mesh
            .colors
            .get(index)
            .map(|color| {
                [
                    color[0] as f32 / 255.0,
                    color[1] as f32 / 255.0,
                    color[2] as f32 / 255.0,
                    color[3] as f32 / 255.0,
                ]
            })
            .unwrap_or([1.0; 4]);
        let tangent = mesh
            .tangents
            .get(index)
            .copied()
            .unwrap_or([1.0, 0.0, 0.0, 1.0]);
        let (joints, weights) = skin
            .filter(|skin| skin.is_valid())
            .map(|skin| {
                let joints = skin.joint_indices.get(index).copied().unwrap_or([0; 4]);
                let weights = skin.joint_weights.get(index).copied().unwrap_or([0.0; 4]);
                (
                    [
                        f32::from(joints[0]),
                        f32::from(joints[1]),
                        f32::from(joints[2]),
                        f32::from(joints[3]),
                    ],
                    weights,
                )
            })
            .unwrap_or(([0.0; 4], [0.0; 4]));
        vertices.push(GpuVertex {
            position: *position,
            normal,
            uv,
            color,
            tangent,
            joints,
            weights,
        });
    }
    vertices
}

/// 追加一个顶点的 24 个 f32。
fn push_vertex(values: &mut Vec<f32>, vertex: &GpuVertex) {
    values.extend_from_slice(&vertex.position);
    values.extend_from_slice(&vertex.normal);
    values.extend_from_slice(&vertex.uv);
    values.extend_from_slice(&vertex.color);
    values.extend_from_slice(&vertex.tangent);
    values.extend_from_slice(&vertex.joints);
    values.extend_from_slice(&vertex.weights);
}

/// 关节矩阵 storage 数据（每个关节 16 个 f32）。
fn skin_bytes(matrices: &[[[f32; 4]; 4]]) -> Vec<u8> {
    let mut values = Vec::with_capacity(matrices.len() * 16);
    for matrix in matrices {
        values.extend_from_slice(&skin::flatten(matrix));
    }
    f32_bytes(&values)
}

/// 叠加层线段顶点（pos3 + color4）与顶点数。
fn overlay_line_vertices(scene: &Scene, settings: &RenderSettings) -> (Vec<u8>, u32) {
    let mut values = Vec::new();
    let mut count = 0u32;
    let mut push_segment = |start: [f32; 3], end: [f32; 3], color: [f32; 4]| {
        for position in [start, end] {
            values.extend_from_slice(&position);
            values.extend_from_slice(&color);
        }
        count += 2;
    };
    if settings.show_grid {
        for (start, end) in crate::overlay::grid_lines(scene) {
            push_segment(start, end, [0.32, 0.35, 0.4, 1.0]);
        }
    }
    if settings.show_bones {
        for (start, end) in crate::overlay::bone_lines(scene) {
            push_segment(start, end, [1.0, 0.45, 0.08, 1.0]);
        }
    }
    if settings.wireframe {
        for (start, end) in crate::overlay::wireframe_lines(
            scene,
            &settings.hidden_material_slots,
            &settings.hidden_sections,
        ) {
            push_segment(start, end, [0.78, 0.84, 0.92, 1.0]);
        }
    }
    (f32_bytes(&values), count)
}

/// SSAO uniform（24 × f32）。
fn ssao_uniform(camera: &Camera, width: u32, height: u32) -> Vec<f32> {
    let mut values = Vec::with_capacity(24);
    for value in camera.eye {
        values.push(value);
    }
    values.push(0.0);
    for value in camera.forward {
        values.push(value);
    }
    values.push(0.0);
    for value in camera.right {
        values.push(value);
    }
    values.push(0.0);
    for value in camera.up {
        values.push(value);
    }
    values.push(0.0);
    values.extend_from_slice(&[camera.tan_half_fov, camera.aspect, camera.near, camera.far]);
    values.extend_from_slice(&[width as f32, height as f32, 100.0, 8000.0]);
    values
}

/// 相机 uniform（224 字节）。/// 相机 uniform（224 字节）。
fn camera_uniform(camera: &Camera, settings: &RenderSettings) -> Vec<u8> {
    let mut values = Vec::with_capacity(48);
    for column in &camera.view_projection {
        values.extend_from_slice(column);
    }
    values.extend_from_slice(&[
        camera.eye[0],
        camera.eye[1],
        camera.eye[2],
        if settings.vertex_colors { 1.0 } else { 0.0 },
    ]);
    values.extend_from_slice(&[camera.forward[0], camera.forward[1], camera.forward[2], 0.0]);
    let key = normalize(settings.key_direction);
    values.extend_from_slice(&[key[0], key[1], key[2], settings.key_intensity]);
    values.extend_from_slice(&[
        settings.key_color[0],
        settings.key_color[1],
        settings.key_color[2],
        0.0,
    ]);
    values.extend_from_slice(&[-key[0], -key[1], -key[2], settings.fill_intensity]);
    values.extend_from_slice(&[0.55, 0.62, 0.75, 0.0]);
    values.extend_from_slice(&[
        settings.ambient_strength,
        match settings.shading {
            crate::ShadingMode::Lit => 0.0,
            crate::ShadingMode::Unlit => 1.0,
            crate::ShadingMode::Normals => 2.0,
        },
        match settings.lighting_mode {
            LightingMode::Studio => 0.0,
            LightingMode::Headlight => 1.0,
            LightingMode::Snooper => 2.0,
        },
        match settings.normal_map_convention {
            NormalMapConvention::Ue => 0.0,
            NormalMapConvention::Fmodel => 1.0,
        },
    ]);
    values.extend_from_slice(&[
        settings.sky_upper_color[0],
        settings.sky_upper_color[1],
        settings.sky_upper_color[2],
        0.0,
    ]);
    values.extend_from_slice(&[
        settings.sky_lower_color[0],
        settings.sky_lower_color[1],
        settings.sky_lower_color[2],
        0.0,
    ]);
    values.extend_from_slice(&[
        settings.snooper.fog.color[0],
        settings.snooper.fog.color[1],
        settings.snooper.fog.color[2],
        if settings.snooper.fog.enabled {
            1.0
        } else {
            0.0
        },
    ]);
    values.extend_from_slice(&[
        settings.snooper.fog.density,
        settings.snooper.fog.height_falloff,
        settings.snooper.fog.max_opacity,
        settings.snooper.fog.start_distance,
    ]);
    for projection in &camera.shadow_view_projections {
        for column in projection {
            values.extend_from_slice(column);
        }
    }
    values.extend_from_slice(&[
        if settings.snooper.shadows.enabled {
            1.0
        } else {
            0.0
        },
        2048.0,
        settings.snooper.shadows.softness,
        settings.snooper.shadows.normal_offset,
    ]);
    values.extend_from_slice(&[
        camera.shadow_splits[0],
        camera.shadow_splits[1],
        camera.shadow_splits[2],
        camera.shadow_splits[3],
    ]);
    for params in &camera.shadow_cascade_params {
        values.extend_from_slice(params);
    }
    f32_bytes(&values)
}

/// 材质 uniform（112 字节）。
fn material_uniform(material: &MaterialData) -> Vec<u8> {
    let mut values = Vec::with_capacity(28);
    values.extend_from_slice(&[
        material.base_color[0],
        material.base_color[1],
        material.base_color[2],
        material.base_color[3],
    ]);
    values.extend_from_slice(&[
        material.metallic.clamp(0.0, 1.0),
        material.roughness.clamp(0.04, 1.0),
        if material.textures.base_color.is_some() {
            1.0
        } else {
            0.0
        },
        if material.textures.normal.is_some() {
            1.0
        } else {
            0.0
        },
    ]);
    values.extend_from_slice(&[
        material.emissive[0],
        material.emissive[1],
        material.emissive[2],
        if material.textures.emissive.is_some() {
            1.0
        } else {
            0.0
        },
    ]);
    values.extend_from_slice(&[
        if material.textures.metallic_roughness.is_some() {
            1.0
        } else {
            0.0
        },
        material.alpha_cutoff.clamp(0.0, 1.0),
        match material.orm_layout {
            OrmLayout::AoRoughMetal => 0.0,
            OrmLayout::SpecularMask => 1.0,
        },
        if material.textures.occlusion.is_some() {
            1.0
        } else {
            0.0
        },
    ]);
    // FModel 头灯：粗糙度区间、自发光倍率、AO 颜色增强。
    values.extend_from_slice(&[
        material.roughness_min.clamp(0.0, 1.0),
        material.roughness_max.clamp(0.0, 1.0),
        material.emissive_multiplier.max(0.0),
        if material.has_ao_boost { 1.0 } else { 0.0 },
    ]);
    values.extend_from_slice(&[
        material.ao_boost_color[0],
        material.ao_boost_color[1],
        material.ao_boost_color[2],
        material.ao_boost_exponent.max(0.0),
    ]);
    // Snooper 高光强度标量；y = 是否有巩膜贴图。
    values.extend_from_slice(&[
        material.specular.clamp(0.0, 1.0),
        if material.textures.sclera.is_some() {
            1.0
        } else {
            0.0
        },
        0.0,
        0.0,
    ]);
    f32_bytes(&values)
}

/// 线性背景色 → 清屏颜色（gamma 编码，与着色器输出同一空间）。
fn clear_color(background: [f32; 3], gamma_only: bool) -> wgpu::Color {
    let encode = |value: f32| {
        let positive = value.max(0.0);
        let mapped = if gamma_only {
            positive
        } else {
            positive / (1.0 + positive)
        };
        f64::from(mapped.powf(1.0 / 2.2))
    };
    wgpu::Color {
        r: encode(background[0]),
        g: encode(background[1]),
        b: encode(background[2]),
        a: 1.0,
    }
}

/// Snooper 模式 FXAA-lite：沿高对比边方向做一次受限邻域混合。
pub(crate) fn fxaa(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    if width < 3 || height < 3 || rgba.len() < (width * height * 4) as usize {
        return rgba.to_vec();
    }
    let sample = |x: i32, y: i32| {
        let cx = x.clamp(0, width as i32 - 1) as u32;
        let cy = y.clamp(0, height as i32 - 1) as u32;
        let offset = ((cy * width + cx) * 4) as usize;
        [
            rgba[offset] as f32 / 255.0,
            rgba[offset + 1] as f32 / 255.0,
            rgba[offset + 2] as f32 / 255.0,
        ]
    };
    let luma = |color: [f32; 3]| 0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2];
    let mut output = rgba.to_vec();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let xi = x as i32;
            let yi = y as i32;
            let nw = sample(xi - 1, yi - 1);
            let ne = sample(xi + 1, yi - 1);
            let sw = sample(xi - 1, yi + 1);
            let se = sample(xi + 1, yi + 1);
            let center = sample(xi, yi);
            let l_m = luma(center);
            let l_nw = luma(nw);
            let l_ne = luma(ne);
            let l_sw = luma(sw);
            let l_se = luma(se);
            let l_min = l_m.min(l_nw).min(l_ne).min(l_sw).min(l_se);
            let l_max = l_m.max(l_nw).max(l_ne).max(l_sw).max(l_se);
            let mut dir_x = -((l_nw + l_ne) - (l_sw + l_se));
            let mut dir_y = (l_nw + l_sw) - (l_ne + l_se);
            let reduce = ((l_nw + l_ne + l_sw + l_se) * (1.0 / 8.0 * 0.25)).max(1.0 / 128.0);
            let rcp = 1.0 / (dir_x.abs().min(dir_y.abs()) + reduce);
            dir_x = (dir_x * rcp).clamp(-8.0, 8.0);
            dir_y = (dir_y * rcp).clamp(-8.0, 8.0);
            let offset = ((y * width + x) * 4) as usize;
            let sample_offset = |factor: f32| {
                sample(
                    (xi as f32 + dir_x * factor).round() as i32,
                    (yi as f32 + dir_y * factor).round() as i32,
                )
            };
            let a0 = sample_offset(1.0 / 3.0 - 0.5);
            let a1 = sample_offset(2.0 / 3.0 - 0.5);
            let b0 = sample_offset(-0.5);
            let b1 = sample_offset(0.5);
            let mut rgb_a = [0.0f32; 3];
            let mut rgb_b = [0.0f32; 3];
            for channel in 0..3 {
                rgb_a[channel] = 0.5 * (a0[channel] + a1[channel]);
                rgb_b[channel] = rgb_a[channel] * 0.5 + 0.25 * (b0[channel] + b1[channel]);
            }
            let l_b = luma(rgb_b);
            let rgb = if l_b < l_min || l_b > l_max {
                rgb_a
            } else {
                rgb_b
            };
            for channel in 0..3 {
                output[offset + channel] = (rgb[channel].clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
    }
    output
}
/// 背景/天空 uniform：旧渐变 + Snooper 天空球/太阳。
fn background_uniform(settings: &RenderSettings, camera: &Camera) -> [f32; 44] {
    let background = settings.background;
    let sun = normalize(settings.key_direction);
    let mut values = [0.0f32; 44];
    let mut index = 0;
    let push4 = |value: [f32; 4], values: &mut [f32; 44], index: &mut usize| {
        values[*index..*index + 4].copy_from_slice(&value);
        *index += 4;
    };
    push4(
        [
            background[0] * 0.75,
            background[1] * 0.75,
            background[2] * 0.75,
            1.0,
        ],
        &mut values,
        &mut index,
    );
    push4(
        [
            background[0] * 1.25,
            background[1] * 1.25,
            background[2] * 1.25,
            1.0,
        ],
        &mut values,
        &mut index,
    );
    push4(
        [camera.eye[0], camera.eye[1], camera.eye[2], 0.0],
        &mut values,
        &mut index,
    );
    push4(
        [camera.forward[0], camera.forward[1], camera.forward[2], 0.0],
        &mut values,
        &mut index,
    );
    push4(
        [camera.right[0], camera.right[1], camera.right[2], 0.0],
        &mut values,
        &mut index,
    );
    push4(
        [camera.up[0], camera.up[1], camera.up[2], 0.0],
        &mut values,
        &mut index,
    );
    push4(
        [
            camera.tan_half_fov,
            camera.aspect,
            if settings.lighting_mode == LightingMode::Snooper {
                1.0
            } else {
                0.0
            },
            0.0,
        ],
        &mut values,
        &mut index,
    );
    push4([sun[0], sun[1], sun[2], 0.0], &mut values, &mut index);
    push4(
        [
            settings.key_color[0],
            settings.key_color[1],
            settings.key_color[2],
            0.0,
        ],
        &mut values,
        &mut index,
    );
    push4(
        [
            settings.sky_upper_color[0],
            settings.sky_upper_color[1],
            settings.sky_upper_color[2],
            0.0,
        ],
        &mut values,
        &mut index,
    );
    push4(
        [
            settings.sky_lower_color[0],
            settings.sky_lower_color[1],
            settings.sky_lower_color[2],
            0.0,
        ],
        &mut values,
        &mut index,
    );
    values
}

/// f32 序列转 little-endian 字节。
fn f32_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// 向量差。
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 三维点积。
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 三维叉积。
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_scene::{MeshData, MeshSection, NodeKind, SceneNode, Transform};

    /// FXAA 保持尺寸/alpha，并会改变高对比边缘像素。
    #[test]
    fn fxaa_softens_high_contrast_edge() {
        let width = 8;
        let height = 8;
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let inside = (2..6).contains(&x) && (2..6).contains(&y);
                let value = if inside { 255 } else { 0 };
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let output = fxaa(&rgba, width, height);
        assert_eq!(output.len(), rgba.len());
        assert_eq!(output[3], 255);
        assert_ne!(output, rgba, "边缘应被混合");
    }
    /// 相机 uniform 必须携带雾颜色/密度，且长度与 shader 结构一致。
    #[test]
    fn camera_uniform_includes_fog_settings() {
        let settings = RenderSettings::snooper_viewer(900, 600);
        let bounds = lumark_scene::Bounds::new([-1.0, -1.0, 0.0], [1.0, 1.0, 2.0]);
        let camera = Camera::fit(&bounds, &settings);
        let bytes = camera_uniform(&camera, &settings);
        assert_eq!(bytes.len(), CAMERA_UNIFORM_SIZE as usize);
        let value = |index: usize| {
            f32::from_le_bytes([
                bytes[index * 4],
                bytes[index * 4 + 1],
                bytes[index * 4 + 2],
                bytes[index * 4 + 3],
            ])
        };
        assert!((value(52) - settings.snooper.fog.color[0]).abs() < 1e-6);
        assert!((value(56) - settings.snooper.fog.density).abs() < 1e-6);
    }
    /// G-buffer shader 必须通过解析/语义校验。
    #[test]
    fn gbuffer_shader_parses_and_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(SNOOPER_GBUFFER_SHADER)
            .expect("解析 snooper_gbuffer.wgsl");
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("校验 snooper_gbuffer.wgsl");
    }

    /// 背景/天空 WGSL 也需要通过解析与语义校验。
    #[test]
    fn background_wgsl_shader_parses_and_validates() {
        let module =
            wgpu::naga::front::wgsl::parse_str(BACKGROUND_SHADER).expect("解析 background.wgsl");
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("校验 background.wgsl");
    }
    /// WGSL 语法/语义校验；不需要 GPU 适配器，防止着色器改动只在运行时暴露。
    #[test]
    fn wgsl_shader_parses_and_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("shader.wgsl"))
            .expect("解析 shader.wgsl");
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("校验 shader.wgsl");
    }

    /// 构造一个红色三角形场景。
    fn triangle_scene() -> Scene {
        let mut scene = Scene::new("Test");
        let material = scene.add_material(MaterialData {
            name: "Red".to_string(),
            base_color: [1.0, 0.0, 0.0, 1.0],
            ..MaterialData::default()
        });
        let mut mesh = MeshData::new(
            "Triangle",
            vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
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

    /// 线框模式只画边线，仍能产生非背景像素。
    #[test]
    fn gpu_wireframe_renders_edges_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU 线框测试：{error}");
                return;
            }
        };
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            background: [0.0; 3],
            wireframe: true,
            ..RenderSettings::default()
        };
        let target = renderer
            .render(&triangle_scene(), &settings)
            .expect("线框渲染");
        assert!(!target.is_uniform(), "线框未绘制边线");
    }

    /// 有 GPU 时渲染三角形；无 GPU 的 CI 环境跳过。
    #[test]
    fn gpu_renders_triangle_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU 渲染测试：{error}");
                return;
            }
        };
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        let target = renderer
            .render(&triangle_scene(), &settings)
            .expect("GPU 渲染");
        assert_eq!(target.width, 64);
        assert_eq!(target.height, 64);
        assert!(!target.is_uniform(), "GPU 未渲染出三角形");
        let mut red_pixels = 0;
        for pixel in target.rgba.as_chunks::<4>().0 {
            if pixel[0] > pixel[2].saturating_add(10) && pixel[0] > 10 {
                red_pixels += 1;
            }
        }
        assert!(red_pixels > 0, "GPU 渲染结果缺少红色像素");
    }

    /// 有 GPU 时以 4×MSAA 渲染三角形并 resolve 到单采样目标。
    #[test]
    fn gpu_msaa_renders_triangle_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU MSAA 测试：{error}");
                return;
            }
        };
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            msaa_samples: 4,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        let target = renderer
            .render(&triangle_scene(), &settings)
            .expect("GPU MSAA 渲染");
        assert!(!target.is_uniform(), "MSAA resolve 后没有三角形");
        let red_pixels = target
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > pixel[2].saturating_add(10) && pixel[0] > 10)
            .count();
        assert!(red_pixels > 0, "MSAA 渲染结果缺少红色像素");
    }

    /// 同一网格的两个节点应通过实例矩阵渲染到左右两半。
    #[test]
    fn gpu_instances_share_mesh_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU 实例化测试：{error}");
                return;
            }
        };
        let mut scene = triangle_scene();
        let mesh = scene.nodes[0].mesh.expect("网格");
        let material = scene.nodes[0].materials[0];
        scene.add_node(SceneNode {
            name: "Triangle2".to_string(),
            parent: None,
            transform: Transform::from_translation([4.0, 0.0, 0.0]),
            mesh: Some(mesh),
            materials: vec![material],
            visible: true,
            kind: NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        let settings = RenderSettings {
            width: 128,
            height: 64,
            supersample: 1,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        let target = renderer.render(&scene, &settings).expect("GPU 渲染");
        let mut left = 0;
        let mut right = 0;
        for (index, pixel) in target.rgba.as_chunks::<4>().0.iter().enumerate() {
            if pixel[0] > pixel[2].saturating_add(10) && pixel[0] > 10 {
                if index % 128 < 64 {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
        assert!(left > 0, "实例 1 未渲染");
        assert!(right > 0, "实例 2 未渲染");
    }

    /// 有 GPU 时验证蒙皮姿态平移；无 GPU 时跳过。
    #[test]
    fn gpu_skinned_pose_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU 蒙皮测试：{error}");
                return;
            }
        };
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        let scene = |offset: f32| {
            let mut scene = triangle_scene();
            let mesh = scene.meshes.first_mut().expect("网格");
            mesh.skin = Some(lumark_scene::MeshSkin {
                joints: vec![lumark_scene::SceneNodeId(0)],
                joint_indices: vec![[0; 4]; 3],
                joint_weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
                inverse_bind_matrices: vec![[
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ]],
                pose: Some(vec![[
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, offset, 0.0, 0.0,
                    1.0,
                ]]),
            });
            scene
        };
        let centroid = |offset: f32| {
            let target = renderer
                .render(&scene(offset), &settings)
                .expect("GPU 渲染");
            let mut sum = 0.0f32;
            let mut count = 0.0f32;
            for (index, pixel) in target.rgba.as_chunks::<4>().0.iter().enumerate() {
                if pixel[0] > pixel[2].saturating_add(10) {
                    sum += (index % 64) as f32;
                    count += 1.0;
                }
            }
            assert!(count > 0.0, "GPU 蒙皮三角形不可见");
            sum / count
        };
        let base = centroid(0.0);
        let shifted = centroid(1.5);
        assert!(
            (shifted - base).abs() > 2.0,
            "GPU 蒙皮平移未生效：{base} → {shifted}"
        );
    }

    /// 有 GPU 时验证网格叠加层与着色模式共存；无 GPU 时跳过。
    #[test]
    fn gpu_shading_and_grid_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU 着色模式测试：{error}");
                return;
            }
        };
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            background: [0.0; 3],
            key_intensity: 0.0,
            fill_intensity: 0.0,
            ambient_strength: 0.0,
            ..RenderSettings::default()
        };
        let mut scene = triangle_scene();
        scene.materials[0].base_color = [1.0, 0.0, 0.0, 1.0];
        if let Some(mesh) = scene.meshes.first_mut() {
            mesh.normals = vec![[0.0, 0.0, 1.0]; 3];
        }
        let unlit = renderer
            .render(
                &scene,
                &RenderSettings {
                    shading: crate::ShadingMode::Unlit,
                    ..settings.clone()
                },
            )
            .expect("GPU 渲染");
        assert!(
            unlit
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > pixel[2].saturating_add(10) && pixel[0] > 10),
            "GPU Unlit 模式未输出基础色"
        );
        let normals = renderer
            .render(
                &scene,
                &RenderSettings {
                    shading: crate::ShadingMode::Normals,
                    ..settings.clone()
                },
            )
            .expect("GPU 渲染");
        assert!(
            normals
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[2] > pixel[0].saturating_add(20)),
            "GPU Normals 模式未输出法线颜色"
        );

        let empty = Scene::new("Empty");
        let grid = renderer
            .render(
                &empty,
                &RenderSettings {
                    show_grid: true,
                    ..settings
                },
            )
            .expect("GPU 渲染");
        assert!(!grid.is_uniform(), "GPU 网格叠加层未生效");
    }
}
