//! 引擎无关的三维渲染器。
//!
//! 当前提供确定性的 CPU 光栅化后端：适用于无 GPU 环境、单元测试和离屏截图。
//! 公共 API（[`RenderSettings`] / [`RenderTarget`] / [`Renderer`]）与后续 wgpu
//! 后端保持一致；GPU 后端渲染结果同样为 RGBA8，可转为 BGRA 交给 GPUI
//! `RenderImage`。
//!
//! 渲染约定：Z-up（与 Unreal 场景一致）、右手系、深度 0..1；场景 `unit_scale`
//! 由上层决定是否缩放，本模块直接使用场景坐标。

mod camera;
mod gpu;
mod overlay;
mod raster;
mod skin;
mod sky_lut;

pub use gpu::{GpuError, GpuRenderer};

use lumark_scene::{Bounds, Scene};

/// 计算渲染取景使用的场景包围盒。
///
/// 蒙皮网格按关节矩阵变换后的顶点统计；静态网格按节点世界变换统计。
/// 查看器/离屏导出用该包围盒拟合相机，避免蒙皮模型取景偏移。
pub fn render_bounds(scene: &Scene) -> Bounds {
    skin::render_bounds(scene)
}

/// Snooper 预览/查看器共享预设版本；修改环境/效果后递增，使详情缩略图缓存失效。
pub const SNOOPER_PRESET_VERSION: u32 = 3;
/// 场景渲染统计（V-20 状态/性能展示）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneStats {
    /// 可见网格实例数。
    pub mesh_instances: usize,
    /// 唯一网格数。
    pub unique_meshes: usize,
    /// 预计绘制批次（按可见 section 计）。
    pub draw_calls: usize,
    /// 顶点数（含实例重复）。
    pub vertices: usize,
    /// 三角形数（含实例重复）。
    pub triangles: usize,
    /// 含蒙皮的网格实例数。
    pub skinned_instances: usize,
}

/// 统计场景中可见网格的规模与预计绘制批次。
pub fn scene_stats(scene: &Scene) -> SceneStats {
    use std::collections::HashSet;
    let mut stats = SceneStats::default();
    let mut handles = HashSet::new();
    for node in &scene.nodes {
        if !node.visible {
            continue;
        }
        let Some(handle) = node.mesh else {
            continue;
        };
        let Some(mesh) = scene.meshes.get(handle.0) else {
            continue;
        };
        stats.mesh_instances += 1;
        handles.insert(handle.0);
        stats.vertices += mesh.positions.len();
        stats.triangles += mesh.indices.len() / 3;
        stats.draw_calls += mesh.sections.len().max(1);
        if mesh.skin.is_some() {
            stats.skinned_instances += 1;
        }
    }
    stats.unique_meshes = handles.len();
    stats
}

/// 着色模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadingMode {
    /// 近似 PBR 光照（默认）。
    #[default]
    Lit,
    /// 无光照基础色。
    Unlit,
    /// 世界空间法线可视化。
    Normals,
}

/// 光照模式。
///
/// `Studio` 是查看器最初的观感（双方向光 + 环境光），作为默认值保证既有
/// 渲染结果与测试不回归；`Headlight` 对齐 FModel Snooper `default.frag`：
/// 以视线方向为头灯，漫反射取 `max(N·V, 0.4)`，高光用 Schlick + GGX。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightingMode {
    /// 双方向光 + 环境光（默认）。
    #[default]
    Studio,
    /// 视线方向头灯（FModel 观感）。
    Headlight,
    /// Snooper 场景光照：方向光太阳 + 天空上下半球环境光，输出只做 gamma。
    Snooper,
}

/// 法线贴图的切线空间约定。
///
/// 两种约定只在副切线方向上不同：UE 网格切线使用 `cross(N, T) * sign(w)`，
/// 而 FModel Snooper 固定使用 `-cross(N, T)` 并忽略 `w`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NormalMapConvention {
    /// UE 网格切线约定（默认，保留既有渲染结果）。
    #[default]
    Ue,
    /// FModel Snooper 约定。
    Fmodel,
}

