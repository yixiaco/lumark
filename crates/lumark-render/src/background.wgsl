// 背景渐变：Studio/Headlight 保留旧渐变；Snooper 模式绘制天空球方向色 + 太阳盘。

struct Background {
    bottom: vec4<f32>,
    top: vec4<f32>,
    eye: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    // x=tan(fov/2), y=aspect, z=是否 Snooper 天空
    params: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_upper: vec4<f32>,
    sky_lower: vec4<f32>,
};

@group(0) @binding(0) var<uniform> background: Background;
@group(0) @binding(1) var sky_lut: texture_2d<f32>;
@group(0) @binding(2) var sky_sampler: sampler;

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let position = positions[index];
    var out: VertexOut;
    out.clip_position = vec4<f32>(position, 0.0, 1.0);
    out.uv = position * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let linear = mix(background.bottom.rgb, background.top.rgb, clamp(in.uv.y, 0.0, 1.0));
    let mapped = linear / (vec3<f32>(1.0) + linear);
    let gradient = pow(mapped, vec3<f32>(1.0 / 2.2));
    if (background.params.z < 0.5) {
        return vec4<f32>(gradient, 1.0);
    }

    // 天空球射线：从屏幕 uv 反投影到相机方向。
    let ndc = in.uv * 2.0 - vec2<f32>(1.0);
    let direction = normalize(
        background.forward.xyz
            + background.right.xyz * ndc.x * background.params.x * background.params.y
            + background.up.xyz * ndc.y * background.params.x
    );
    // Snooper 使用 OpenGL Y-up；查看器为 Z-up，先转换到天空 LUT 坐标。
    let gl_direction = normalize(vec3<f32>(direction.x, direction.z, direction.y));
    let azimuth = atan2(gl_direction.z, gl_direction.x);
    let elevation = asin(clamp(gl_direction.y, -1.0, 1.0));
    let row = sign(elevation) * sqrt(abs(elevation) / (0.5 * 3.1415927));
    let sky_uv = vec2<f32>(azimuth / (2.0 * 3.1415927) + 0.5, row * 0.5 + 0.5);
    let sky = textureSample(sky_lut, sky_sampler, sky_uv).rgb;

    let sun = normalize(vec3<f32>(
        background.sun_dir.x,
        background.sun_dir.z,
        background.sun_dir.y,
    ));
    let mu = dot(gl_direction, sun);
    let edge = max(fwidth(mu), 1e-7);
    let disc = smoothstep(cos(0.004675) - edge, cos(0.004675) + edge, mu);
    let brightest = max(max(sky.r, sky.g), max(sky.b, 1e-4));
    let tint = sky / brightest;
    let color = sky + disc * 50.0 * tint * smoothstep(-0.01, 0.01, direction.z);
    return vec4<f32>(pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), 1.0);
}
