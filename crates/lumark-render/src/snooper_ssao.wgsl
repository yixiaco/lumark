//! Snooper SSAO：半分辨率深度重建 + 法线半球采样，仅作为环境光遮蔽乘到颜色。

struct SsaoUniform {
    eye: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    // x=tan(fov/2), y=aspect, z=near, w=far
    params: vec4<f32>,
    // x=width, y=height, z=radius_pixels, w=strength
    screen: vec4<f32>,
};

@group(0) @binding(0) var<uniform> ssao: SsaoUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
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

fn depth_at(coord: vec2<i32>, dimensions: vec2<i32>) -> f32 {
    let clamped = clamp(coord, vec2<i32>(0), dimensions - vec2<i32>(1));
    return textureLoad(depth_texture, clamped, 0);
}

fn ray_direction(uv: vec2<f32>) -> vec3<f32> {
    let ndc = uv * 2.0 - vec2<f32>(1.0);
    let direction = ssao.forward.xyz
        + ssao.right.xyz * ndc.x * ssao.params.x * ssao.params.y
        + ssao.up.xyz * ndc.y * ssao.params.x;
    return normalize(direction);
}

fn world_from_depth(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let near = ssao.params.z;
    let far = ssao.params.w;
    let view_z = near * far / max(far - depth * (far - near), 1e-5);
    let ray = ray_direction(uv);
    return ssao.eye.xyz + ray * (view_z / max(dot(ray, ssao.forward.xyz), 1e-5));
}

fn sample_direction(index: i32, count: i32) -> vec2<f32> {
    let angle = (f32(index) + 0.5) / f32(count) * 6.2831853;
    let radius = sqrt((f32(index) + 0.5) / f32(count));
    return vec2<f32>(cos(angle), sin(angle)) * radius;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(depth_texture));
    let coord = vec2<i32>(in.uv * vec2<f32>(dimensions));
    let depth = depth_at(coord, dimensions);
    let base = textureSample(color_texture, color_sampler, in.uv);
    if (depth >= 1.0) {
        return base;
    }

    let world = world_from_depth(in.uv, depth);
    let normal = normalize(cross(dpdx(world), dpdy(world)));
    var occlusion = 0.0;
    var count = 0.0;
    let radius = ssao.screen.z;
    for (var index = 0; index < 10; index = index + 1) {
        let direction = sample_direction(index, 10);
        let sample_coord = coord + vec2<i32>(direction * radius);
        let sample_depth = depth_at(sample_coord, dimensions);
        if (sample_depth >= 1.0 || sample_depth >= depth) {
            continue;
        }
        let sample_uv = vec2<f32>(sample_coord) / vec2<f32>(dimensions);
        let sample_world = world_from_depth(sample_uv, sample_depth);
        let delta = sample_world - world;
        let distance = length(delta);
        if (distance <= 0.5 || distance > 80.0) {
            continue;
        }
        let normal_dot = abs(dot(normalize(delta), normal));
        occlusion = occlusion + normal_dot * (1.0 - smoothstep(20.0, 80.0, distance));
        count = count + 1.0;
    }
    var ao = 1.0;
    if (count > 0.0) {
        ao = clamp(1.0 - ssao.screen.w * occlusion / count, 0.35, 1.0);
    }
    return vec4<f32>(base.rgb * ao, base.a);
}
