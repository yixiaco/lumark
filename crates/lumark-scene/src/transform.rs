//! TRS 变换与四元数工具。
//!
//! 所有矩阵均为列主序 `[[f32; 4]; 4]`，与 wgpu/OpenGL 一致；点变换使用
//! `matrix * point` 约定。

/// 平移、旋转、缩放组成的局部变换。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// 平移。
    pub translation: [f32; 3],
    /// 旋转四元数（xyzw）。
    pub rotation: [f32; 4],
    /// 缩放。
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// 单位变换。
    pub const IDENTITY: Self = Self {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };

    /// 仅平移。
    pub fn from_translation(translation: [f32; 3]) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// 仅旋转。
    pub fn from_rotation(rotation: [f32; 4]) -> Self {
        Self {
            rotation: normalize_quat(rotation),
            ..Self::IDENTITY
        }
    }

    /// 绕单位轴旋转 `angle_radians`。
    pub fn from_axis_angle(axis: [f32; 3], angle_radians: f32) -> Self {
        let length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        if length <= f32::EPSILON {
            return Self::IDENTITY;
        }
        let half = angle_radians * 0.5;
        let sin = half.sin();
        Self::from_rotation([
            axis[0] / length * sin,
            axis[1] / length * sin,
            axis[2] / length * sin,
            half.cos(),
        ])
    }

    /// 仅缩放。
    pub fn from_scale(scale: [f32; 3]) -> Self {
        Self {
            scale,
            ..Self::IDENTITY
        }
    }

    /// 转为列主序 4×4 矩阵。
    pub fn matrix(&self) -> [[f32; 4]; 4] {
        let [x, y, z, w] = normalize_quat(self.rotation);
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        let wx = w * x;
        let wy = w * y;
        let wz = w * z;
        let [sx, sy, sz] = self.scale;
        let [tx, ty, tz] = self.translation;
        [
            [
                (1.0 - 2.0 * (yy + zz)) * sx,
                2.0 * (xy + wz) * sx,
                2.0 * (xz - wy) * sx,
                0.0,
            ],
            [
                2.0 * (xy - wz) * sy,
                (1.0 - 2.0 * (xx + zz)) * sy,
                2.0 * (yz + wx) * sy,
                0.0,
            ],
            [
                2.0 * (xz + wy) * sz,
                2.0 * (yz - wx) * sz,
                (1.0 - 2.0 * (xx + yy)) * sz,
                0.0,
            ],
            [tx, ty, tz, 1.0],
        ]
    }

    /// 组合变换：返回先应用 `self`、再应用父级 `parent` 的世界变换。
    pub fn combine(parent: &Transform, local: &Transform) -> Transform {
        let rotation = quat_mul(parent.rotation, local.rotation);
        let scaled_translation = [
            local.translation[0] * parent.scale[0],
            local.translation[1] * parent.scale[1],
            local.translation[2] * parent.scale[2],
        ];
        let translated = quat_rotate(parent.rotation, scaled_translation);
        Transform {
            translation: [
                parent.translation[0] + translated[0],
                parent.translation[1] + translated[1],
                parent.translation[2] + translated[2],
            ],
            rotation: normalize_quat(rotation),
            scale: [
                parent.scale[0] * local.scale[0],
                parent.scale[1] * local.scale[1],
                parent.scale[2] * local.scale[2],
            ],
        }
    }

    /// 变换点（含平移和缩放）。
    pub fn transform_point(&self, point: [f32; 3]) -> [f32; 3] {
        let scaled = [
            point[0] * self.scale[0],
            point[1] * self.scale[1],
            point[2] * self.scale[2],
        ];
        let rotated = quat_rotate(self.rotation, scaled);
        [
            rotated[0] + self.translation[0],
            rotated[1] + self.translation[1],
            rotated[2] + self.translation[2],
        ]
    }

    /// 变换方向（忽略平移）。
    pub fn transform_direction(&self, direction: [f32; 3]) -> [f32; 3] {
        let scaled = [
            direction[0] * self.scale[0],
            direction[1] * self.scale[1],
            direction[2] * self.scale[2],
        ];
        quat_rotate(self.rotation, scaled)
    }

    /// 是否所有分量都是有限值且缩放非零。
    pub fn is_finite(&self) -> bool {
        self.translation.iter().all(|value| value.is_finite())
            && self.rotation.iter().all(|value| value.is_finite())
            && self
                .scale
                .iter()
                .all(|value| value.is_finite() && *value != 0.0)
    }
}

