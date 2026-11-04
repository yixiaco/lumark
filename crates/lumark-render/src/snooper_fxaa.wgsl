//! Snooper `Framebuffers/fxaa.frag` 的 WGSL 移植。

struct FxaaUniform {
    // x=width, y=height
    screen: vec4<f32>,
};

@group(0) @binding(0) var<uniform> fxaa: FxaaUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;

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
    out.uv = vec2<f32>(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
    return out;
}

fn luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let inv_res = 1.0 / max(fxaa.screen.xy, vec2<f32>(1.0));
    let rgb_nw = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(-1.0, -1.0) * inv_res).rgb;
    let rgb_ne = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(1.0, -1.0) * inv_res).rgb;
    let rgb_sw = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(-1.0, 1.0) * inv_res).rgb;
    let rgb_se = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(1.0, 1.0) * inv_res).rgb;
    let rgb_m = textureSample(color_texture, color_sampler, in.uv).rgb;

    let luma_m = luma(rgb_m);
    let luma_nw = luma(rgb_nw);
    let luma_ne = luma(rgb_ne);
    let luma_sw = luma(rgb_sw);
    let luma_se = luma(rgb_se);
    let luma_min = min(luma_m, min(min(luma_nw, luma_ne), min(luma_sw, luma_se)));
    let luma_max = max(luma_m, max(max(luma_nw, luma_ne), max(luma_sw, luma_se)));

    var direction = vec2<f32>(0.0);
    direction.x = -((luma_nw + luma_ne) - (luma_sw + luma_se));
    direction.y = (luma_nw + luma_sw) - (luma_ne + luma_se);
    let dir_reduce = max((luma_nw + luma_ne + luma_sw + luma_se) * (1.0 / 8.0 * 0.25), 1.0 / 128.0);
    let rcp_dir_min = 1.0 / (min(abs(direction.x), abs(direction.y)) + dir_reduce);
    direction = clamp(direction * rcp_dir_min * inv_res, -8.0 * inv_res, 8.0 * inv_res);

    let rgb_a = 0.5 * (
        textureSample(color_texture, color_sampler, in.uv + direction * (1.0 / 3.0 - 0.5)).rgb
            + textureSample(color_texture, color_sampler, in.uv + direction * (2.0 / 3.0 - 0.5)).rgb
    );
    let rgb_b = rgb_a * 0.5 + 0.25 * (
        textureSample(color_texture, color_sampler, in.uv + direction * -0.5).rgb
            + textureSample(color_texture, color_sampler, in.uv + direction * 0.5).rgb
    );
    let luma_b = luma(rgb_b);
    if (luma_b < luma_min || luma_b > luma_max) {
        return vec4<f32>(rgb_a, 1.0);
    }
    return vec4<f32>(rgb_b, 1.0);
}
