// Shared helpers: blend modes, color conversions, normal combination.
// Prepended to every compute shader; must not declare bindings.

const PI: f32 = 3.14159265359;
const EPS: f32 = 1e-5;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn rgb2hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = select(vec4<f32>(c.bg, k.wz), vec4<f32>(c.gb, k.xy), c.b <= c.g);
    let q = select(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), p.x <= c.r);
    let d = q.x - min(q.w, q.y);
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10)), d / (q.x + 1e-10), q.x);
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
    let p = abs(fract(c.xxx + vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return c.z * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

fn soft_light(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let d = select(sqrt(max(a, vec3<f32>(0.0))), ((16.0 * a - 12.0) * a + 4.0) * a, a <= vec3<f32>(0.25));
    let lo = a - (1.0 - 2.0 * b) * a * (1.0 - a);
    let hi = a + (2.0 * b - 1.0) * (d - a);
    return select(hi, lo, b <= vec3<f32>(0.5));
}

fn color_burn(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return select(1.0 - min(vec3<f32>(1.0), (1.0 - a) / max(b, vec3<f32>(EPS))), vec3<f32>(0.0), b <= vec3<f32>(0.0));
}

fn color_dodge(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return select(min(vec3<f32>(1.0), a / max(1.0 - b, vec3<f32>(EPS))), vec3<f32>(1.0), b >= vec3<f32>(1.0));
}

// Blend modes, in the order of `BlendMode::ALL` on the Rust side.
fn blend3(mode: u32, a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 2u: { return a * b; }                                            // Multiply
        case 3u: { return a / max(b, vec3<f32>(EPS)); }                       // Divide
        case 4u: { return b / max(a, vec3<f32>(EPS)); }                       // Inverse divide
        case 5u: { return min(a, b); }                                        // Darken
        case 6u: { return max(a, b); }                                        // Lighten
        case 7u: { return a + b; }                                            // Linear dodge (add)
        case 8u: { return a - b; }                                            // Subtract
        case 9u: { return b - a; }                                            // Inverse subtract
        case 10u: { return abs(a - b); }                                      // Difference
        case 11u: { return a + b - 2.0 * a * b; }                             // Exclusion
        case 12u: { return a + 2.0 * b - 1.0; }                               // Signed addition
        case 13u: {                                                           // Overlay
            return select(1.0 - 2.0 * (1.0 - a) * (1.0 - b), 2.0 * a * b, a < vec3<f32>(0.5));
        }
        case 14u: { return 1.0 - (1.0 - a) * (1.0 - b); }                     // Screen
        case 15u: { return a + b - 1.0; }                                     // Linear burn
        case 16u: { return color_burn(a, b); }                                // Color burn
        case 17u: { return color_dodge(a, b); }                               // Color dodge
        case 18u: { return soft_light(a, b); }                                // Soft light
        case 19u: {                                                           // Hard light
            return select(1.0 - 2.0 * (1.0 - a) * (1.0 - b), 2.0 * a * b, b < vec3<f32>(0.5));
        }
        case 20u: {                                                           // Vivid light
            return select(color_dodge(a, 2.0 * (b - 0.5)), color_burn(a, 2.0 * b), b < vec3<f32>(0.5));
        }
        case 21u: {                                                           // Pin light
            return select(max(a, 2.0 * (b - 0.5)), min(a, 2.0 * b), b < vec3<f32>(0.5));
        }
        case 22u: {                                                           // Saturation
            let ha = rgb2hsv(a);
            return hsv2rgb(vec3<f32>(ha.x, rgb2hsv(b).y, ha.z));
        }
        case 23u: {                                                           // Color
            let hb = rgb2hsv(b);
            return hsv2rgb(vec3<f32>(hb.x, hb.y, rgb2hsv(a).z));
        }
        case 24u: {                                                           // Value
            let ha = rgb2hsv(a);
            return hsv2rgb(vec3<f32>(ha.x, ha.y, rgb2hsv(b).z));
        }
        case 25u: {                                                           // Hue
            let ha = rgb2hsv(a);
            return hsv2rgb(vec3<f32>(rgb2hsv(b).x, ha.y, ha.z));
        }
        default: { return b; }                                                // Normal / Replace
    }
}

fn blend1(mode: u32, a: f32, b: f32) -> f32 {
    return blend3(mode, vec3<f32>(a), vec3<f32>(b)).x;
}

// Reoriented normal mapping: detail applied on top of base (tangent space).
fn rnm(base: vec3<f32>, detail: vec3<f32>) -> vec3<f32> {
    let t = base + vec3<f32>(0.0, 0.0, 1.0);
    let u = detail * vec3<f32>(-1.0, -1.0, 1.0);
    return normalize(t * dot(t, u) / max(t.z, EPS) - u);
}

fn safe_normalize(v: vec3<f32>) -> vec3<f32> {
    let l = length(v);
    if l < 1e-8 {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    return v / l;
}

// 26 = normal map combine (RNM), 27 = normal map detail (whiteout).
fn blend_normal(mode: u32, a: vec3<f32>, b: vec3<f32>, alpha: f32) -> vec3<f32> {
    let na = safe_normalize(a);
    let nb = safe_normalize(b);
    if mode == 26u {
        return safe_normalize(mix(na, rnm(na, nb), alpha));
    }
    if mode == 27u {
        return safe_normalize(mix(na, vec3<f32>(na.xy + nb.xy, na.z * nb.z), alpha));
    }
    return safe_normalize(mix(na, nb, alpha));
}

fn rotate2(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

fn levels(v: f32, in_lo: f32, in_hi: f32, gamma: f32, out_lo: f32, out_hi: f32) -> f32 {
    let t = clamp((v - in_lo) / max(in_hi - in_lo, EPS), 0.0, 1.0);
    return mix(out_lo, out_hi, pow(t, 1.0 / max(gamma, 0.01)));
}

// ---------------------------------------------------------------- layer blending

// Accumulated material: col = (base rgb, opacity), mrh = (rough, metal, height, 0),
// emi = (emissive rgb, 0), nrm = (tangent normal xyz, 0).
struct Acc {
    col: vec4<f32>,
    mrh: vec4<f32>,
    emi: vec4<f32>,
    nrm: vec4<f32>,
};

// Straight layer values with per-channel coverage.
struct Sample {
    col: vec3<f32>,
    a_col: f32,
    // rough, metal, height
    val: vec3<f32>,
    // coverage of rough, metal, height, opacity
    a_val: vec4<f32>,
    emi: vec3<f32>,
    a_emi: f32,
    nrm: vec3<f32>,
    a_nrm: f32,
    opa: f32,
    // Optional channels: specular level, transmission (normal blending).
    ext: vec2<f32>,
    a_ext: vec2<f32>,
};

// Blends one layer sample over the accumulator. mode/op: color, rough,
// metal, height | emissive, opacity, normal.
fn blend_layer(a: Acc, s: Sample, m: f32, mode_a: vec4<u32>, mode_b: vec4<u32>, op_a: vec4<f32>, op_b: vec4<f32>) -> Acc {
    let w0 = clamp(s.a_col * m * op_a.x, 0.0, 1.0);
    let w1 = clamp(s.a_val.x * m * op_a.y, 0.0, 1.0);
    let w2 = clamp(s.a_val.y * m * op_a.z, 0.0, 1.0);
    let w3 = clamp(s.a_val.z * m * op_a.w, 0.0, 1.0);
    let w4 = clamp(s.a_emi * m * op_b.x, 0.0, 1.0);
    let w5 = clamp(s.a_val.w * m * op_b.y, 0.0, 1.0);
    let w6 = clamp(s.a_nrm * m * op_b.z, 0.0, 1.0);
    var o: Acc;
    let c = mix(a.col.rgb, blend3(mode_a.x, a.col.rgb, s.col), w0);
    let op = mix(a.col.a, blend1(mode_b.y, a.col.a, s.opa), w5);
    o.col = vec4<f32>(c, op);
    o.mrh = vec4<f32>(
        mix(a.mrh.x, blend1(mode_a.y, a.mrh.x, s.val.x), w1),
        mix(a.mrh.y, blend1(mode_a.z, a.mrh.y, s.val.y), w2),
        mix(a.mrh.z, blend1(mode_a.w, a.mrh.z, s.val.z), w3),
        mix(a.mrh.w, s.ext.x, clamp(s.a_ext.x * m * op_b.w, 0.0, 1.0))
    );
    o.emi = vec4<f32>(mix(a.emi.rgb, blend3(mode_b.x, a.emi.rgb, s.emi), w4), mix(a.emi.w, s.ext.y, clamp(s.a_ext.y * m * op_b.w, 0.0, 1.0)));
    var n = a.nrm.xyz;
    if w6 > 0.0 {
        n = blend_normal(mode_b.z, a.nrm.xyz, s.nrm, w6);
    }
    o.nrm = vec4<f32>(n, a.nrm.w);
    return o;
}

// Texel of a map that may be a 1x1 constant (mesh maps not baked yet,
// paint textures not painted yet).
fn load_clamped(t: texture_2d<f32>, p: vec2<i32>) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(t));
    return textureLoad(t, min(p, d - vec2<i32>(1)), 0);
}
