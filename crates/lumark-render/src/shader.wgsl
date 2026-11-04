// Lumark GPU 着色器：近似 PBR（Studio 双灯）与 FModel 头灯（Headlight）两种光照。
// 输出到 Rgba8Unorm 目标，因此这里手动做 tone mapping 与 sRGB 编码。
//
// 光照模式与法线约定通过 camera.ambient 的 z/w 分量传入（见 camera_uniform）：
//   ambient.z = 0 → Studio，1 → Headlight，2 → Snooper；
//   ambient.w = 0 → UE 切线约定，1 → FModel Snooper 约定。
// ORM 贴图布局通过 material.flags.z 传入：0 → R=AO/G=粗糙度/B=金属度，
// 1 → 高光遮罩（G=金属度、B=粗糙度）。

const PI: f32 = 3.141592653589793;

struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
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
    shadow_params: vec4<f32>,
    shadow_splits: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var shadow_texture: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

struct Material {
    base_color: vec4<f32>,
    // x = metallic, y = roughness, z = has base texture, w = has normal texture
    params: vec4<f32>,
    // xyz = emissive, w = has emissive texture
    emissive: vec4<f32>,
    // x = has ORM/spec texture, y = alpha cutoff, z = ORM 布局（0=ORM，1=高光遮罩），w = has AO
    flags: vec4<f32>,
    // x = roughness_min, y = roughness_max, z = emissive_multiplier, w = has_ao_boost
    extra: vec4<f32>,
    // xyz = ao_boost_color, w = ao_boost_exponent
    extra2: vec4<f32>,
    // x = Snooper 高光强度标量
    extra3: vec4<f32>,
};

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

// 解析材质 PBR 参数：x=环境光遮蔽、y=粗糙度、z=金属度。
fn resolve_pbr_params(uv: vec2<f32>) -> vec3<f32> {
    var occlusion = 1.0;
    var roughness = material.params.y;
    var metallic = material.params.x;
    if (material.flags.x > 0.5) {
        let packed = textureSample(orm_texture, tex_sampler, uv).rgb;
        if (material.flags.z > 0.5) {
            // 高光遮罩布局（FModel specular mask）：G=金属度、B=粗糙度。
            occlusion = 1.0;
            roughness = clamp(roughness * packed.b, 0.04, 1.0);
            metallic = clamp(packed.g, 0.0, 1.0);
        } else {
            // ORM：R=遮蔽、G=粗糙度、B=金属度。
            occlusion = packed.r;
            roughness = clamp(roughness * packed.g, 0.04, 1.0);
            metallic = clamp(metallic * packed.b, 0.0, 1.0);
        }
    }
    return vec3<f32>(occlusion, roughness, metallic);
}

// 切线空间法线贴图 → 世界法线；按 camera.ambient.w 选择切线约定。
fn apply_normal_map(
    normal_in: vec3<f32>,
    tangent_in: vec3<f32>,
    tangent_w: f32,
    uv: vec2<f32>,
) -> vec3<f32> {
    let sampled = textureSample(normal_texture, tex_sampler, uv).xyz * 2.0 - vec3<f32>(1.0);
    var tangent = normalize(tangent_in);
    var bitangent: vec3<f32>;
    if (camera.ambient.w < 0.5) {
        // UE 约定：切线正交化，副切线带 w 符号。
        let tangent_dot = dot(normal_in, tangent);
        tangent = normalize(tangent - normal_in * tangent_dot);
        bitangent = cross(normal_in, tangent) * sign(tangent_w);
    } else {
        // FModel Snooper 约定：副切线固定取 -cross(N, T)。
        bitangent = -normalize(cross(normal_in, tangent));
    }
    return normalize(tangent * sampled.x + bitangent * sampled.y + normal_in * sampled.z);
}

