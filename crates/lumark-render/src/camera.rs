//! 相机、视图/投影矩阵与裁剪空间工具。

use lumark_scene::Bounds;

use crate::RenderSettings;

/// 列主序 4×4 矩阵。
pub(crate) type Mat4 = [[f32; 4]; 4];

/// 渲染相机（后处理/阴影还需基向量与近远面）。
#[allow(dead_code)]
pub(crate) struct Camera {
    /// 视图投影矩阵。
    pub view_projection: Mat4,
    /// 相机世界位置。
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub tan_half_fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub shadow_view_projections: [Mat4; 4],
    pub shadow_splits: [f32; 4],
    pub shadow_cascade_params: [[f32; 4]; 4],
}

impl Camera {
    /// 根据场景包围盒和轨道参数拟合相机。
    ///
    /// `pan_pixels` 以输出画布像素为单位、按屏幕像素 1:1 换算世界偏移：
    /// 向右拖拽时观察目标左移（场景跟手右移），向下拖拽时目标上移
    /// （场景跟手下移）。偏移在 `target_offset` 之后叠加。
    pub fn fit(bounds: &Bounds, settings: &RenderSettings) -> Self {
        let bounds = settings
            .fit_bounds
            .as_ref()
            .filter(|bounds| !bounds.is_empty())
            .unwrap_or(bounds);
        let base_center = if bounds.is_empty() {
            settings.target_offset
        } else {
            let center = bounds.center();
            [
                center[0] + settings.target_offset[0],
                center[1] + settings.target_offset[1],
                center[2] + settings.target_offset[2],
            ]
        };
        let radius = if bounds.is_empty() {
            100.0
        } else {
            (bounds.extent() * 0.5).max(1.0e-3)
        };
        let fov_y = settings.fov_y_degrees.to_radians().clamp(0.05, 3.0);
        let distance = (radius / (fov_y * 0.5).tan()) * settings.distance_scale;
        let yaw = settings.yaw_degrees.to_radians();
        let pitch = settings.pitch_degrees.to_radians().clamp(-1.5, 1.5);
        let direction = [
            pitch.cos() * yaw.cos(),
            pitch.cos() * yaw.sin(),
            pitch.sin(),
        ];
        // 相机屏幕右/上基，与 `look_at` 的 side/up 保持一致。
        let forward = [-direction[0], -direction[1], -direction[2]];
        let right = normalize(cross(forward, [0.0, 0.0, 1.0]));
        let up = normalize(cross(right, forward));
        let logical_height = if settings.logical_height == 0 {
            settings.height.max(1)
        } else {
            settings.logical_height.max(1)
        };
        let world_per_pixel = 2.0 * distance * (fov_y * 0.5).tan() / logical_height as f32;
        let center = [
            base_center[0] - right[0] * settings.pan_pixels[0] * world_per_pixel
                + up[0] * settings.pan_pixels[1] * world_per_pixel,
            base_center[1] - right[1] * settings.pan_pixels[0] * world_per_pixel
                + up[1] * settings.pan_pixels[1] * world_per_pixel,
            base_center[2] - right[2] * settings.pan_pixels[0] * world_per_pixel
                + up[2] * settings.pan_pixels[1] * world_per_pixel,
        ];
        let eye = [
            center[0] + direction[0] * distance,
            center[1] + direction[1] * distance,
            center[2] + direction[2] * distance,
        ];
        let up_world = [0.0, 0.0, 1.0];
        let near = (distance * 0.01).max(0.01);
        let far = (distance + radius * 100.0).max(near + 1.0);
        let view = look_at(eye, center, up_world);
        let aspect = settings.width.max(1) as f32 / settings.height.max(1) as f32;
        let projection = perspective(fov_y, aspect, near, far);
        let cascades = build_shadow_cascades(
            eye,
            forward,
            up,
            (fov_y * 0.5).tan(),
            aspect,
            near,
            far,
            settings,
        );
        Self {
            view_projection: multiply(&projection, &view),
            eye,
            forward,
            right,
            up,
            tan_half_fov: (fov_y * 0.5).tan(),
            aspect,
            near,
            far,
            shadow_view_projections: cascades.projections,
            shadow_splits: cascades.splits,
            shadow_cascade_params: cascades.params,
        }
    }

    /// 把世界点变换到裁剪空间。
    pub fn clip_position(&self, position: [f32; 3]) -> [f32; 4] {
        transform_point4(&self.view_projection, position)
    }
}

