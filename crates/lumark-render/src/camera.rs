//! 相机、视图/投影矩阵与裁剪空间工具。

use lumark_scene::Bounds;

use crate::RenderSettings;

/// 列主序 4×4 矩阵。
pub(crate) type Mat4 = [[f32; 4]; 4];

/// 渲染相机。
pub(crate) struct Camera {
    /// 视图投影矩阵。
    pub view_projection: Mat4,
    /// 相机世界位置。
    pub eye: [f32; 3],
}

impl Camera {
    /// 根据场景包围盒和轨道参数拟合相机。
    pub fn fit(bounds: &Bounds, settings: &RenderSettings) -> Self {
        let center = if bounds.is_empty() {
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
        let eye = [
            center[0] + direction[0] * distance,
            center[1] + direction[1] * distance,
            center[2] + direction[2] * distance,
        ];
        let up = [0.0, 0.0, 1.0];
        let near = (distance * 0.01).max(0.01);
        let far = (distance + radius * 8.0).max(near + 1.0);
        let view = look_at(eye, center, up);
        let aspect = settings.width.max(1) as f32 / settings.height.max(1) as f32;
        let projection = perspective(fov_y, aspect, near, far);
        Self {
            view_projection: multiply(&projection, &view),
            eye,
        }
    }

    /// 把世界点变换到裁剪空间。
    pub fn clip_position(&self, position: [f32; 3]) -> [f32; 4] {
        transform_point4(&self.view_projection, position)
    }
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
}
