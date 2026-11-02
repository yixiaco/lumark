// 叠加层线段着色器：地面网格 / 骨骼线段。
// 使用与场景着色器相同的相机 uniform（仅用到 view_proj 字段）。

struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light0_dir: vec4<f32>,
    light0_color: vec4<f32>,
    light1_dir: vec4<f32>,
    light1_color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: LineIn) -> LineOut {
    var out: LineOut;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: LineOut) -> @location(0) vec4<f32> {
    let non_negative = max(in.color.rgb, vec3<f32>(0.0));
    let mapped = non_negative / (vec3<f32>(1.0) + non_negative);
    return vec4<f32>(pow(mapped, vec3<f32>(1.0 / 2.2)), in.color.a);
}
