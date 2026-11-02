//! wgpu 30 离屏渲染后端（V-2b）。
//!
//! 优先选择独立显卡（`PowerPreference::HighPerformance`），失败时回退默认适配器
//! 与软件适配器；渲染到离屏纹理后回读 RGBA8，供 GPUI `RenderImage` 或截图使用。
//! 着色模型与 CPU 参考渲染器一致（双方向光 + 环境光、基础色/法线贴图、超采样）。

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use lumark_scene::{MaterialData, MeshData, Scene};

use crate::camera::{Camera, normalize};
use crate::{RenderSettings, RenderTarget};

/// WGSL 着色器。
const SHADER: &str = include_str!("shader.wgsl");

/// 颜色目标格式（着色器手动 gamma 编码）。
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// 深度格式。
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// 每个顶点的 f32 数量：position3 + normal3 + uv2 + color4 + tangent4。
const VERTEX_FLOATS: usize = 16;
/// 相机 uniform 大小（字节）。
const CAMERA_UNIFORM_SIZE: u64 = 160;
/// 材质 uniform 大小（字节）。
const MATERIAL_UNIFORM_SIZE: u64 = 48;

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
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white_view: wgpu::TextureView,
    flat_normal_view: wgpu::TextureView,
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
                pipeline: resources.pipeline,
                camera_layout: resources.camera_layout,
                material_layout: resources.material_layout,
                sampler: resources.sampler,
                white_view: resources.white_view,
                flat_normal_view: resources.flat_normal_view,
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
        let width = settings.width * supersample;
        let height = settings.height * supersample;
        let bounds = scene.compute_bounds();
        let camera = Camera::fit(&bounds, settings);
        let batches = build_batches(scene);

        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumark-depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

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
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumark-render"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumark-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color(settings.background)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &camera_bind_group, &[]);
            for batch in &batches {
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
                    .write_buffer(&material_buffer, 0, &material_uniform(&batch.material));
                let base_upload = batch
                    .material
                    .textures
                    .base_color
                    .as_ref()
                    .and_then(|texture| self.upload_texture(texture, true));
                let normal_upload = batch
                    .material
                    .textures
                    .normal
                    .as_ref()
                    .and_then(|texture| self.upload_texture(texture, false));
                let base_view = base_upload.as_ref().unwrap_or(&self.white_view);
                let normal_view = normal_upload.as_ref().unwrap_or(&self.flat_normal_view);
                let material_bind_group =
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                        ],
                    });
                pass.set_bind_group(1, &material_bind_group, &[]);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.draw(0..batch.count, 0..1);
            }
        }

        // 回读：copy_texture_to_buffer 的每行字节数必须是 256 的倍数。
        let padded_row = ((width * 4).div_ceil(256)) * 256;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumark-readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
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
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| GpuError::new(format!("等待 GPU 完成失败：{error}")))?;
        let slice = readback.slice(..);
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
        readback.unmap();
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
        Some(gpu_texture.create_view(&wgpu::TextureViewDescriptor::default()))
    }
}

