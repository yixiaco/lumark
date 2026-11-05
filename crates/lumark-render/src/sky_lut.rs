//! Snooper 大气 LUT：按 `Skybox/atmosphere.glsl` 的参数在 CPU 烘焙 256×128 天空纹理。
//!
//! 使用 Snooper 默认地球常量；输出已做 `1-exp(-color)` 曝光，供背景 shader 直接采样。

const LUT_WIDTH: u32 = 256;
const LUT_HEIGHT: u32 = 128;
const PI: f32 = 3.141_592_7;
const I_STEPS: usize = 16;
const J_STEPS: usize = 8;

const PLANET_RADIUS: f32 = 6360.0e3;
const ATMOSPHERE_HEIGHT: f32 = 60.0e3;
const ALTITUDE: f32 = 100.0;
const RAYLEIGH_SCATTERING: [f32; 3] = [5.802e-6, 13.558e-6, 33.1e-6];
const RAYLEIGH_SCALE_HEIGHT: f32 = 8.0e3;
const MIE_SCATTERING: f32 = 3.996e-6;
const MIE_ABSORPTION: f32 = 0.444e-6;
const MIE_SCALE_HEIGHT: f32 = 1.2e3;
const MIE_ANISOTROPY: f32 = 0.9;
const OZONE_ABSORPTION: [f32; 3] = [0.650e-6, 1.881e-6, 0.085e-6];
/// Snooper `SkyParameters.Intensity`；太阳颜色 (1, 0.96, 0.90)。
const SUN_LUMINANCE: [f32; 3] = [56.0, 53.76, 50.4];

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let magnitude = length(a).max(1e-20);
    [a[0] / magnitude, a[1] / magnitude, a[2] / magnitude]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], value: f32) -> [f32; 3] {
    [a[0] * value, a[1] * value, a[2] * value]
}

/// Ray-sphere intersection；无交点时返回 `(1e5, -1e5)`，与 Snooper 一致。
fn ray_sphere(origin: [f32; 3], direction: [f32; 3], radius: f32) -> (f32, f32) {
    let a = dot(direction, direction);
    let b = 2.0 * dot(direction, origin);
    let c = dot(origin, origin) - radius * radius;
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return (1e5, -1e5);
    }
    (
        (-b - discriminant.sqrt()) / (2.0 * a),
        (-b + discriminant.sqrt()) / (2.0 * a),
    )
}

fn ozone(height: f32) -> f32 {
    (1.0 - (height - 25.0e3).abs() / 15.0e3).max(0.0)
}