/// Snooper 默认环境（SunActor + EnvironmentActor）。
///
/// 原始强度/颜色保留 Snooper 数值；`resolve` 按 `SceneLighting.cs` 的
/// 1.25 亮度预算折算成着色器使用的太阳强度和天空上下半球色。
#[derive(Debug, Clone, PartialEq)]
pub struct SnooperEnvironmentSettings {
    /// 原始太阳强度（Snooper SunActor 默认 10）。
    pub sun_intensity: f32,
    /// 太阳颜色。
    pub sun_color: [f32; 3],
    /// 太阳角直径（度）。
    pub sun_angle_degrees: f32,
    /// 天空光上半球颜色（未乘强度）。
    pub sky_upper_color: [f32; 3],
    /// 天空光下半球颜色（未乘强度）。
    pub sky_lower_color: [f32; 3],
    /// 天空光强度（Snooper 默认 1.5）。
    pub sky_intensity: f32,
    /// 白天比例；1=白天，0=夜间。
    pub daylight: f32,
    /// 太阳/天空共享曝光预算。
    pub budget: f32,
}

impl Default for SnooperEnvironmentSettings {
    fn default() -> Self {
        Self {
            sun_intensity: 10.0,
            sun_color: [1.0, 0.96, 0.90],
            sun_angle_degrees: 0.5357,
            sky_upper_color: [0.6, 0.7, 0.8],
            sky_lower_color: [0.4, 0.35, 0.3],
            sky_intensity: 1.5,
            daylight: 1.0,
            budget: 1.25,
        }
    }
}

/// 解析后的 Snooper 环境。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedSnooperEnvironment {
    /// 着色器太阳强度。
    pub sun_intensity: f32,
    /// 天空上半球环境色。
    pub sky_upper_color: [f32; 3],
    /// 天空下半球环境色。
    pub sky_lower_color: [f32; 3],
}

impl SnooperEnvironmentSettings {
    /// 按 Snooper `SceneLighting.cs` 计算阳光/天空共享预算。
    pub fn resolve(&self) -> ResolvedSnooperEnvironment {
        const PI: f32 = std::f32::consts::PI;
        let luminance = |color: [f32; 3]| color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722;
        let upper = [
            self.sky_upper_color[0] * self.sky_intensity,
            self.sky_upper_color[1] * self.sky_intensity,
            self.sky_upper_color[2] * self.sky_intensity,
        ];
        let lower = [
            self.sky_lower_color[0] * self.sky_intensity,
            self.sky_lower_color[1] * self.sky_intensity,
            self.sky_lower_color[2] * self.sky_intensity,
        ];
        let daylight = self.daylight.clamp(0.0, 1.0);
        let from_sky = PI * luminance(upper);
        let from_sun = if daylight > 0.0 {
            self.sun_intensity / daylight * luminance(self.sun_color)
        } else {
            0.0
        };
        let sun_share = if from_sun + from_sky > 0.0 {
            from_sun / (from_sun + from_sky)
        } else {
            1.0
        };
        let sun_brightest = self.sun_color[0]
            .max(self.sun_color[1])
            .max(self.sun_color[2]);
        let sun_intensity = if sun_brightest > 0.0 {
            PI * self.budget * sun_share * daylight / sun_brightest
        } else {
            0.0
        };
        let sky_brightest = upper[0].max(upper[1]).max(upper[2]);
        let sky_scale = if sky_brightest > 0.0 {
            self.budget * (1.0 - sun_share) / sky_brightest
        } else {
            0.0
        };
        let night_upper = [0.080, 0.100, 0.145];
        let night_lower = [0.026, 0.030, 0.038];
        let mix = |night: [f32; 3], day: [f32; 3]| {
            [
                night[0] + (day[0] - night[0]) * daylight,
                night[1] + (day[1] - night[1]) * daylight,
                night[2] + (day[2] - night[2]) * daylight,
            ]
        };
        ResolvedSnooperEnvironment {
            sun_intensity,
            sky_upper_color: mix(
                night_upper,
                [
                    upper[0] * sky_scale,
                    upper[1] * sky_scale,
                    upper[2] * sky_scale,
                ],
            ),
            sky_lower_color: mix(
                night_lower,
                [
                    lower[0] * sky_scale,
                    lower[1] * sky_scale,
                    lower[2] * sky_scale,
                ],
            ),
        }
    }
}

