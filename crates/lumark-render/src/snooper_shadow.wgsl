//! 太阳阴影深度 pass：与主顶点着色器一致的蒙皮/实例变换。

struct ShadowCamera {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: ShadowCamera;
@group(1) @binding(6) var<storage, read> joint_matrices: array<mat4x4<f32>>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) joints: vec4<f32>,
    @location(6) weights: vec4<f32>,
    @location(7) instance_c0: vec4<f32>,
    @location(8) instance_c1: vec4<f32>,
    @location(9) instance_c2: vec4<f32>,
    @location(10) instance_c3: vec4<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> @builtin(position) vec4<f32> {
    var position = in.position;
    let weight_sum = in.weights.x + in.weights.y + in.weights.z + in.weights.w;
    if (weight_sum > 0.0) {
        let inverse = 1.0 / weight_sum;
        var blended = in.weights.x * inverse * joint_matrices[u32(in.joints.x)];
        blended = blended + in.weights.y * inverse * joint_matrices[u32(in.joints.y)];
        blended = blended + in.weights.z * inverse * joint_matrices[u32(in.joints.z)];
        blended = blended + in.weights.w * inverse * joint_matrices[u32(in.joints.w)];
        position = (blended * vec4<f32>(position, 1.0)).xyz;
    }
    let instance = mat4x4<f32>(in.instance_c0, in.instance_c1, in.instance_c2, in.instance_c3);
    position = (instance * vec4<f32>(position, 1.0)).xyz;
    return camera.view_proj * vec4<f32>(position, 1.0);
}
