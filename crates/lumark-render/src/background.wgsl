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
    let elevation = clamp(direction.z * 0.5 + 0.5, 0.0, 1.0);
    // 天空观感色（Snooper 大气 LUT 的近似；光照仍使用 background.sky_*）。
    let horizon = vec3<f32>(0.55, 0.72, 0.92);
    let zenith = vec3<f32>(0.10, 0.28, 0.65);
    let sky = mix(horizon, zenith, pow(elevation, 0.65));
    let sun = pow(max(dot(direction, normalize(background.sun_dir.xyz)), 0.0), 256.0);
    let color = sky + background.sun_color.rgb * sun * 4.0;
    return vec4<f32>(pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), 1.0);
}
