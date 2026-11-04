//! Snooper GPU FXAA 最终 pass：高对比边方向混合，输出可直接回读。

struct FxaaUniform {
    // x=width, y=height, z=contrast threshold, w=blend strength
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
    let center = textureSample(color_texture, color_sampler, in.uv);
    let texel = 1.0 / max(fxaa.screen.xy, vec2<f32>(1.0));
    let north = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(0.0, -texel.y));
    let south = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(0.0, texel.y));
    let west = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(-texel.x, 0.0));
    let east = textureSample(color_texture, color_sampler, in.uv + vec2<f32>(texel.x, 0.0));
    let l_center = luma(center.rgb);
    let l_north = luma(north.rgb);
    let l_south = luma(south.rgb);
    let l_west = luma(west.rgb);
    let l_east = luma(east.rgb);
    let contrast = max(max(l_north, l_south), max(l_west, l_east))
        - min(min(l_north, l_south), min(l_west, l_east));
    if (contrast < fxaa.screen.z) {
        return center;
    }
    var neighbor: vec3<f32>;
    if (abs(l_west - l_east) >= abs(l_north - l_south)) {
        neighbor = 0.5 * (
            textureSample(color_texture, color_sampler, in.uv + vec2<f32>(-texel.x, 0.0)).rgb
                + textureSample(color_texture, color_sampler, in.uv + vec2<f32>(texel.x, 0.0)).rgb
        );
    } else {
        neighbor = 0.5 * (
            textureSample(color_texture, color_sampler, in.uv + vec2<f32>(0.0, -texel.y)).rgb
                + textureSample(color_texture, color_sampler, in.uv + vec2<f32>(0.0, texel.y)).rgb
        );
    }
    let blend = fxaa.screen.w;
    return vec4<f32>(mix(center.rgb, neighbor, blend), center.a);
}