/// 实时阴影设置（Snooper 默认 4 级联 / 2048²）。
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowSettings {
    pub enabled: bool,
    pub cascade_count: u32,
    pub resolution: u32,
    pub max_distance: f32,
    pub caster_distance: f32,
    pub lambda: f32,
    pub softness: f32,
    pub normal_offset: f32,
    pub blend: f32,
    pub slope_bias: f32,
    pub constant_bias: f32,
}

impl Default for ShadowSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            cascade_count: 4,
            resolution: 2048,
            max_distance: 150.0,
            caster_distance: 100.0,
            lambda: 0.85,
            softness: 1.0,
            normal_offset: 1.0,
            blend: 0.1,
            slope_bias: 2.0,
            constant_bias: 1.0,
        }
    }
}

/// SSAO 设置（Snooper 默认半径 1 / 强度 1 / 最大距离 80 / 模糊 2）。
#[derive(Debug, Clone, PartialEq)]
pub struct SsaoSettings {
    pub enabled: bool,
    pub radius: f32,
    pub intensity: f32,
    pub max_distance: f32,
    pub blur_radius: u32,
    pub downscale: u32,
}

impl Default for SsaoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            radius: 1.0,
            intensity: 1.0,
            max_distance: 80.0,
            blur_radius: 2,
            downscale: 2,
        }
    }
}

/// 程序化天空/大气参数（Snooper 默认地球）。
#[derive(Debug, Clone, PartialEq)]
pub struct SkySettings {
    pub enabled: bool,
    pub lut_width: u32,
    pub lut_height: u32,
    pub planet_radius: f32,
    pub atmosphere_height: f32,
    pub rayleigh_scattering: [f32; 3],
    pub rayleigh_scale_height: f32,
    pub mie_scattering: f32,
    pub mie_absorption: f32,
    pub mie_scale_height: f32,
    pub mie_anisotropy: f32,
    pub ozone_absorption: [f32; 3],
    pub luminance_factor: [f32; 3],
}

impl Default for SkySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            lut_width: 256,
            lut_height: 128,
            planet_radius: 6_360_000.0,
            atmosphere_height: 60_000.0,
            rayleigh_scattering: [5.802e-6, 13.558e-6, 33.1e-6],
            rayleigh_scale_height: 8_000.0,
            mie_scattering: 3.996e-6,
            mie_absorption: 0.444e-6,
            mie_scale_height: 1_200.0,
            mie_anisotropy: 0.9,
            ozone_absorption: [0.650e-6, 1.881e-6, 0.085e-6],
            luminance_factor: [1.0, 1.0, 1.0],
        }
    }
}

/// 指数高度雾（Snooper EnvironmentActor 默认）。
#[derive(Debug, Clone, PartialEq)]
pub struct FogSettings {
    pub enabled: bool,
    pub color: [f32; 3],
    pub density: f32,
    pub height_falloff: f32,
    pub max_opacity: f32,
    pub start_distance: f32,
    pub directional_color: [f32; 3],
    pub directional_exponent: f32,
    pub directional_start_distance: f32,
}

impl Default for FogSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0.447, 0.638, 1.0],
            density: 0.002,
            height_falloff: 0.02,
            max_opacity: 1.0,
            start_distance: 0.0,
            directional_color: [0.0; 3],
            directional_exponent: 4.0,
            directional_start_distance: 0.0,
        }
    }
}

/// 无限地面/网格外观。
#[derive(Debug, Clone, PartialEq)]
pub struct GroundSettings {
    pub enabled: bool,
    pub height: f32,
    pub cell_size: f32,
    pub lod_step: f32,
    pub minor_color: [f32; 3],
    pub major_color: [f32; 3],
    pub axis_x_color: [f32; 3],
    pub axis_z_color: [f32; 3],
    pub checker_a: [f32; 3],
    pub checker_b: [f32; 3],
    pub minor_opacity: f32,
    pub major_opacity: f32,
    pub fade_start: f32,
    pub fade_end: f32,
}

