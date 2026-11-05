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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let position = textureSample(g_position, g_sampler, in.uv).xyz;
    if (dot(position, position) < 1e-4) {
        let sky = mix(camera.sky_lower.rgb, camera.sky_upper.rgb, in.uv.y);
        return vec4<f32>(pow(max(sky, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), 1.0);
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
    let sun = diffuse * camera.light0_color.rgb * ndotl * camera.light0_dir.w;
    return vec4<f32>(pow(max(ambient + sun, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), 1.0);
}
