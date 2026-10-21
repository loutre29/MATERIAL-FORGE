// Layer stack compositing, one dispatch per layer (ping-pong accumulators).
// Accumulators: color = base color rgb, mrh = (roughness, metallic, height).

struct LayerParams {
    // 0 paint, 1 fill
    kind: u32,
    has_mask: u32,
    generator: u32,
    pad0: u32,
    // per channel: color, roughness, metallic, height
    blend: vec4<u32>,
    opacity: vec4<f32>,
    fill_color: vec4<f32>,
    fill_values: vec4<f32>,
    enabled: vec4<f32>,
    // level, contrast, invert, scale
    gen_a: vec4<f32>,
    // seed, spare...
    gen_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: LayerParams;
@group(0) @binding(1) var acc_color_in: texture_2d<f32>;
@group(0) @binding(2) var acc_mrh_in: texture_2d<f32>;
@group(0) @binding(3) var acc_color_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var acc_mrh_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var l_color: texture_2d<f32>;
@group(0) @binding(6) var l_values: texture_2d<f32>;
@group(0) @binding(7) var l_alphas: texture_2d<f32>;
@group(0) @binding(8) var l_mask: texture_2d<f32>;
@group(0) @binding(9) var b_ao: texture_2d<f32>;
@group(0) @binding(10) var b_curv: texture_2d<f32>;
@group(0) @binding(11) var b_pos: texture_2d<f32>;

fn blend3(mode: u32, a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 1u: { return a * b; }
        case 2u: { return 1.0 - (1.0 - a) * (1.0 - b); }
        case 3u: {
            let lo = 2.0 * a * b;
            let hi = 1.0 - 2.0 * (1.0 - a) * (1.0 - b);
            return select(hi, lo, a < vec3<f32>(0.5));
        }
        case 4u: { return a + b; }
        case 5u: { return a - b; }
        case 6u: { return min(a, b); }
        case 7u: { return max(a, b); }
        case 8u: { return abs(a - b); }
        default: { return b; }
    }
}

fn blend1(mode: u32, a: f32, b: f32) -> f32 {
    return blend3(mode, vec3<f32>(a), vec3<f32>(b)).x;
}

fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let n000 = hash3(i);
    let n100 = hash3(i + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash3(i + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash3(i + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash3(i + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash3(i + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash3(i + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash3(i + vec3<f32>(1.0, 1.0, 1.0));
    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn fbm(p: vec3<f32>) -> f32 {
    var amp = 0.5;
    var sum = 0.0;
    var q = p;
    for (var o = 0; o < 5; o++) {
        sum += amp * vnoise(q);
        q = q * 2.03 + vec3<f32>(1.7, 9.2, 4.1);
        amp *= 0.5;
    }
    return sum / 0.96875;
}

fn generator(id: vec2<i32>) -> f32 {
    let g = P.generator;
    if g == 0u {
        return 1.0;
    }
    let curv = textureLoad(b_curv, id, 0).r;
    let ao = textureLoad(b_ao, id, 0).r;
    let pos = textureLoad(b_pos, id, 0).xyz;
    let c = (curv - 0.5) * 2.0;
    let seed = vec3<f32>(P.gen_b.x * 17.31, P.gen_b.x * 7.77, P.gen_b.x * 3.13);
    var v = 1.0;
    switch g {
        // Edges (convex curvature)
        case 1u: { v = max(c, 0.0); }
        // Cavity (concave curvature + occlusion)
        case 2u: { v = max(-c, 0.0) * 0.6 + (1.0 - ao) * 0.4; }
        // Ambient occlusion (dirt in occluded areas)
        case 3u: { v = 1.0 - ao; }
        // 3D noise on the object's position
        case 4u: { v = fbm(pos * P.gen_a.w + seed); }
        // Edge wear: edges broken up by noise
        case 5u: {
            let n = fbm(pos * P.gen_a.w + seed);
            v = max(c, 0.0) * (0.3 + 1.4 * n);
        }
        // Vertical gradient (bottom to top of the bounding box)
        case 6u: { v = pos.y; }
        default: { v = 1.0; }
    }
    v = clamp((v - P.gen_a.x) * P.gen_a.y + 0.5, 0.0, 1.0);
    if P.gen_a.z > 0.5 {
        v = 1.0 - v;
    }
    return v;
}

@compute @workgroup_size(8, 8)
fn cs_layer(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(acc_color_in);
    if gid.x >= dims.x || gid.y >= dims.y {
        return;
    }
    let id = vec2<i32>(gid.xy);
    let ac = textureLoad(acc_color_in, id, 0);
    let am = textureLoad(acc_mrh_in, id, 0);

    var col: vec3<f32>;
    var vals: vec3<f32>;
    var a: vec4<f32>;
    if P.kind == 0u {
        let c = textureLoad(l_color, id, 0);
        let v = textureLoad(l_values, id, 0);
        let al = textureLoad(l_alphas, id, 0);
        col = c.rgb / max(c.a, 1e-5);
        vals = v.xyz / max(al.xyz, vec3<f32>(1e-5));
        a = vec4<f32>(c.a, al.x, al.y, al.z);
    } else {
        col = P.fill_color.rgb;
        vals = P.fill_values.xyz;
        a = P.enabled;
    }
    var m = 1.0;
    if P.has_mask == 1u {
        m = textureLoad(l_mask, id, 0).r;
    }
    m *= generator(id);
    a = clamp(a * m * P.opacity, vec4<f32>(0.0), vec4<f32>(1.0));

    let out_c = mix(ac.rgb, blend3(P.blend.x, ac.rgb, col), a.x);
    let r = mix(am.x, blend1(P.blend.y, am.x, vals.x), a.y);
    let mt = mix(am.y, blend1(P.blend.z, am.y, vals.y), a.z);
    let h = mix(am.z, blend1(P.blend.w, am.z, vals.z), a.w);
    textureStore(acc_color_out, id, vec4<f32>(out_c, 1.0));
    textureStore(acc_mrh_out, id, vec4<f32>(r, mt, h, 1.0));
}
