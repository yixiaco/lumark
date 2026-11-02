// Lumark GPU 着色器：与 CPU 参考渲染器一致的近似 PBR + 双灯 + 法线贴图。
// 输出到 Rgba8Unorm 目标，因此这里手动做 tone mapping 与 sRGB 编码。

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

struct Material {
    base_color: vec4<f32>,
    // x = metallic, y = roughness, z = has base texture, w = has normal texture
    params: vec4<f32>,
    emissive: vec4<f32>,
};

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_texture: texture_2d<f32>;
@group(1) @binding(3) var tex_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) tangent: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) tangent: vec4<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.world = in.position;
    out.normal = in.normal;
    out.uv = in.uv;
    out.color = in.color;
    out.tangent = in.tangent;
    return out;
}

fn light_contribution(
    normal: vec3<f32>,
    view: vec3<f32>,
    base: vec3<f32>,
    light_dir: vec3<f32>,
    light_color: vec3<f32>,
    intensity: f32,
    shininess: f32,
) -> vec3<f32> {
    if intensity <= 0.0 {
        return vec3<f32>(0.0);
    }
    let l = normalize(light_dir);
    let ndl = max(dot(normal, l), 0.0);
    if ndl <= 0.0 {
        return vec3<f32>(0.0);
    }
    let half_vector = normalize(l + view);
    let ndh = max(dot(normal, half_vector), 0.0);
    let specular = pow(ndh, shininess);
    return base * light_color * intensity * ndl
        + light_color * specular * intensity * vec3<f32>(0.1);
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    var normal = normalize(in.normal);
    if (!front_facing) {
        normal = -normal;
    }
    var base = material.base_color.rgb;
    if (material.params.z > 0.5) {
        base = base * textureSample(base_texture, tex_sampler, in.uv).rgb;
    }
    base = base * in.color.rgb;
    if (material.params.w > 0.5) {
        let tangent = normalize(in.tangent.xyz - normal * dot(normal, in.tangent.xyz));
        let bitangent = cross(normal, tangent) * sign(in.tangent.w);
        let tangent_normal = textureSample(normal_texture, tex_sampler, in.uv).xyz * 2.0
            - vec3<f32>(1.0);
        normal = normalize(
            tangent * tangent_normal.x
            + bitangent * tangent_normal.y
            + normal * tangent_normal.z,
        );
    }
    let view = normalize(camera.eye.xyz - in.world);
    if (dot(normal, view) < 0.0) {
        normal = -normal;
    }
    let roughness = clamp(material.params.y, 0.04, 1.0);
    let shininess = clamp(2.0 / pow(roughness, 4.0), 4.0, 2048.0);
    var color = base * camera.ambient.x;
    color = color + light_contribution(
        normal,
        view,
        base,
        camera.light0_dir.xyz,
        camera.light0_color.rgb,
        camera.light0_dir.w,
        shininess,
    );
    color = color + light_contribution(
        normal,
        view,
        base,
        camera.light1_dir.xyz,
        camera.light1_color.rgb,
        camera.light1_dir.w,
        shininess,
    );
    color = color + material.emissive.rgb;
    let non_negative = max(color, vec3<f32>(0.0));
    let mapped = non_negative / (vec3<f32>(1.0) + non_negative);
    return vec4<f32>(pow(mapped, vec3<f32>(1.0 / 2.2)), 1.0);
}
