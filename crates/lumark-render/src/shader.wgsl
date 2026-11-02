// Lumark GPU 着色器：与 CPU 参考渲染器一致的近似 PBR + 双灯 + 法线/ORM/自发光贴图。
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
    // xyz = emissive, w = has emissive texture
    emissive: vec4<f32>,
    // x = has ORM texture, y = alpha cutoff, z/w 保留
    flags: vec4<f32>,
};

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_texture: texture_2d<f32>;
@group(1) @binding(3) var tex_sampler: sampler;
@group(1) @binding(4) var orm_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
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
    // 静态网格实例矩阵；蒙皮实例为单位矩阵。
    let instance = mat4x4<f32>(in.instance_c0, in.instance_c1, in.instance_c2, in.instance_c3);
    position = (instance * vec4<f32>(position, 1.0)).xyz;
    normal = normalize((instance * vec4<f32>(normal, 0.0)).xyz);
    tangent = vec4<f32>(normalize((instance * vec4<f32>(tangent.xyz, 0.0)).xyz), tangent.w);
    out.clip_position = camera.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    out.normal = normal;
    out.uv = in.uv;
    out.color = in.color;
    out.tangent = tangent;
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
    specular_strength: f32,
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
        + light_color * specular * specular_strength * intensity;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    var normal = normalize(in.normal);
    if (!front_facing) {
        normal = -normal;
    }
    var base = material.base_color.rgb;
    var alpha = material.base_color.a * in.color.a;
    if (material.params.z > 0.5) {
        let sampled = textureSample(base_texture, tex_sampler, in.uv);
        base = base * sampled.rgb;
        alpha = alpha * sampled.a;
    }
    base = base * in.color.rgb;
    // 首版 alpha 采用 0.5 阈值裁剪，与 CPU 后端一致。
    if (alpha < 0.5) {
        discard;
    }
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
    // 着色模式：y=0 Lit、1 Unlit、2 Normals。
    if (camera.ambient.y > 1.5) {
        return vec4<f32>(normal * 0.5 + vec3<f32>(0.5), 1.0);
    }
    if (camera.ambient.y > 0.5) {
        let non_negative = max(base, vec3<f32>(0.0));
        let mapped = non_negative / (vec3<f32>(1.0) + non_negative);
        return vec4<f32>(pow(mapped, vec3<f32>(1.0 / 2.2)), 1.0);
    }
    var metallic = material.params.x;
    var roughness = material.params.y;
    var occlusion = 1.0;
    if (material.flags.x > 0.5) {
        // ORM：R=环境光遮蔽、G=粗糙度、B=金属度（线性数据）。
        let orm = textureSample(orm_texture, tex_sampler, in.uv).rgb;
        occlusion = orm.r;
        roughness = roughness * orm.g;
        metallic = metallic * orm.b;
    }
    roughness = clamp(roughness, 0.04, 1.0);
    let shininess = clamp(2.0 / pow(roughness, 4.0), 4.0, 2048.0);
    let specular_strength = 0.04 + metallic * 0.5;
    let ambient = camera.ambient.x * occlusion;
    var color = base * ambient;
    color = color + light_contribution(
        normal,
        view,
        base,
        camera.light0_dir.xyz,
        camera.light0_color.rgb,
        camera.light0_dir.w,
        shininess,
        specular_strength,
    );
    color = color + light_contribution(
        normal,
        view,
        base,
        camera.light1_dir.xyz,
        camera.light1_color.rgb,
        camera.light1_dir.w,
        shininess,
        specular_strength,
    );
    var emissive = material.emissive.rgb;
    if (material.emissive.w > 0.5) {
        emissive = emissive * textureSample(emissive_texture, tex_sampler, in.uv).rgb;
    }
    color = color + emissive;
    let non_negative = max(color, vec3<f32>(0.0));
    let mapped = non_negative / (vec3<f32>(1.0) + non_negative);
    return vec4<f32>(pow(mapped, vec3<f32>(1.0 / 2.2)), 1.0);
}