/// 四元数乘法（`a * b`，先旋转 b 再旋转 a）。
pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// 归一化四元数；零四元数回退为单位四元数。
pub fn normalize_quat(mut quat: [f32; 4]) -> [f32; 4] {
    let length = quat.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length <= f32::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    for value in &mut quat {
        *value /= length;
    }
    quat
}

/// 用四元数旋转向量。
pub fn quat_rotate(quat: [f32; 4], vector: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = normalize_quat(quat);
    // v' = v + 2w(q×v) + 2q×(q×v)
    let qv = [x, y, z];
    let uv = cross(qv, vector);
    let uuv = cross(qv, uv);
    [
        vector[0] + 2.0 * (w * uv[0] + uuv[0]),
        vector[1] + 2.0 * (w * uv[1] + uuv[1]),
        vector[2] + 2.0 * (w * uv[2] + uuv[2]),
    ]
}

/// 三维叉积。
pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单位变换的矩阵与点变换均为恒等。
    #[test]
    fn identity_is_neutral() {
        let transform = Transform::IDENTITY;
        assert_eq!(transform.transform_point([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
        let matrix = transform.matrix();
        assert_eq!(matrix[0][0], 1.0);
        assert_eq!(matrix[1][1], 1.0);
        assert_eq!(matrix[2][2], 1.0);
        assert_eq!(matrix[3][3], 1.0);
        assert_eq!(matrix[3][0], 0.0);
    }

    /// 绕 Z 轴 90° 把 +X 转到 +Y。
    #[test]
    fn rotates_around_z() {
        let transform = Transform::from_axis_angle([0.0, 0.0, 1.0], std::f32::consts::FRAC_PI_2);
        let rotated = transform.transform_point([1.0, 0.0, 0.0]);
        assert!(rotated[0].abs() < 1e-5);
        assert!((rotated[1] - 1.0).abs() < 1e-5);
        assert!(rotated[2].abs() < 1e-5);
    }

    /// 父/子变换组合与逐级变换结果一致。
    #[test]
    fn composes_parent_and_child() {
        let parent = Transform::from_translation([0.0, 0.0, 10.0]);
        let child = Transform::from_axis_angle([0.0, 0.0, 1.0], std::f32::consts::FRAC_PI_2);
        let world = Transform::combine(&parent, &child);
        let direct = world.transform_point([1.0, 0.0, 0.0]);
        let staged = parent.transform_point(child.transform_point([1.0, 0.0, 0.0]));
        assert!((direct[0] - staged[0]).abs() < 1e-5);
        assert!((direct[1] - staged[1]).abs() < 1e-5);
        assert!((direct[2] - staged[2]).abs() < 1e-5);
    }

    /// 缩放参与局部平移的组合。
    #[test]
    fn applies_parent_scale_to_child_translation() {
        let parent = Transform::from_scale([2.0, 2.0, 2.0]);
        let child = Transform::from_translation([1.0, 2.0, 3.0]);
        let world = Transform::combine(&parent, &child);
        assert_eq!(world.translation, [2.0, 4.0, 6.0]);
        assert_eq!(world.scale, [2.0, 2.0, 2.0]);
    }

    /// 非法缩放会被 `is_finite` 拒绝。
    #[test]
    fn rejects_zero_scale() {
        let transform = Transform::from_scale([1.0, 0.0, 1.0]);
        assert!(!transform.is_finite());
    }
}
