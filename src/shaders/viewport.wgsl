// Viewport: environment background + PBR metal/roughness shading with
// image-based lighting (prefiltered equirect mips + SH9 irradiance).
// Output is linear HDR; the post pass tonemaps it.

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // x: env rotation (rad), y: exposure multiplier, z: background blur lod, w: env max lod
    env: vec4<f32>,
    // x: display channel, y: background mode (0 env, 1 flat), z: shadow strength, w: background opacity
    display: vec4<f32>,
    sh: array<vec4<f32>, 9>,
    light_view_proj: mat4x4<f32>,
    // xyz direction toward the key light, w intensity
    light: vec4<f32>,
};

struct SetParams {
    // alpha mode (0 opaque, 1 test, 2 blend), cutoff, emissive intensity, display kind (0 gray, 1 rgb, 2 vector)
    k: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var env_tex: texture_2d<f32>;
@group(0) @binding(2) var env_samp: sampler;
@group(0) @binding(3) var shadow_map: texture_depth_2d;
@group(0) @binding(4) var shadow_samp: sampler_comparison;

@group(1) @binding(0) var t_base: texture_2d<f32>;
@group(1) @binding(1) var t_orm: texture_2d<f32>;
@group(1) @binding(2) var t_normal: texture_2d<f32>;
@group(1) @binding(3) var t_emi: texture_2d<f32>;
@group(1) @binding(4) var t_disp: texture_2d<f32>;
@group(1) @binding(5) var mat_samp: sampler;
@group(1) @binding(6) var<uniform> S: SetParams;

const PI: f32 = 3.14159265359;

fn env_space(d: vec3<f32>) -> vec3<f32> {
    let c = cos(cam.env.x);
    let s = sin(cam.env.x);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

fn equirect_uv(d: vec3<f32>) -> vec2<f32> {
    let u = atan2(d.x, -d.z) / (2.0 * PI) + 0.5;
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return vec2<f32>(u, v);
}

fn sample_env(d: vec3<f32>, lod: f32) -> vec3<f32> {
    return textureSampleLevel(env_tex, env_samp, equirect_uv(env_space(normalize(d))), lod).rgb;
}

fn sh_diffuse(n_world: vec3<f32>) -> vec3<f32> {
    let n = env_space(n_world);
    let x = n.x;
    let y = n.y;
    let z = n.z;
    var r = cam.sh[0].rgb * 0.282095;
    r += cam.sh[1].rgb * (0.488603 * y);
    r += cam.sh[2].rgb * (0.488603 * z);
    r += cam.sh[3].rgb * (0.488603 * x);
    r += cam.sh[4].rgb * (1.092548 * x * y);
    r += cam.sh[5].rgb * (1.092548 * y * z);
    r += cam.sh[6].rgb * (0.315392 * (3.0 * z * z - 1.0));
    r += cam.sh[7].rgb * (1.092548 * x * z);
    r += cam.sh[8].rgb * (0.546274 * (x * x - y * y));
    return max(r, vec3<f32>(0.0));
}

// Values shown as raw data: the post pass applies no tonemapping when the
// display channel is not the material (it only encodes to sRGB).
fn as_data(v: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2)), 1.0);
}

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: BgOut;
    o.pos = vec4<f32>(p, 1.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    let g = mix(vec3<f32>(0.018), vec3<f32>(0.06), in.ndc.y * 0.5 + 0.5);
    // Channel views skip the post exposure; the material view gets it.
    if cam.display.x > 0.5 {
        return vec4<f32>(g, 1.0);
    }
    if cam.display.y > 0.5 {
        return vec4<f32>(g / max(cam.env.y, 1e-4), 1.0);
    }
    let far = cam.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let near = cam.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let dir = normalize(far.xyz / far.w - near.xyz / near.w);
    let c = sample_env(dir, cam.env.z) * cam.display.w;
    return vec4<f32>(c, 1.0);
}

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
};

@vertex
fn vs_mesh(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = cam.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.uv = v.uv;
    o.tangent = v.tangent;
    return o;
}

fn shadow(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if cam.display.z <= 0.0 { return 1.0; }
    let p = cam.light_view_proj * vec4<f32>(world + n * 0.002 * cam.eye.w, 1.0);
    let c = p.xyz / p.w;
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || c.z > 1.0 { return 1.0; }
    let texel = 1.0 / f32(textureDimensions(shadow_map).x);
    var s = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            s += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(x), f32(y)) * texel * 1.5, c.z - 0.0015);
        }
    }
    return mix(1.0, s / 9.0, cam.display.z);
}

