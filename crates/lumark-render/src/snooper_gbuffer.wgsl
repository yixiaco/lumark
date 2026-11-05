//! Snooper 风格 G-buffer：位置/法线/基础色/高光参数，供后续延迟光照与全屏 grid 使用。

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

struct Material {
    base_color: vec4<f32>,
    params: vec4<f32>,
    emissive: vec4<f32>,
    flags: vec4<f32>,
    extra: vec4<f32>,
    extra2: vec4<f32>,
    extra3: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_texture: texture_2d<f32>;
@group(1) @binding(3) var tex_sampler: sampler;
@group(1) @binding(4) var orm_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var<storage, read> joint_matrices: array<mat4x4<f32>>;
@group(1) @binding(7) var ao_texture: texture_2d<f32>;
@group(1) @binding(8) var sclera_texture: texture_2d<f32>;

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
    var position = in.position;
    var normal = in.normal;
    var tangent = in.tangent;
    let weight_sum = in.weights.x + in.weights.y + in.weights.z + in.weights.w;
    if (weight_sum > 0.0) {
        let inverse = 1.0 / weight_sum;
        var blended = in.weights.x * inverse * joint_matrices[u32(in.joints.x)];
        blended = blended + in.weights.y * inverse * joint_matrices[u32(in.joints.y)];
        blended = blended + in.weights.z * inverse * joint_matrices[u32(in.joints.z)];
        blended = blended + in.weights.w * inverse * joint_matrices[u32(in.joints.w)];
        position = (blended * vec4<f32>(position, 1.0)).xyz;
        normal = normalize((blended * vec4<f32>(normal, 0.0)).xyz);
        tangent = vec4<f32>(normalize((blended * vec4<f32>(tangent.xyz, 0.0)).xyz), tangent.w);
    }
    let instance = mat4x4<f32>(in.instance_c0, in.instance_c1, in.instance_c2, in.instance_c3);
    position = (instance * vec4<f32>(position, 1.0)).xyz;
    normal = normalize((instance * vec4<f32>(normal, 0.0)).xyz);
    tangent = vec4<f32>(normalize((instance * vec4<f32>(tangent.xyz, 0.0)).xyz), tangent.w);
    var out: VertexOut;
    out.clip_position = camera.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    out.normal = normal;
    out.uv = in.uv;
    out.color = in.color;
    out.tangent = tangent;
    return out;
}

struct GbufferOut {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) albedo: vec4<f32>,
    @location(3) specular: vec4<f32>,
};

fn unpack_normal(value: vec4<f32>) -> vec3<f32> {
    return normalize(value.xyz * 2.0 - vec3<f32>(1.0, 1.0, 1.0));
}

@fragment
fn fs_main(in: VertexOut) -> GbufferOut {
    var normal = normalize(in.normal);
    if (material.params.w > 0.5) {
        let tangent_normal = unpack_normal(textureSample(normal_texture, tex_sampler, in.uv));
        let tangent = normalize(in.tangent.xyz - normal * dot(normal, in.tangent.xyz));
        let bitangent = normalize(cross(normal, tangent)) * in.tangent.w;
        normal = normalize(tangent * tangent_normal.x + bitangent * tangent_normal.y + normal * tangent_normal.z);
    }

    var albedo = material.base_color;
    if (material.params.z > 0.5) {
        albedo = albedo * textureSample(base_texture, tex_sampler, in.uv);
    }
    if (camera.eye.w > 0.5) {
        albedo = albedo * in.color;
    }
    if (albedo.a < material.flags.y) {
        discard;
    }

    var roughness = material.params.y;
    var metallic = material.params.x;
    var ao = 1.0;
    if (material.flags.x > 0.5) {
        let orm = textureSample(orm_texture, tex_sampler, in.uv);
        if (material.flags.z < 0.5) {
            roughness = clamp(orm.g, 0.04, 1.0);
            metallic = clamp(orm.b, 0.0, 1.0);
            ao = orm.r;
        }
    }
    if (material.flags.w > 0.5) {
        ao = ao * textureSample(ao_texture, tex_sampler, in.uv).r;
    }

    var out: GbufferOut;
    out.position = vec4<f32>(in.world, 0.0);
    out.normal = vec4<f32>(normal, 0.0);
    out.albedo = vec4<f32>(albedo.rgb, 1.0);
    out.specular = vec4<f32>(material.extra3.x, metallic, roughness, ao);
    return out;
}