impl Default for GroundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            height: 0.0,
            cell_size: 1.0,
            lod_step: 10.0,
            minor_color: [0.35, 0.38, 0.42],
            major_color: [0.55, 0.58, 0.62],
            axis_x_color: [0.65, 0.32, 0.30],
            axis_z_color: [0.30, 0.45, 0.65],
            checker_a: [0.035, 0.04, 0.045],
            checker_b: [0.055, 0.06, 0.065],
            minor_opacity: 0.35,
            major_opacity: 0.55,
            fade_start: 0.05,
            fade_end: 0.75,
        }
    }
}

/// Snooper 后处理效果集合；默认关闭，查看器 preset 开启。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SnooperEffects {
    pub shadows: ShadowSettings,
    pub ssao: SsaoSettings,
    pub sky: SkySettings,
    pub fog: FogSettings,
    pub ground: GroundSettings,
    pub fxaa: bool,
}

impl SnooperEffects {
    /// 查看器/详情缩略图默认开启全套 Snooper 效果。
    pub fn enabled() -> Self {
        Self {
            shadows: ShadowSettings {
                enabled: true,
                ..ShadowSettings::default()
            },
            ssao: SsaoSettings {
                enabled: true,
                ..SsaoSettings::default()
            },
            sky: SkySettings {
                enabled: true,
                ..SkySettings::default()
            },
            fog: FogSettings {
                enabled: true,
                ..FogSettings::default()
            },
            ground: GroundSettings {
                enabled: true,
                ..GroundSettings::default()
            },
            fxaa: true,
        }
    }
}

/// 渲染设置。
#[derive(Debug, Clone)]
pub struct RenderSettings {
    /// 输出宽度（像素）。
    pub width: u32,
    /// 输出高度（像素）。
    pub height: u32,
    /// 背景色（线性 RGB）。
    pub background: [f32; 3],
    /// 垂直视场角（度）。
    pub fov_y_degrees: f32,
    /// 轨道相机水平角（度）。
    pub yaw_degrees: f32,
    /// 轨道相机俯仰角（度）。
    pub pitch_degrees: f32,
    /// 相机距离系数（相对自动拟合距离）。
    pub distance_scale: f32,
    /// 固定的取景包围盒；设置后相机不再随当前 LOD 变化。
    pub fit_bounds: Option<Bounds>,
    /// 屏幕平移使用的逻辑画布高度；0 表示使用当前渲染高度。
    pub logical_height: u32,
    /// 观察目标偏移。
    pub target_offset: [f32; 3],
    /// 屏幕空间平移像素：+x 向右、+y 向下；场景跟随鼠标方向移动。
    pub pan_pixels: [f32; 2],
    /// 主光方向（世界空间，指向光源）。
    pub key_direction: [f32; 3],
    /// 主光颜色（线性 RGB）。
    pub key_color: [f32; 3],
    /// 主光强度。
    pub key_intensity: f32,
    /// 补光强度（从相反方向照射）。
    pub fill_intensity: f32,
    /// 环境光强度。
    pub ambient_strength: f32,
    /// Snooper 天空上半球环境色（线性 RGB，仅 `Snooper` 模式使用）。
    pub sky_upper_color: [f32; 3],
    /// Snooper 天空下半球环境色（线性 RGB，仅 `Snooper` 模式使用）。
    pub sky_lower_color: [f32; 3],
    /// Snooper 原始环境参数；`snooper_viewer` 会解析成 key/sky 值。
    pub environment: SnooperEnvironmentSettings,
    /// Snooper 后处理效果集合（阴影/SSAO/天空/雾/地面/FXAA）。
    pub snooper: SnooperEffects,
    /// 超采样倍数（1–4）；提高画质并降低锯齿。
    pub supersample: u32,
    /// MSAA 采样数（1 或 4）；仅 GPU 后端使用。
    pub msaa_samples: u32,
    /// 基础色/自发光贴图是否按 sRGB 采样（关闭后按线性）。
    pub srgb_textures: bool,
    /// 是否把顶点色乘到基础色（FModel 默认关闭，查看器默认关闭）。
    pub vertex_colors: bool,
    /// 线框模式：只绘制三角形边。
    pub wireframe: bool,
    /// 着色模式。
    pub shading: ShadingMode,
    /// 光照模式（Studio 默认 / Headlight 对齐 FModel）。
    pub lighting_mode: LightingMode,
    /// 法线贴图切线空间约定（Ue 默认 / Fmodel）。
    pub normal_map_convention: NormalMapConvention,
    /// 是否显示地面网格。
    pub show_grid: bool,
    /// 是否显示骨骼线段。
    pub show_bones: bool,
    /// 隐藏的材质槽（section）索引。
    pub hidden_material_slots: Vec<usize>,
    /// 隐藏的 section 索引（按网格 section 顺序）。
    pub hidden_sections: Vec<usize>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            background: [0.025, 0.03, 0.04],
            // FModel 查看器默认：45° 垂直视场、约 1.05 倍拟合距离（模型占满画面）。
            fov_y_degrees: 45.0,
            yaw_degrees: 35.0,
            pitch_degrees: 20.0,
            distance_scale: 1.05,
            fit_bounds: None,
            logical_height: 0,
            target_offset: [0.0; 3],
            pan_pixels: [0.0; 2],
            key_direction: [0.4, -0.55, 0.75],
            key_color: [1.0, 0.97, 0.92],
            key_intensity: 1.15,
            fill_intensity: 0.35,
            ambient_strength: 0.22,
            sky_upper_color: [0.0; 3],
            sky_lower_color: [0.0; 3],
            environment: SnooperEnvironmentSettings::default(),
            snooper: SnooperEffects::default(),
            supersample: 2,
            msaa_samples: 1,
            srgb_textures: true,
            vertex_colors: true,
            wireframe: false,
            shading: ShadingMode::Lit,
            lighting_mode: LightingMode::Studio,
            normal_map_convention: NormalMapConvention::Ue,
            show_grid: false,
            show_bones: false,
            hidden_material_slots: Vec::new(),
            hidden_sections: Vec::new(),
        }
    }
}

