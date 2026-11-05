//! Snooper `Framebuffers/blur.frag`：H/V 双边模糊，第二遍顺带把 AO 乘回颜色。

struct BlurUniform {
    // x,y = 一个 texel 的方向；z = 0 输出 AO，1 输出最终颜色
    direction_mode: vec4<f32>,
};

@group(0) @binding(0) var<uniform> blur: BlurUniform;
@group(0) @binding(1) var ao_texture: texture_2d<f32>;
@group(0) @binding(2) var color_texture: texture_2d<f32>;
@group(0) @binding(3) var color_sampler: sampler;

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

fn unpack_normal(xy: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let center = textureSample(ao_texture, color_sampler, in.uv);
    let center_normal = unpack_normal(center.zw);
    let center_depth = center.y;
    if (dot(center_normal, center_normal) < 0.01) {
        if (blur.direction_mode.z > 0.5) {
            return textureSample(color_texture, color_sampler, in.uv);
        }
        return center;
    }
    var result = 0.0;
    var total_weight = 0.0;
    for (var offset = -2; offset <= 2; offset = offset + 1) {
        let tap = textureSample(
            ao_texture,
            color_sampler,
            in.uv + f32(offset) * blur.direction_mode.xy,
        );
        let sample_normal = unpack_normal(tap.zw);
        if (dot(sample_normal, sample_normal) < 0.01) {
            continue;
        }
        let depth_weight = exp(-abs(tap.y - center_depth) / max(center_depth * 0.05, 0.05));
        let normal_weight = pow(max(dot(sample_normal, center_normal), 0.0), 16.0);
        let weight = depth_weight * normal_weight;
        result += tap.x * weight;
        total_weight += weight;
    }
    let ao = select(center.x, result / total_weight, total_weight > 1e-4);
    if (blur.direction_mode.z > 0.5) {
        let base = textureSample(color_texture, color_sampler, in.uv);
        return vec4<f32>(base.rgb * mix(1.0, ao, 0.45), base.a);
    }
    return vec4<f32>(ao, center.y, center.zw);
}