/// 覆盖场景包围盒的太阳阴影视图投影（正交，深度 0..1）。
#[allow(dead_code)]
pub(crate) fn shadow_view_projection(bounds: &Bounds, light_direction: [f32; 3]) -> Mat4 {
    shadow_view_projection_scaled(bounds, light_direction, 1.0)
}

/// 按级联范围缩放的正交阴影视图投影。
#[allow(dead_code)]
pub(crate) fn shadow_view_projection_scaled(
    bounds: &Bounds,
    light_direction: [f32; 3],
    extent_scale: f32,
) -> Mat4 {
    let center = if bounds.is_empty() {
        [0.0; 3]
    } else {
        bounds.center()
    };
    let radius = if bounds.is_empty() {
        100.0
    } else {
        (bounds.extent() * 0.5).max(1.0)
    } * extent_scale.max(0.1);
    let light = normalize(light_direction);
    let up = if dot(light, [0.0, 0.0, 1.0]).abs() > 0.99 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let distance = radius * 2.0 + 10.0;
    let eye = [
        center[0] + light[0] * distance,
        center[1] + light[1] * distance,
        center[2] + light[2] * distance,
    ];
    let view = look_at(eye, center, up);
    let extent = radius * 1.35;
    let projection = orthographic(
        -extent,
        extent,
        -extent,
        extent,
        0.05,
        distance + radius * 2.0,
    );
    multiply(&projection, &view)
}

struct ShadowCascades {
    projections: [Mat4; 4],
    splits: [f32; 4],
    params: [[f32; 4]; 4],
}

/// 按 Snooper `SunCascades` 的球拟合/texel 对齐构造 4 级联。
#[allow(clippy::too_many_arguments)]
fn build_shadow_cascades(
    eye: [f32; 3],
    forward: [f32; 3],
    _up: [f32; 3],
    tan_half_fov: f32,
    aspect: f32,
    near: f32,
    far: f32,
    settings: &RenderSettings,
) -> ShadowCascades {
    let shadows = &settings.snooper.shadows;
    let max_distance = (shadows.max_distance * 100.0).max(near + 1.0);
    let caster_distance = (shadows.caster_distance * 100.0).max(1.0);
    let far = far.min(max_distance);
    let light = normalize(settings.key_direction);
    let up_world = if dot(light, [0.0, 0.0, 1.0]).abs() > 0.99 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let light_forward = [-light[0], -light[1], -light[2]];
    let light_right = normalize(cross(light_forward, up_world));
    let light_up = cross(light_right, light_forward);
    let k2 = tan_half_fov * tan_half_fov * (1.0 + aspect * aspect);
    let mut projections = [[[0.0; 4]; 4]; 4];
    let mut splits = [0.0; 4];
    let mut params = [[0.0; 4]; 4];
    for index in 0..4 {
        let p = (index + 1) as f32 / 4.0;
        let near_i = if index == 0 { near } else { splits[index - 1] };
        let logarithmic = near * (far / near).powf(p);
        let linear = near + (far - near) * p;
        let split = linear + (logarithmic - linear) * shadows.lambda;
        splits[index] = split;
        let near_i = near_i.max(0.01);
        let far_i = split.max(near_i + 1.0);
        let (distance, radius) = if k2 * (far_i + near_i) >= far_i - near_i {
            (far_i, far_i * k2.sqrt())
        } else {
            let distance = 0.5 * (far_i + near_i) * (1.0 + k2);
            let radius = 0.5
                * ((far_i - near_i) * (far_i - near_i)
                    + 2.0 * (far_i * far_i + near_i * near_i) * k2
                    + (far_i + near_i) * (far_i + near_i) * k2 * k2)
                    .sqrt();
            (distance, radius)
        };
        let radius = (radius * 16.0).ceil() / 16.0;
        let mut center = [
            eye[0] + forward[0] * distance,
            eye[1] + forward[1] * distance,
            eye[2] + forward[2] * distance,
        ];
        let texel = radius * 2.0 / shadows.resolution.max(1) as f32;
        let x = (dot(center, light_right) / texel).floor() * texel;
        let y = (dot(center, light_up) / texel).floor() * texel;
        let z = dot(center, light_forward);
        center = [
            light_right[0] * x + light_up[0] * y + light_forward[0] * z,
            light_right[1] * x + light_up[1] * y + light_forward[1] * z,
            light_right[2] * x + light_up[2] * y + light_forward[2] * z,
        ];
        let depth_range = radius * 2.0 + caster_distance;
        let light_eye = [
            center[0] + light[0] * (radius + caster_distance),
            center[1] + light[1] * (radius + caster_distance),
            center[2] + light[2] * (radius + caster_distance),
        ];
        let view = look_at(light_eye, center, up_world);
        let projection = orthographic(-radius, radius, -radius, radius, 0.0, depth_range);
        projections[index] = multiply(&projection, &view);
        params[index] = [texel, 1.0 / depth_range, split, near_i];
    }
    ShadowCascades {
        projections,
        splits,
        params,
    }
}
/// 正交投影，深度映射到 0..1（wgpu）。
pub(crate) fn orthographic(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
) -> Mat4 {
    [
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, -1.0 / (far - near), 0.0],
        [
            -(right + left) / (right - left),
            -(top + bottom) / (top - bottom),
            -near / (far - near),
            1.0,
        ],
    ]
}
/// 由观察方向构造视图矩阵（右手系，Z 向上）。
pub(crate) fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let forward = normalize(sub(target, eye));
    let side = normalize(cross(forward, up));
    let up = cross(side, forward);
    [
        [side[0], up[0], -forward[0], 0.0],
        [side[1], up[1], -forward[1], 0.0],
        [side[2], up[2], -forward[2], 0.0],
        [-dot(side, eye), -dot(up, eye), dot(forward, eye), 1.0],
    ]
}