impl RenderSettings {
    /// 查看器/详情预览统一的 Snooper 场景预设。
    ///
    /// 环境来自 Snooper `SunActor`/`EnvironmentActor`，太阳/天空强度按
    /// `SceneLighting.cs` 的共享曝光预算解析；全效果默认开启。
    pub fn snooper_viewer(width: u32, height: u32) -> Self {
        let environment = SnooperEnvironmentSettings::default();
        let resolved = environment.resolve();
        Self {
            width,
            height,
            background: [0.12, 0.24, 0.42],
            yaw_degrees: 35.0,
            pitch_degrees: 25.0,
            // Snooper 默认 SunActor 方向（UE Z-up，指向太阳）。
            key_direction: [0.766_044_4, 0.642_787_6, 0.0],
            key_color: environment.sun_color,
            key_intensity: resolved.sun_intensity,
            sky_upper_color: resolved.sky_upper_color,
            sky_lower_color: resolved.sky_lower_color,
            environment,
            snooper: SnooperEffects::enabled(),
            logical_height: 600,
            supersample: 1,
            msaa_samples: 1,
            srgb_textures: true,
            vertex_colors: false,
            lighting_mode: LightingMode::Snooper,
            normal_map_convention: NormalMapConvention::Fmodel,
            show_grid: true,
            ..Self::default()
        }
    }
}

/// 渲染错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    /// 宽度或高度为 0 或超过 4096。
    InvalidSize,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize => write!(formatter, "渲染尺寸必须在 1..=4096 之间"),
        }
    }
}

impl std::error::Error for RenderError {}

/// 渲染输出：RGBA8 帧缓冲。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTarget {
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// RGBA8 像素。
    pub rgba: Vec<u8>,
}

impl RenderTarget {
    /// 创建输出；尺寸与像素长度不一致时返回 `None`。
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        if rgba.len() >= width as usize * height as usize * 4 {
            Some(Self {
                width,
                height,
                rgba,
            })
        } else {
            None
        }
    }

    /// 取一个像素；越界返回 `None`。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba
            .get(offset..offset + 4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
    }

    /// 是否所有像素完全一致。
    pub fn is_uniform(&self) -> bool {
        let Some(first) = self.rgba.get(..4) else {
            return true;
        };
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel == first)
    }

    /// 转换为 BGRA8（GPUI `RenderImage`/GPU 回读常用格式）。
    pub fn to_bgra(&self) -> Vec<u8> {
        let mut bgra = Vec::with_capacity(self.rgba.len());
        for pixel in self.rgba.as_chunks::<4>().0 {
            bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        bgra
    }
}

