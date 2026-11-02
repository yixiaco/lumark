// 背景渐变全屏三角形：与 CPU 后端一致，顶部略亮、底部略暗。

struct Background {
    bottom: vec4<f32>,
    top: vec4<f32>,
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
    return vec4<f32>(pow(mapped, vec3<f32>(1.0 / 2.2)), 1.0);
}