/// 共享管线资源。
struct Resources {
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
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
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
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
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumark-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: (VERTEX_FLOATS * 4) as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vertex_attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
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
            module: &shader,
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
    Ok(Resources {
        pipeline,
        camera_layout,
        material_layout,
        sampler,
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

/// 一个材质批次（同材质的三角形合并为一次绘制）。
struct Batch {
    vertices: Vec<f32>,
    count: u32,
    material: MaterialData,
}

/// GPU 顶点。
#[derive(Debug, Clone, Copy)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    tangent: [f32; 4],
}

/// 把所有可见网格按材质展开为世界空间三角形批次。
fn build_batches(scene: &Scene) -> Vec<Batch> {
    let default_material = MaterialData::default();
    let mut batches: Vec<Batch> = Vec::new();
    let mut index_by_key: HashMap<Option<usize>, usize> = HashMap::new();
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
        let vertices = gpu_world_vertices(mesh, &world);
        if vertices.is_empty() {
            continue;
        }
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
        for (first, count, material_slot) in ranges {
            let material_handle = node
                .materials
                .get(material_slot)
                .copied()
                .flatten()
                .filter(|handle| handle.0 < scene.materials.len());
            let key = material_handle.map(|handle| handle.0);
            let batch_index = *index_by_key.entry(key).or_insert_with(|| {
                let material = material_handle
                    .and_then(|handle| scene.materials.get(handle.0))
                    .cloned()
                    .unwrap_or_else(|| default_material.clone());
                batches.push(Batch {
                    vertices: Vec::new(),
                    count: 0,
                    material,
                });
                batches.len() - 1
            });
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

/// 把网格顶点变换到世界空间。
fn gpu_world_vertices(mesh: &MeshData, world: &lumark_scene::Transform) -> Vec<GpuVertex> {
    let mut vertices = Vec::with_capacity(mesh.positions.len());
    for (index, position) in mesh.positions.iter().enumerate() {
        let normal = mesh
            .normals
            .get(index)
            .map(|normal| normalize(world.transform_direction(*normal)))
            .unwrap_or([0.0; 3]);
        let tangent = mesh
            .tangents
            .get(index)
            .map(|tangent| {
                let direction =
                    normalize(world.transform_direction([tangent[0], tangent[1], tangent[2]]));
                [direction[0], direction[1], direction[2], tangent[3]]
            })
            .unwrap_or([1.0, 0.0, 0.0, 1.0]);
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
        vertices.push(GpuVertex {
            position: world.transform_point(*position),
            normal,
            uv,
            color,
            tangent,
        });
    }
    vertices
}

/// 追加一个顶点的 16 个 f32。
fn push_vertex(values: &mut Vec<f32>, vertex: &GpuVertex) {
    values.extend_from_slice(&vertex.position);
    values.extend_from_slice(&vertex.normal);
    values.extend_from_slice(&vertex.uv);
    values.extend_from_slice(&vertex.color);
    values.extend_from_slice(&vertex.tangent);
}

/// 相机 uniform（160 字节）。
fn camera_uniform(camera: &Camera, settings: &RenderSettings) -> Vec<u8> {
    let mut values = Vec::with_capacity(40);
    for column in &camera.view_projection {
        values.extend_from_slice(column);
    }
    values.extend_from_slice(&[camera.eye[0], camera.eye[1], camera.eye[2], 1.0]);
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
    values.extend_from_slice(&[settings.ambient_strength, 0.0, 0.0, 0.0]);
    f32_bytes(&values)
}

/// 材质 uniform（48 字节）。
fn material_uniform(material: &MaterialData) -> Vec<u8> {
    let mut values = Vec::with_capacity(12);
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
        0.0,
    ]);
    f32_bytes(&values)
}

/// 线性背景色 → 清屏颜色（gamma 编码，与着色器输出同一空间）。
fn clear_color(background: [f32; 3]) -> wgpu::Color {
    let encode = |value: f32| {
        let positive = value.max(0.0);
        let mapped = positive / (1.0 + positive);
        f64::from(mapped.powf(1.0 / 2.2))
    };
    wgpu::Color {
        r: encode(background[0]),
        g: encode(background[1]),
        b: encode(background[2]),
        a: 1.0,
    }
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

    /// 有 GPU 时渲染带基础色贴图的三角形；无 GPU 时跳过。
    #[test]
    fn gpu_samples_base_color_texture_or_skips() {
        let renderer = match GpuRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("跳过 GPU 贴图测试：{error}");
                return;
            }
        };
        let mut scene = triangle_scene();
        scene.materials[0].base_color = [1.0; 4];
        scene.materials[0].textures.base_color = Some(lumark_scene::TextureData {
            name: "T_Red".to_string(),
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        });
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            background: [0.0; 3],
            ..RenderSettings::default()
        };
        let target = renderer.render(&scene, &settings).expect("GPU 渲染");
        let mut red_pixels = 0;
        for pixel in target.rgba.as_chunks::<4>().0 {
            if pixel[0] > pixel[2].saturating_add(10) && pixel[0] > 10 {
                red_pixels += 1;
            }
        }
        assert!(red_pixels > 0, "GPU 基础色贴图未生效");
    }
}