/// 可复用的 CPU 渲染器。
#[derive(Debug, Clone)]
pub struct Renderer {
    settings: RenderSettings,
}

impl Renderer {
    /// 使用给定设置创建渲染器。
    pub fn new(settings: RenderSettings) -> Self {
        Self { settings }
    }

    /// 当前设置。
    pub fn settings(&self) -> &RenderSettings {
        &self.settings
    }

    /// 修改设置。
    pub fn settings_mut(&mut self) -> &mut RenderSettings {
        &mut self.settings
    }

    /// 渲染场景。
    pub fn render(&self, scene: &Scene) -> Result<RenderTarget, RenderError> {
        render_scene(scene, &self.settings)
    }
}

/// 使用一次性设置渲染场景。
pub fn render_scene(scene: &Scene, settings: &RenderSettings) -> Result<RenderTarget, RenderError> {
    if !(1..=4096).contains(&settings.width) || !(1..=4096).contains(&settings.height) {
        return Err(RenderError::InvalidSize);
    }
    let supersample = settings.supersample.clamp(1, 4);
    let width = settings.width * supersample;
    let height = settings.height * supersample;
    let bounds = render_bounds(scene);
    let camera = camera::Camera::fit(&bounds, settings);
    let rgba = raster::render(scene, settings, &camera, width, height);
    let rgba = if supersample > 1 {
        downsample(&rgba, width, height, settings.width, settings.height)
    } else {
        rgba
    };
    Ok(RenderTarget {
        width: settings.width,
        height: settings.height,
        rgba,
    })
}