// FModel default.frag 头灯高光/漫反射项：G=金属度/腔体、B=粗糙度。
fn fmodel_headlight(
    base: vec3<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    metallic: f32,
    roughness: f32,
    cavity: f32,
) -> vec3<f32> {
    let n_dot_v = max(dot(normal, view), 0.0);
    if (n_dot_v <= 0.0) {
        return vec3<f32>(0.0);
    }
    let n_dot_l = n_dot_v;
    let f0 = mix(vec3<f32>(0.04), base, metallic);
    let alpha2 = roughness * roughness * roughness * roughness;
    let denominator = n_dot_v * n_dot_v * (alpha2 - 1.0) + 1.0;
    let distribution = alpha2 / (PI * denominator * denominator);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let geometry =
        (n_dot_l / (n_dot_l * (1.0 - k) + k)) * (n_dot_v / (n_dot_v * (1.0 - k) + k));
    let specular_denominator = 4.0 * n_dot_v * n_dot_l + 0.0001;
    // 头灯下 H 与 V 同向，H·V=1，Schlick 退化为 F0。
    let specular = distribution * geometry * f0 / specular_denominator;
    let kd = (vec3<f32>(1.0) - f0) * (1.0 - cavity);
    let diffuse_brdf = kd * base / PI;
    return (diffuse_brdf + specular) * n_dot_l;
}

// FModel 头灯参数：x=cavity、y=roughness、z=metallic。
fn resolve_fmodel_params(uv: vec2<f32>) -> vec3<f32> {
    var spec = vec3<f32>(0.0);
    if (material.flags.x > 0.5) {
        spec = textureSample(orm_texture, tex_sampler, uv).rgb;
    }
    var metallic = spec.g;
    var cavity = spec.g;
    var roughness = material.extra.x;
    if (material.flags.x > 0.5) {
        roughness = mix(material.extra.x, material.extra.y, spec.b);
    }
    if (material.flags.w > 0.5) {
        cavity = textureSample(ao_texture, tex_sampler, uv).g;
    }
    return vec3<f32>(clamp(cavity, 0.0, 1.0), clamp(roughness, 0.0, 1.0), clamp(metallic, 0.0, 1.0));
}

// Snooper 材质参数：x=高光强度、y=金属度、z=粗糙度。
fn resolve_snooper_params(uv: vec2<f32>) -> vec3<f32> {
    var specular = clamp(material.extra3.x, 0.0, 1.0);
    var metallic = clamp(material.params.x, 0.0, 1.0);
    var roughness = clamp(material.extra.y, 0.02, 1.0);
    if (material.flags.x > 0.5) {
        let sampled = textureSample(orm_texture, tex_sampler, uv).rgb;
        // AO 贴图的 R 通道不是高光；用材质 Specular 兜底。
        specular = max(sampled.r, material.extra3.x);
        metallic = clamp(sampled.g, 0.0, 1.0);
        roughness = clamp(mix(material.extra.x, material.extra.y, sampled.b), 0.02, 1.0);
    }
    return vec3<f32>(specular, metallic, roughness);
}

// Snooper pbr.glsl 的 GGX 分布（cross(N,H) 形式）。
fn snooper_distribution_ggx(normal: vec3<f32>, half_vector: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let n_dot_h = max(dot(normal, half_vector), 0.0);
    let cross_nh = cross(normal, half_vector);
    let k = a / (dot(cross_nh, cross_nh) + n_dot_h * a * n_dot_h * a);
    return k * k / PI;
}

fn snooper_geometry_schlick(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn snooper_geometry_smith(
    normal: vec3<f32>,
    view: vec3<f32>,
    light: vec3<f32>,
    roughness: f32,
) -> f32 {
    let n_dot_v = max(dot(normal, view), 0.0);
    let n_dot_l = max(dot(normal, light), 0.0);
    return snooper_geometry_schlick(n_dot_v, roughness)
        * snooper_geometry_schlick(n_dot_l, roughness);
}

// Snooper scene_lighting.glsl：天空上下半球环境光 + 单方向太阳光。
fn snooper_lighting(
    base: vec3<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    specular: f32,
    metallic: f32,
    roughness: f32,
    ao: f32,
    shadow: f32,
) -> vec3<f32> {
    let n_dot_v = max(dot(normal, view), 0.001);
    let light = normalize(camera.light0_dir.xyz);
    let n_dot_l = max(dot(normal, light), 0.0);
    let half_vector = normalize(light + view);
    let h_dot_v = max(dot(half_vector, view), 0.0);

    // F0 = mix(0.08 * specular, albedo, metallic)。
    let f0 = mix(vec3<f32>(0.08 * specular), base, vec3<f32>(metallic));
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - h_dot_v, 5.0);
    let distribution = snooper_distribution_ggx(normal, half_vector, roughness);
    let geometry = snooper_geometry_smith(normal, view, light, roughness);
    let specular_denominator = 4.0 * n_dot_v * n_dot_l + 0.001;

    let sky_mix = clamp(normal.z * 0.5 + 0.5, 0.0, 1.0);
    let sky = mix(camera.sky_lower.rgb, camera.sky_upper.rgb, sky_mix);
    let ambient = sky * base * ao;
    let specular_term = distribution * geometry * fresnel / specular_denominator;
    let kd = (vec3<f32>(1.0) - fresnel) * (1.0 - metallic);
    let diffuse = kd * base / PI;
    let sun = (diffuse + specular_term)
        * camera.light0_color.rgb
        * n_dot_l
        * camera.light0_dir.w
        * shadow;
    return ambient + sun;
}

