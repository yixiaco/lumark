//! Snooper 全屏 grid：射线与地面求交，写入 G-buffer（不再依赖几何地面）。

struct GridUniform {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    // x=tanHalfFov, y=aspect, z=cell, w=majorCell
    params: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    line_minor: vec4<f32>,
    line_major: vec4<f32>,
    // x=near, y=far, z=height, w=unused
    depth: vec4<f32>,
};

@group(0) @binding(0) var<uniform> grid: GridUniform;

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

struct GbufferOut {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) albedo: vec4<f32>,
    @location(3) specular: vec4<f32>,
    @builtin(frag_depth) frag_depth: f32,
};

fn line_coverage(coord: vec2<f32>, width_pixels: f32) -> f32 {
    let derivative = max(fwidth(coord), vec2<f32>(1e-6));
    let distance = abs(fract(coord - 0.5) - 0.5);
    let aa = derivative * 1.5;
    let line = smoothstep(derivative * width_pixels * 0.5 + aa, derivative * width_pixels * 0.5 - aa, distance);
    return max(line.x, line.y);
}

@fragment
fn fs_main(in: VertexOut) -> GbufferOut {
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let direction = normalize(
        grid.forward.xyz
            + grid.right.xyz * ndc.x * grid.params.x * grid.params.y
            + grid.up.xyz * ndc.y * grid.params.x
    );
    if (abs(direction.z) < 1e-6) {
        discard;
    }
    let t = (grid.depth.z - grid.eye.z) / direction.z;
    if (t <= 0.0) {
        discard;
    }
    let world = grid.eye.xyz + direction * t;
    let clip = grid.view_proj * vec4<f32>(world, 1.0);
    let depth = clip.z / clip.w;
    if (depth < 0.0 || depth >= 1.0) {
        discard;
    }
    let camera_distance = length(world - grid.eye.xyz);
    let fade = 1.0 - smoothstep(grid.depth.y * 0.55, grid.depth.y, camera_distance);
    if (fade <= 0.001) {
        discard;
    }

    let cell = grid.params.z;
    let major = grid.params.w;
    let minor_coord = world.xy / cell;
    let major_coord = world.xy / major;
    let minor = line_coverage(minor_coord, 1.0);
    let major_line = line_coverage(major_coord, 1.5);
    let checker = (fract(world.x / cell) + fract(world.y / cell)) % 2.0;
    var albedo = mix(grid.color_a.rgb, grid.color_b.rgb, checker);
    albedo = mix(albedo, grid.line_minor.rgb, minor * grid.line_minor.w);
    albedo = mix(albedo, grid.line_major.rgb, major_line * grid.line_major.w);

    var out: GbufferOut;
    // w=1 标记编辑器网格，延迟光照不对它接收角色投影。
    out.position = vec4<f32>(world, 1.0);
    out.normal = vec4<f32>(0.0, 0.0, 1.0, 0.0);
    out.albedo = vec4<f32>(albedo, fade);
    out.specular = vec4<f32>(0.5, 0.0, 0.9, 1.0);
    out.frag_depth = depth;
    return out;
}