/// 盒式滤波降采样。
pub(crate) fn downsample(
    source: &[u8],
    width: u32,
    height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    let mut output = vec![0u8; target_width as usize * target_height as usize * 4];
    let scale_x = width / target_width;
    let scale_y = height / target_height;
    let samples = (scale_x * scale_y).max(1);
    for y in 0..target_height {
        for x in 0..target_width {
            let mut sums = [0u32; 4];
            for offset_y in 0..scale_y {
                for offset_x in 0..scale_x {
                    let source_x = x * scale_x + offset_x;
                    let source_y = y * scale_y + offset_y;
                    let offset = (source_y as usize * width as usize + source_x as usize) * 4;
                    for channel in 0..4 {
                        sums[channel] += u32::from(source[offset + channel]);
                    }
                }
            }
            let target = (y as usize * target_width as usize + x as usize) * 4;
            for channel in 0..4 {
                output[target + channel] = (sums[channel] / samples) as u8;
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumark_scene::{MeshData, Scene, SceneNode, Transform};

    /// 场景统计包含实例/唯一网格/批次/顶点/三角形。
    #[test]
    fn counts_visible_scene_stats() {
        let mut scene = triangle_scene();
        let stats = scene_stats(&scene);
        assert_eq!(stats.mesh_instances, 1);
        assert_eq!(stats.unique_meshes, 1);
        assert_eq!(stats.draw_calls, 1);
        assert_eq!(stats.vertices, 3);
        assert_eq!(stats.triangles, 1);
        assert_eq!(stats.skinned_instances, 0);
        // 复用同一网格再添加一个实例。
        let handle = scene.nodes[0].mesh.unwrap();
        scene.add_node(SceneNode {
            name: "Tri2".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(handle),
            materials: Vec::new(),
            visible: true,
            kind: lumark_scene::NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        let stats = scene_stats(&scene);
        assert_eq!(stats.mesh_instances, 2);
        assert_eq!(stats.unique_meshes, 1);
        assert_eq!(stats.vertices, 6);
        scene.nodes[0].visible = false;
        scene.nodes[1].visible = false;
        assert_eq!(scene_stats(&scene), SceneStats::default());
    }

    /// 构造只含一个三角形的场景。
    fn triangle_scene() -> Scene {
        let mut scene = Scene::new("Test");
        let mesh = scene.add_mesh(MeshData::new(
            "Tri",
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 0.0]],
            vec![0, 1, 2],
        ));
        scene.add_node(SceneNode {
            name: "Tri".to_string(),
            parent: None,
            transform: Transform::IDENTITY,
            mesh: Some(mesh),
            materials: Vec::new(),
            visible: true,
            kind: lumark_scene::NodeKind::StaticMesh,
            metadata: Vec::new(),
        });
        scene
    }

    /// 空场景输出背景色。
    #[test]
    fn renders_background_for_empty_scene() {
        let scene = Scene::new("Empty");
        let settings = RenderSettings {
            width: 64,
            height: 64,
            supersample: 1,
            background: [0.1, 0.2, 0.3],
            ..RenderSettings::default()
        };
        let target = render_scene(&scene, &settings).expect("渲染");
        assert_eq!(target.width, 64);
        assert_eq!(target.rgba.len(), 64 * 64 * 4);
        let pixel = target.pixel(32, 32).expect("像素");
        assert_eq!(pixel[3], 255);
        assert!(pixel[0] < 120 && pixel[1] < 150 && pixel[2] < 180);
        // 背景是有意设计的上下渐变，这里只检查暗色与不透明度。
        let top = target.pixel(0, 0).expect("像素");
        assert_eq!(top[3], 255);
        assert!(top[0] < 120);
    }

    /// 三角形渲染后存在非背景像素。
    #[test]
    fn renders_triangle_pixels() {
        let scene = triangle_scene();
        let settings = RenderSettings {
            width: 96,
            height: 96,
            supersample: 1,
            ..RenderSettings::default()
        };
        let target = render_scene(&scene, &settings).expect("渲染");
        assert!(!target.is_uniform());
        assert_eq!(target.width, 96);
    }

    /// 尺寸非法时返回错误。
    #[test]
    fn rejects_invalid_size() {
        let scene = Scene::new("Empty");
        let settings = RenderSettings {
            width: 0,
            ..RenderSettings::default()
        };
        assert_eq!(
            render_scene(&scene, &settings),
            Err(RenderError::InvalidSize)
        );
    }

    /// BGRA 转换交换红蓝通道。
    #[test]
    fn converts_rgba_to_bgra() {
        let target = RenderTarget::new(1, 1, vec![10, 20, 30, 40]).expect("目标");
        assert_eq!(target.to_bgra(), vec![30, 20, 10, 40]);
        assert_eq!(target.pixel(0, 0), Some([10, 20, 30, 40]));
        assert_eq!(target.pixel(1, 0), None);
    }

    /// Snooper 默认环境按共享预算解析出约 2.94 太阳强度与天空上下色。
    #[test]
    fn snooper_default_environment_matches_snooper_budget() {
        let resolved = SnooperEnvironmentSettings::default().resolve();
        assert!((resolved.sun_intensity - 2.941).abs() < 0.05);
        assert!((resolved.sky_upper_color[0] - 0.235).abs() < 0.01);
        assert!((resolved.sky_upper_color[1] - 0.275).abs() < 0.01);
        assert!((resolved.sky_upper_color[2] - 0.314).abs() < 0.01);
        assert!((resolved.sky_lower_color[0] - 0.157).abs() < 0.01);
        assert!((resolved.sky_lower_color[1] - 0.137).abs() < 0.01);
        assert!((resolved.sky_lower_color[2] - 0.118).abs() < 0.01);
    }
    /// 查看器与详情缩略图共享 Snooper 光照/背景预设。
    #[test]
    fn snooper_viewer_preset_is_shared_by_viewer_and_details() {
        let settings = RenderSettings::snooper_viewer(480, 360);
        assert_eq!(settings.width, 480);
        assert_eq!(settings.height, 360);
        assert_eq!(settings.lighting_mode, LightingMode::Snooper);
        assert_eq!(settings.normal_map_convention, NormalMapConvention::Fmodel);
        assert_eq!(settings.background, [0.12, 0.24, 0.42]);
        let resolved = settings.environment.resolve();
        assert_eq!(settings.key_intensity, resolved.sun_intensity);
        assert_eq!(settings.sky_upper_color, resolved.sky_upper_color);
        assert_eq!(settings.sky_lower_color, resolved.sky_lower_color);
        assert_eq!(settings.snooper, SnooperEffects::enabled());
        assert!(settings.show_grid);
    }
}