// Snooper 只做 gamma，不做 Reinhard 高光压缩。
fn encode_gamma(color: vec3<f32>, alpha: f32) -> vec4<f32> {
    let non_negative = max(color, vec3<f32>(0.0));
    return vec4<f32>(pow(non_negative, vec3<f32>(1.0 / 2.2)), clamp(alpha, 0.0, 1.0));
}

// 线性辐射度 → 基础色/自发光输出（tone mapping + gamma），同时保留 alpha
// 供半透明混合使用。
fn encode(color: vec3<f32>, alpha: f32) -> vec4<f32> {
    let non_negative = max(color, vec3<f32>(0.0));
    let mapped = non_negative / (vec3<f32>(1.0) + non_negative);
    return vec4<f32>(pow(mapped, vec3<f32>(1.0 / 2.2)), clamp(alpha, 0.0, 1.0));
}

// 太阳阴影：两级联正交阴影贴图 + 3x3 PCF，未启用时返回全亮。
fn shadow_factor(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    if (camera.shadow_params.x < 0.5) {
        return 1.0;
    }
    let position = world + normal * max(camera.shadow_params.w, 0.5);
    let distance = length(camera.eye.xyz - world);
    var clip: vec4<f32>;
    var layer: i32;
    if (distance < camera.shadow_splits.x) {
        clip = camera.shadow_view_proj_0 * vec4<f32>(position, 1.0);
        layer = 0;
    } else {
        clip = camera.shadow_view_proj_1 * vec4<f32>(position, 1.0);
        layer = 1;
    }
    if (clip.w <= 0.0) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if (ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 || ndc.z >= 1.0) {
        return 1.0;
    }
    let uv = ndc.xy * 0.5 + vec2<f32>(0.5);
    let reference = ndc.z - 0.0015;
    let texel = 1.0 / max(camera.shadow_params.y, 1.0);
    var lit = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            lit = lit + textureSampleCompare(
                shadow_texture,
                shadow_sampler,
                uv + vec2<f32>(f32(x), f32(y)) * texel,
                layer,
                reference,
            );
        }
    }
    return lit / 9.0;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    var normal = normalize(in.normal);
    if (!front_facing) {
        normal = -normal;
    }
    // FModel 默认不乘顶点色；查看器“顶点色”开关把标志放在 camera.eye.w。
    var tint = vec4<f32>(1.0);
    if (camera.eye.w > 0.5) {
        tint = in.color;
    }
    var base = material.base_color.rgb;
    var alpha = material.base_color.a * tint.a;
    if (material.params.z > 0.5) {
        var sampled = textureSample(base_texture, tex_sampler, in.uv);
        var sampled_rgb = sampled.rgb;
        if (material.extra3.y > 0.5) {
            // 折射眼睛：eyeBro 是虹膜，黑色底不是巩膜；在虹膜半径内混合，
            // 半径外使用 ScleraColor(EyeW) 的白色巩膜。
            let sclera = textureSample(sclera_texture, tex_sampler, in.uv).rgb;
            let distance_to_center = distance(in.uv, vec2<f32>(0.5, 0.5));
            let iris_mask = 1.0 - smoothstep(0.28, 0.34, distance_to_center);
            sampled_rgb = mix(sclera, sampled_rgb, iris_mask);
        }
        base = base * sampled_rgb;
        alpha = alpha * sampled.a;
    }
    base = base * tint.rgb;
    alpha = alpha * tint.a;
    // Masked 材质按 material.flags.y 阈值裁剪；Opaque 材质阈值为 0，忽略贴图 alpha。
    if (material.flags.y > 0.0 && alpha < material.flags.y) {
        discard;
    }
    let view = normalize(camera.eye.xyz - in.world);
    // 着色模式：y=0 Lit、1 Unlit、2 Normals。
    if (camera.ambient.y > 1.5) {
        if (material.params.w > 0.5) {
            normal = apply_normal_map(normal, in.tangent.xyz, in.tangent.w, in.uv);
        }
        if (dot(normal, view) < 0.0) {
            normal = -normal;
        }
        return vec4<f32>(normal * 0.5 + vec3<f32>(0.5), 1.0);
    }
    if (camera.ambient.y > 0.5) {
        return encode(base, alpha);
    }
    if (material.params.w > 0.5) {
        normal = apply_normal_map(normal, in.tangent.xyz, in.tangent.w, in.uv);
    }
    if (camera.ambient.z > 1.5) {
        // Snooper：方向光太阳 + 天空环境光；材质 AO 近似 SSAO，输出只做 gamma。
        let params = resolve_snooper_params(in.uv);
        var ao = 1.0;
        if (material.flags.w > 0.5) {
            ao = textureSample(ao_texture, tex_sampler, in.uv).r;
        }
        let shadow = shadow_factor(in.world, normal);
        var color = snooper_lighting(base, normal, view, params.x, params.y, params.z, ao, shadow);
        if (camera.fog_color.w > 0.5) {
            // UE 场景单位为厘米，Snooper 雾参数按米换算。
            let distance = length(camera.eye.xyz - in.world) / 100.0;
            let density = camera.fog_params.x
                * exp(-camera.fog_params.y * max(in.world.z / 100.0, 0.0));
            let amount = clamp(
                1.0 - exp(-density * max(distance - camera.fog_params.w, 0.0)),
                0.0,
                camera.fog_params.z,
            );
            color = mix(color, camera.fog_color.rgb, amount);
        }
        return encode_gamma(color, alpha);
    }
    var color: vec3<f32>;
    if (camera.ambient.z > 0.5) {
        // Headlight：严格按 FModel default.frag；AO 只衰减基础色，
        // 自发光在头灯前叠加，高光不乘 AO。
        let params = resolve_fmodel_params(in.uv);
        let diffuse_factor = max(max(dot(normal, view), 0.0), 0.4);
        var result = base * diffuse_factor;
        if (material.flags.w > 0.5) {
            let ao = textureSample(ao_texture, tex_sampler, in.uv).rgb;
            result = result * ao.r;
            if (material.extra.w > 0.5) {
                let boost = material.extra2.rgb * material.extra2.w;
                result = mix(result, result * boost, ao.b);
            }
        }
        if (material.emissive.w > 0.5) {
            result = result
                + material.emissive.rgb
                    * textureSample(emissive_texture, tex_sampler, in.uv).rgb
                    * material.extra.z;
        }
        color = result + fmodel_headlight(base, normal, view, params.z, params.y, params.x);
    } else {
        // Studio 保持既有观感：AO 先乘到基础色，法线朝视线方向翻转后走双灯。
        if (material.flags.w > 0.5) {
            let ao = textureSample(ao_texture, tex_sampler, in.uv);
            base = base * ao.r;
        }
        if (dot(normal, view) < 0.0) {
            normal = -normal;
        }
        let params = resolve_pbr_params(in.uv);
        let occlusion = params.x;
        let roughness = params.y;
        let metallic = params.z;
        let shininess = clamp(2.0 / pow(roughness, 4.0), 4.0, 2048.0);
        let specular_strength = 0.04 + metallic * 0.5;
        let ambient = camera.ambient.x * occlusion;
        color = base * ambient;
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
    }
    return encode(color, alpha);

}