/// 右手系透视投影，深度映射到 0..1（与 wgpu 一致）。
pub(crate) fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    [
        [f / aspect.max(1.0e-4), 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

/// 矩阵乘法 `a * b`。
pub(crate) fn multiply(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut result = [[0.0f32; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            let mut value = 0.0f32;
            for index in 0..4 {
                value += a[index][row] * b[column][index];
            }
            result[column][row] = value;
        }
    }
    result
}

/// 矩阵乘 4 维向量。
pub(crate) fn transform_point4(matrix: &Mat4, point: [f32; 3]) -> [f32; 4] {
    let vector = [point[0], point[1], point[2], 1.0];
    let mut result = [0.0f32; 4];
    for row in 0..4 {
        result[row] = matrix[0][row] * vector[0]
            + matrix[1][row] * vector[1]
            + matrix[2][row] * vector[2]
            + matrix[3][row] * vector[3];
    }
    result
}

/// 向量差。
pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 三维点积。
pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 三维叉积。
pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 归一化向量；零向量回退到 +Z。
pub(crate) fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, value[1] / length, value[2] / length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认设置下包围盒中心投影到屏幕中心。
    #[test]
    fn camera_targets_bounds_center() {
        let settings = RenderSettings {
            width: 256,
            height: 256,
            ..RenderSettings::default()
        };
        let bounds = Bounds::new([-50.0, -50.0, 0.0], [50.0, 50.0, 100.0]);
        let camera = Camera::fit(&bounds, &settings);
        let clip = camera.clip_position(bounds.center());
        assert!(clip[3] > 0.0);
        assert!((clip[0] / clip[3]).abs() < 1e-4);
        assert!((clip[1] / clip[3]).abs() < 1e-4);
    }

    /// 阴影视图投影必须覆盖场景中心。
    #[test]
    fn shadow_projection_covers_scene_center() {
        let bounds = Bounds::new([-80.0, -30.0, 0.0], [80.0, 30.0, 170.0]);
        let matrix = shadow_view_projection(&bounds, [0.766, 0.643, 0.0]);
        let clip = transform_point4(&matrix, bounds.center());
        assert!(clip[3] > 0.0);
        let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
        assert!(
            (-1.0..=1.0).contains(&ndc[0]) && (-1.0..=1.0).contains(&ndc[1]),
            "阴影 NDC 应在范围内：{ndc:?}"
        );
        assert!((0.0..=1.0).contains(&ndc[2]), "阴影深度应在 0..1：{ndc:?}");
    }
    /// Snooper 默认相机：眼睛在地面上方，屏幕底部射线向下，能命中地面。
    #[test]
    fn default_camera_ray_can_hit_ground() {
        let bounds = Bounds::new([-0.8, -0.3, 0.0], [0.8, 0.3, 1.7]);
        let settings = RenderSettings::snooper_viewer(900, 600);
        let camera = Camera::fit(&bounds, &settings);
        assert!(camera.eye[2] > 0.0, "相机应在地面上方：{:?}", camera.eye);
        let bottom = [
            camera.forward[0] - camera.up[0] * camera.tan_half_fov,
            camera.forward[1] - camera.up[1] * camera.tan_half_fov,
            camera.forward[2] - camera.up[2] * camera.tan_half_fov,
        ];
        assert!(bottom[2] < 0.0, "屏幕底部射线应向下：{:?}", bottom);
        let hit = -camera.eye[2] / bottom[2];
        assert!(hit > 0.0, "地面交点应在相机前方：hit={hit}");
    }
    /// 透视投影把近/远裁面映射到 0/1。
    #[test]
    fn perspective_maps_depth() {
        let projection = perspective(45.0_f32.to_radians(), 1.0, 0.1, 100.0);
        let near = transform_point4(&projection, [0.0, 0.0, -0.1]);
        let far = transform_point4(&projection, [0.0, 0.0, -100.0]);
        assert!((near[2] / near[3]).abs() < 1e-4);
        assert!((far[2] / far[3] - 1.0).abs() < 1e-4);
    }

    /// 空场景使用固定距离并看向原点。
    #[test]
    fn empty_bounds_use_defaults() {
        let settings = RenderSettings::default();
        let camera = Camera::fit(&Bounds::EMPTY, &settings);
        let clip = camera.clip_position([0.0; 3]);
        assert!(clip[3] > 0.0);
        assert!((clip[0] / clip[3]).abs() < 1e-4);
        assert!((clip[1] / clip[3]).abs() < 1e-4);
    }

    /// 平移像素与投影屏幕位移 1:1：向右拖拽场景右移、向下拖拽场景下移。
    #[test]
    fn camera_pan_moves_target_by_screen_pixels() {
        let bounds = Bounds::new([-50.0, -50.0, 0.0], [50.0, 50.0, 100.0]);
        let settings = RenderSettings {
            width: 320,
            height: 240,
            pan_pixels: [32.0, 16.0],
            ..RenderSettings::default()
        };
        let camera = Camera::fit(&bounds, &settings);
        let clip = camera.clip_position(bounds.center());
        assert!(clip[3] > 0.0);
        let ndc_x = clip[0] / clip[3];
        let ndc_y = clip[1] / clip[3];
        assert!(
            (ndc_x - 2.0 * 32.0 / 320.0).abs() < 1e-3,
            "向右拖拽后场景应右移：ndc_x={ndc_x}"
        );
        assert!(
            (ndc_y + 2.0 * 16.0 / 240.0).abs() < 1e-3,
            "向下拖拽后场景应下移：ndc_y={ndc_y}"
        );
    }

    /// 内部渲染分辨率变化时，同一 pan_pixels 产生同一相机，避免静止后跳变。
    #[test]
    fn camera_pan_is_independent_of_internal_render_size() {
        let bounds = Bounds::new([-50.0, -50.0, 0.0], [50.0, 50.0, 100.0]);
        let base = RenderSettings {
            width: 900,
            height: 600,
            logical_height: 600,
            pan_pixels: [48.0, -24.0],
            ..RenderSettings::default()
        };
        let low = RenderSettings {
            width: 675,
            height: 450,
            ..base.clone()
        };
        let high_camera = Camera::fit(&bounds, &base);
        let low_camera = Camera::fit(&bounds, &low);
        for axis in 0..3 {
            assert!(
                (high_camera.eye[axis] - low_camera.eye[axis]).abs() < 1e-4,
                "质量切换后相机位置应一致：axis={axis} high={:?} low={:?}",
                high_camera.eye,
                low_camera.eye
            );
        }
    }

    /// fit_bounds 覆盖当前 LOD 包围盒，LOD 切换不改变相机取景。
    #[test]
    fn fit_bounds_override_is_stable_across_lods() {
        let reference = Bounds::new([-10.0, -10.0, 0.0], [10.0, 10.0, 20.0]);
        let other_lod = Bounds::new([-2.0, -2.0, 0.0], [2.0, 2.0, 2.0]);
        let settings = RenderSettings {
            fit_bounds: Some(reference),
            ..RenderSettings::default()
        };
        let camera = Camera::fit(&other_lod, &settings);
        let clip = camera.clip_position(reference.center());
        assert!(clip[3] > 0.0);
        assert!((clip[0] / clip[3]).abs() < 1e-4);
        assert!((clip[1] / clip[3]).abs() < 1e-4);
    }
}
