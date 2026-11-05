//! Snooper 延迟光照第一版：G-buffer → 方向光/天空环境光，天空像素用天空色。

struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    forward: vec4<f32>,
    light0_dir: vec4<f32>,
    light0_color: vec4<f32>,
    light1_dir: vec4<f32>,
    light1_color: vec4<f32>,
    ambient: vec4<f32>,
    sky_upper: vec4<f32>,
    sky_lower: vec4<f32>,
    fog_color: vec4<f32>,
    fog_params: vec4<f32>,
    shadow_view_proj_0: mat4x4<f32>,
    shadow_view_proj_1: mat4x4<f32>,
    shadow_view_proj_2: mat4x4<f32>,
    shadow_view_proj_3: mat4x4<f32>,
    shadow_params: vec4<f32>,
    shadow_splits: vec4<f32>,
    shadow_cascade_params: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var shadow_texture: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(1) @binding(0) var g_position: texture_2d<f32>;
@group(1) @binding(1) var g_normal: texture_2d<f32>;
@group(1) @binding(2) var g_albedo: texture_2d<f32>;
@group(1) @binding(3) var g_specular: texture_2d<f32>;
@group(1) @binding(4) var g_sampler: sampler;

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

fn shadow_matrix(layer: i32) -> mat4x4<f32> {
    if (layer == 0) { return camera.shadow_view_proj_0; }
    if (layer == 1) { return camera.shadow_view_proj_1; }
    if (layer == 2) { return camera.shadow_view_proj_2; }
    return camera.shadow_view_proj_3;
}

fn shadow_factor(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    if (camera.shadow_params.x < 0.5) { return 1.0; }
    let light = normalize(camera.light0_dir.xyz);
    let n_dot_l = max(dot(normal, light), 0.0);
    if (n_dot_l <= 0.0) { return 0.0; }
    let depth = dot(world - camera.eye.xyz, camera.forward.xyz);
    var layer = -1;
    if (depth < camera.shadow_splits.x) { layer = 0; }
    else if (depth < camera.shadow_splits.y) { layer = 1; }
    else if (depth < camera.shadow_splits.z) { layer = 2; }
    else if (depth < camera.shadow_splits.w) { layer = 3; }
    if (layer < 0) { return 0.0; }
    let params = camera.shadow_cascade_params[layer];
    let sin_theta = sqrt(max(0.0, 1.0 - n_dot_l * n_dot_l));
    let sample_pos = world + normal * (params.x * 3.0 * sin_theta * camera.shadow_params.w);
    let clip = shadow_matrix(layer) * vec4<f32>(sample_pos, 1.0);
    if (clip.w <= 0.0) { return 1.0; }
    let ndc = clip.xyz / clip.w;
    if (ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 || ndc.z >= 1.0) { return 1.0; }
    let reference = ndc.z - params.x * min(sin_theta / max(n_dot_l, 1e-3), 8.0) * params.y;
    let texel = camera.shadow_params.z / max(camera.shadow_params.y, 1.0);
    var lit = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            lit = lit + textureSampleCompare(
                shadow_texture,
                shadow_sampler,
                ndc.xy * 0.5 + vec2<f32>(0.5) + vec2<f32>(f32(x), f32(y)) * texel,
                layer,
                reference,
            );
        }
    }
    return lit / 9.0;
}

fn apply_fog(color: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    if (camera.fog_color.w < 0.5) { return color; }
    // UE 场景为厘米；Snooper 雾参数按米积分。
    let ray = (world - camera.eye.xyz) / 100.0;
    let range = length(ray);
    if (range <= 1e-4) { return color; }
    let direction = ray / range;
    let start = camera.fog_params.w / 100.0;
    let fog_range = range - start;
    if (fog_range <= 0.0) { return color; }
    let height = camera.eye.z / 100.0 + direction.z * start;
    let climb = direction.z * fog_range;
    let column = select(
        exp(-camera.fog_params.y * max(height, 0.0)),
        ((1.0 - exp(-camera.fog_params.y * abs(climb))) / max(camera.fog_params.y, 1e-4))
            / max(abs(climb), 1e-3),
        abs(climb) > 1e-3,
    );
    let amount = clamp(1.0 - exp(-camera.fog_params.x * fog_range * column), 0.0, camera.fog_params.z);
    let fog = pow(clamp(camera.fog_color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    return mix(color, fog, amount);
}
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let position = textureSample(g_position, g_sampler, in.uv).xyz;
    if (dot(position, position) < 1e-4) {
        discard;
    }
    let normal = normalize(textureSample(g_normal, g_sampler, in.uv).xyz);
    let albedo = textureSample(g_albedo, g_sampler, in.uv).rgb;
    let specular = textureSample(g_specular, g_sampler, in.uv);
    let light = normalize(camera.light0_dir.xyz);
    let ndotl = max(dot(normal, light), 0.0);
    let sky_mix = clamp(normal.z * 0.5 + 0.5, 0.0, 1.0);
    let sky = mix(camera.sky_lower.rgb, camera.sky_upper.rgb, sky_mix);
    let ambient = sky * albedo * specular.w;
    let diffuse = albedo * (1.0 - clamp(specular.y, 0.0, 1.0)) / 3.1415927;
    let shadow = shadow_factor(position, normal);
    let sun = diffuse * camera.light0_color.rgb * ndotl * camera.light0_dir.w * shadow;
    let encoded = pow(max(ambient + sun, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    return vec4<f32>(apply_fog(encoded, position), 1.0);
}