fn shade(in: VsOut, front: bool) -> vec4<f32> {
    let base4 = textureSample(t_base, mat_samp, in.uv);
    let base = base4.rgb;
    let orm = textureSample(t_orm, mat_samp, in.uv);
    let nts = textureSample(t_normal, mat_samp, in.uv).xyz;
    let emi4 = textureSample(t_emi, mat_samp, in.uv);
    let emi = emi4.rgb;
    // Optional channels (spare .w of the finals).
    let spec_level = emi4.a;
    let transmission = textureSample(t_normal, mat_samp, in.uv).a;
    let disp = textureSample(t_disp, mat_samp, in.uv);

    var ng = normalize(in.normal);
    if !front { ng = -ng; }
    var t = in.tangent.xyz - ng * dot(ng, in.tangent.xyz);
    if dot(t, t) < 1e-12 {
        t = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(ng.x) > 0.9);
        t = t - ng * dot(ng, t);
    }
    t = normalize(t);
    let b = cross(ng, t) * select(1.0, -1.0, in.tangent.w < 0.0);
    let n = normalize(t * nts.x + b * nts.y + ng * max(nts.z, 0.0001));

    let mode = u32(cam.display.x + 0.5);
    switch mode {
        case 1u: { return vec4<f32>(base, 1.0); }
        case 2u: { return as_data(vec3<f32>(orm.g)); }
        case 3u: { return as_data(vec3<f32>(orm.b)); }
        case 4u: { return as_data(normalize(nts) * 0.5 + 0.5); }
        case 5u: { return as_data(vec3<f32>(orm.a * 0.5 + 0.5)); }
        case 6u: { return as_data(vec3<f32>(orm.r)); }
        case 7u: { return vec4<f32>(emi, 1.0); }
        case 8u: { return as_data(vec3<f32>(base4.a)); }
        case 9u: {
            let c = floor(in.uv * 16.0);
            let k = fract((c.x + c.y) * 0.5) * 2.0;
            return as_data(mix(vec3<f32>(0.25), vec3<f32>(0.8), k) * (0.5 + 0.5 * max(dot(ng, normalize(cam.eye.xyz - in.world)), 0.0)));
        }
        case 11u: { return as_data(vec3<f32>(spec_level)); }
        case 12u: { return as_data(vec3<f32>(transmission)); }
        case 10u: {
            let kind = u32(S.k.w + 0.5);
            if kind == 1u { return as_data(disp.rgb); }
            if kind == 2u { return as_data(normalize(disp.xyz) * 0.5 + 0.5); }
            return as_data(vec3<f32>(disp.r));
        }
        default: {}
    }

    let v = normalize(cam.eye.xyz - in.world);
    let rough = clamp(orm.g, 0.03, 1.0);
    let metal = clamp(orm.b, 0.0, 1.0);
    let ao = orm.r;
    let f0 = mix(vec3<f32>(0.08 * spec_level), base, metal);
    let ndv = max(dot(n, v), 1e-4);

    // Analytical split-sum approximation (Karis, "Physically Based Shading on Mobile").
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    let spec_color = f0 * ab.x + ab.y;

    let refl = reflect(-v, n);
    let lod = sqrt(rough) * cam.env.w;
    let sh = shadow(in.world, ng);
    let spec = sample_env(refl, lod) * spec_color;
    let diffuse = base * (1.0 - metal) * sh_diffuse(n);
    let spec_occ = clamp(pow(ndv + ao, exp2(-16.0 * rough - 1.0)) - 1.0 + ao, 0.0, 1.0);

    // Key light (direction of the environment's brightest region) for shadows.
    let l = normalize(cam.light.xyz);
    let ndl = max(dot(n, l), 0.0);
    let h = normalize(l + v);
    let a2 = max(rough * rough * rough * rough, 1e-4);
    let ndh = max(dot(n, h), 0.0);
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let ggx = a2 / (PI * dd * dd);
    let vis = 0.25 / max(mix(ndl, 1.0, rough * 0.5) * mix(ndv, 1.0, rough * 0.5), 1e-3);
    let fres = f0 + (1.0 - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
    let key = (base * (1.0 - metal) / PI + ggx * vis * fres) * ndl * cam.light.w;

    let ambient_shadow = mix(1.0, sh, 0.6);
    var color = (diffuse * ao + spec * spec_occ) * ambient_shadow + key * sh * ao;
    // Transmission: the environment seen through the surface, tinted by the
    // base color, where the surface does not reflect.
    if transmission > 0.001 {
        let fr = clamp(spec_color.g, 0.0, 1.0);
        let through = sample_env(refract(-v, n, 1.0 / 1.45), lod) * base;
        color = mix(color, through * (1.0 - fr) + spec * spec_occ, transmission * (1.0 - metal));
    }
    color += emi * S.k.z;
    var alpha = 1.0;
    let amode = u32(S.k.x + 0.5);
    if amode == 2u { alpha = base4.a; }
    return vec4<f32>(color, alpha);
}

@fragment
fn fs_mesh(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = shade(in, front);
    let amode = u32(S.k.x + 0.5);
    if amode == 1u && textureSample(t_base, mat_samp, in.uv).a < S.k.y { discard; }
    return c;
}

// Depth of the (nearly) opaque texels of an alpha-blended set.
@fragment
fn fs_prepass(in: VsOut) -> @location(0) vec4<f32> {
    if textureSample(t_base, mat_samp, in.uv).a < 0.98 { discard; }
    return vec4<f32>(0.0);
}

// Depth-only pass for the key light shadow map.
@vertex
fn vs_shadow(v: VsIn) -> @builtin(position) vec4<f32> {
    return cam.light_view_proj * vec4<f32>(v.pos, 1.0);
}

// Wireframe overlay
@vertex
fn vs_wire(v: VsIn) -> @builtin(position) vec4<f32> {
    var p = cam.view_proj * vec4<f32>(v.pos, 1.0);
    p.z -= 0.0002 * p.w;
    return p;
}

@fragment
fn fs_wire() -> @location(0) vec4<f32> {
    return vec4<f32>(0.85, 0.85, 0.9, 0.35);
}
