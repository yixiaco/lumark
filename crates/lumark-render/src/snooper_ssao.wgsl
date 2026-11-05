//! Snooper `Framebuffers/ssao.frag` 的 WGSL 移植（深度重建世界位置/法线）。

struct SsaoUniform {
    eye: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    params: vec4<f32>,
    // x=width, y=height, z=radius(world), w=maxDistance
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

fn interleaved_gradient_noise(position: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(position, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(depth_texture));
    let coord = vec2<i32>(in.uv * vec2<f32>(dimensions));
    let depth = depth_at(coord, dimensions);
    if (depth >= 1.0) {
        return vec4<f32>(1.0, 0.0, 0.0, 0.0);
    }
    let world = world_from_depth(in.uv, depth);
    let view_depth = max(dot(world - ssao.eye.xyz, ssao.forward.xyz), 0.0);
    if (view_depth <= 0.0 || view_depth >= ssao.screen.w) {
        return vec4<f32>(1.0, 0.0, 0.0, 0.0);
    }
    var normal = normalize(cross(dpdx(world), dpdy(world)));
    let view = normalize(ssao.eye.xyz - world);
    if (dot(normal, view) < 0.0) {
        normal = -normal;
    }

    let radius = max(ssao.screen.z, 1.0);
    let radius_pixels = clamp(
        radius * ssao.screen.y / (2.0 * max(view_depth, 1.0) * ssao.params.x),
        4.0,
        256.0,
    );
    let noise = interleaved_gradient_noise(vec2<f32>(coord));
    let slice_rotation = noise * 3.1415927;
    let step_jitter = fract(noise * 8.423);
    let radius_sq = radius * radius;
    let texel = vec2<f32>(1.0) / vec2<f32>(dimensions);
    var occlusion = 0.0;
    for (var slice = 0; slice < 3; slice = slice + 1) {
        let angle = slice_rotation + 3.1415927 * f32(slice) / 3.0;
        let direction = vec2<f32>(cos(angle), sin(angle));
        for (var side = 0; side < 2; side = side + 1) {
            let side_sign = select(-1.0, 1.0, side == 0);
            var max_contribution = 0.0;
            for (var step = 0; step < 4; step = step + 1) {
                let t = (f32(step) + step_jitter) / 4.0;
                let step_distance = t * t * radius_pixels;
                let sample_uv = in.uv + direction * side_sign * step_distance * texel;
                let sample_depth = depth_at(
                    vec2<i32>(sample_uv * vec2<f32>(dimensions)),
                    dimensions,
                );
                if (sample_depth >= 1.0) {
                    continue;
                }
                let sample_world = world_from_depth(sample_uv, sample_depth);
                let horizon = sample_world - world;
                let horizon_sq = dot(horizon, horizon);
                if (horizon_sq < 1e-8) {
                    continue;
                }
                let contribution = max(0.0, dot(normalize(horizon), normal))
                    * clamp(1.0 - horizon_sq / radius_sq, 0.0, 1.0);
                max_contribution = max(max_contribution, contribution);
            }
            occlusion += max_contribution;
        }
    }
    occlusion /= 6.0;
    let visibility = clamp(1.0 - occlusion, 0.0, 1.0);
    var ao = mix(0.05, 1.0, visibility);
    ao = mix(1.0, ao, 1.0 - smoothstep(ssao.screen.w * 0.7, ssao.screen.w, view_depth));
    // 前向管线没有单独的 ambient 通道，按 Snooper AO 强度折半近似。
    return vec4<f32>(ao, view_depth, normal.xy);
}