#[allow(clippy::too_many_arguments)]
fn atmosphere(
    ray: [f32; 3],
    origin: [f32; 3],
    sun: [f32; 3],
    planet_radius: f32,
    atmosphere_radius: f32,
) -> [f32; 3] {
    let ray = normalize(ray);
    let sun = normalize(sun);
    let (mut near, mut far) = ray_sphere(origin, ray, atmosphere_radius);
    if near > far {
        return [0.0; 3];
    }
    near = near.max(0.0);
    let (ground_near, ground_far) = ray_sphere(origin, ray, planet_radius);
    if ground_near < ground_far && ground_near > 0.0 {
        far = far.min(ground_near);
    }
    if far <= near {
        return [0.0; 3];
    }
    let primary_step = (far - near) / I_STEPS as f32;
    let mut time = near;

    let mut total_rayleigh = [0.0; 3];
    let mut total_mie = [0.0; 3];
    let mut od_rayleigh = 0.0;
    let mut od_mie = 0.0;
    let mut od_ozone = 0.0;

    let mu = dot(ray, sun);
    let mumu = mu * mu;
    let g = MIE_ANISOTROPY;
    let gg = g * g;
    let phase_rayleigh = 3.0 / (16.0 * PI) * (1.0 + mumu);
    let phase_mie = 3.0 / (8.0 * PI) * ((1.0 - gg) * (mumu + 1.0))
        / ((1.0 + gg - 2.0 * mu * g).powf(1.5) * (2.0 + gg));

    for _ in 0..I_STEPS {
        let position = add(origin, scale(ray, time + primary_step * 0.5));
        let height = length(position) - planet_radius;
        let step_rayleigh = (-height / RAYLEIGH_SCALE_HEIGHT).exp() * primary_step;
        let step_mie = (-height / MIE_SCALE_HEIGHT).exp() * primary_step;
        od_rayleigh += step_rayleigh;
        od_mie += step_mie;
        od_ozone += ozone(height) * primary_step;

        let secondary_step = ray_sphere(position, sun, atmosphere_radius).1 / J_STEPS as f32;
        let mut secondary_time = 0.0;
        let mut j_od_rayleigh = 0.0;
        let mut j_od_mie = 0.0;
        let mut j_od_ozone = 0.0;
        for _ in 0..J_STEPS {
            let secondary = add(position, scale(sun, secondary_time + secondary_step * 0.5));
            let secondary_height = length(secondary) - planet_radius;
            j_od_rayleigh += (-secondary_height / RAYLEIGH_SCALE_HEIGHT).exp() * secondary_step;
            j_od_mie += (-secondary_height / MIE_SCALE_HEIGHT).exp() * secondary_step;
            j_od_ozone += ozone(secondary_height) * secondary_step;
            secondary_time += secondary_step;
        }

        let mut attenuation = [0.0; 3];
        for channel in 0..3 {
            let optical_depth = (MIE_SCATTERING + MIE_ABSORPTION) * (od_mie + j_od_mie)
                + RAYLEIGH_SCATTERING[channel] * (od_rayleigh + j_od_rayleigh)
                + OZONE_ABSORPTION[channel] * (od_ozone + j_od_ozone);
            attenuation[channel] = (-optical_depth).exp();
        }
        for channel in 0..3 {
            total_rayleigh[channel] += step_rayleigh * attenuation[channel];
            total_mie[channel] += step_mie * attenuation[channel];
        }
        time += primary_step;
    }

    let mut color = [0.0; 3];
    for channel in 0..3 {
        color[channel] = phase_rayleigh * RAYLEIGH_SCATTERING[channel] * total_rayleigh[channel]
            + phase_mie * MIE_SCATTERING * total_mie[channel];
    }
    color
}

/// 全局缓存的天空 LUT（CPU/GPU 共用）。
pub(crate) fn baked() -> &'static [u8] {
    static CACHE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    CACHE.get_or_init(bake)
}

/// 烘焙天空 LUT，返回 RGBA8 像素（已曝光）。
pub(crate) fn bake() -> Vec<u8> {
    let sun = normalize([0.766_044_4, 0.0, 0.642_787_6]);
    let origin = [0.0, PLANET_RADIUS + ALTITUDE, 0.0];
    let atmosphere_radius = PLANET_RADIUS + ATMOSPHERE_HEIGHT;
    let mut pixels = Vec::with_capacity((LUT_WIDTH * LUT_HEIGHT * 4) as usize);
    for y in 0..LUT_HEIGHT {
        let row = (y as f32 + 0.5) / LUT_HEIGHT as f32 * 2.0 - 1.0;
        let elevation = row.signum() * row * row * 0.5 * PI;
        let cos_elevation = elevation.cos();
        for x in 0..LUT_WIDTH {
            let u = (x as f32 + 0.5) / LUT_WIDTH as f32;
            let azimuth = (u - 0.5) * 2.0 * PI;
            let direction = [
                cos_elevation * azimuth.cos(),
                elevation.sin(),
                cos_elevation * azimuth.sin(),
            ];
            let radiance = atmosphere(direction, origin, sun, PLANET_RADIUS, atmosphere_radius);
            for channel in 0..3 {
                let luminance = radiance[channel] * SUN_LUMINANCE[channel];
                let exposed = 1.0 - (-luminance).exp();
                pixels.push((exposed.clamp(0.0, 1.0) * 255.0) as u8);
            }
            pixels.push(255);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_sky_lut_is_not_empty() {
        let pixels = bake();
        assert_eq!(pixels.len(), (LUT_WIDTH * LUT_HEIGHT * 4) as usize);
        let sum: u64 = pixels.iter().map(|value| u64::from(*value)).sum();
        assert!(sum > 0, "大气 LUT 不能全黑");
        let horizon = &pixels[(LUT_HEIGHT / 2 * LUT_WIDTH * 4) as usize..];
        assert!(horizon.iter().any(|value| *value > 8), "地平线应有散射亮度");
    }
}
/// LUT 尺寸。
pub(crate) const fn size() -> (u32, u32) {
    (LUT_WIDTH, LUT_HEIGHT)
}
