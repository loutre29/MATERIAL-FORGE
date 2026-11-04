// Layer stack passes. This file is split by the Rust side at the
// "//// ENTRY" markers: each section becomes its own module (one entry point
// per module so automatic bind group layouts include every binding).
//
// Accumulator slot (7 x rgba16f, see `Acc` in common.wgsl): col, mrh, emi,
// nrm, then x0..x2, the optional channel groups (1x1 while the set uses
// none of them: their reads and writes then do nothing that matters).
// Layer content (paint textures or a fill evaluation): col = (rgb*a, a),
// val = (r, m, h premultiplied, 0), alp = (a_r, a_m, a_h, a_o),
// emi = (rgb*a, a), nrm = (xyz*a, a), opa = (opacity * a_o).
//
// Passes run per tile (see tile.wgsl): scratch textures (accumulators,
// masks) are tile local, set textures (paint, mesh maps, finals) global.

//// ENTRY clear
struct ClearParams { col: vec4<f32>, mrh: vec4<f32>, emi: vec4<f32>, nrm: vec4<f32>, x0: vec4<f32>, x1: vec4<f32>, x2: vec4<f32> };
@group(0) @binding(0) var<uniform> C: ClearParams;
@group(0) @binding(1) var o_col: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var o_mrh: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var o_nrm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var o_x0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(6) var o_x1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(7) var o_x2: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    textureStore(o_col, id, C.col);
    textureStore(o_mrh, id, C.mrh);
    textureStore(o_emi, id, C.emi);
    textureStore(o_nrm, id, C.nrm);
    textureStore(o_x0, id, C.x0);
    textureStore(o_x1, id, C.x1);
    textureStore(o_x2, id, C.x2);
}

//// ENTRY blend
struct BlendParams {
    mode_a: vec4<u32>,   // color, roughness, metallic, height
    mode_b: vec4<u32>,   // emissive, opacity, normal, has_mask
    op_a: vec4<f32>,
    op_b: vec4<f32>,
};
@group(0) @binding(0) var<uniform> P: BlendParams;
@group(0) @binding(1) var a_col: texture_2d<f32>;
@group(0) @binding(2) var a_mrh: texture_2d<f32>;
@group(0) @binding(3) var a_emi: texture_2d<f32>;
@group(0) @binding(4) var a_nrm: texture_2d<f32>;
@group(0) @binding(5) var c_col: texture_2d<f32>;
@group(0) @binding(6) var c_val: texture_2d<f32>;
@group(0) @binding(7) var c_alp: texture_2d<f32>;
@group(0) @binding(8) var c_emi: texture_2d<f32>;
@group(0) @binding(9) var c_nrm: texture_2d<f32>;
@group(0) @binding(10) var c_opa: texture_2d<f32>;
@group(0) @binding(11) var mask: texture_2d<f32>;
@group(0) @binding(12) var o_col: texture_storage_2d<rgba16float, write>;
@group(0) @binding(13) var o_mrh: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(15) var o_nrm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(16) var a_x0: texture_2d<f32>;
@group(0) @binding(17) var a_x1: texture_2d<f32>;
@group(0) @binding(18) var a_x2: texture_2d<f32>;
@group(0) @binding(19) var o_x0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(20) var o_x1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(21) var o_x2: texture_storage_2d<rgba16float, write>;

fn load_opt(t: texture_2d<f32>, id: vec2<i32>) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(t));
    if d.x <= 1 { return textureLoad(t, vec2<i32>(0), 0); }
    return textureLoad(t, id, 0);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    let gid = to_global(id);
    let ac = textureLoad(a_col, id, 0);
    let am = textureLoad(a_mrh, id, 0);
    let ae = textureLoad(a_emi, id, 0);
    let an = textureLoad(a_nrm, id, 0);
    let cc = load_clamped(c_col, gid);
    let cv = load_clamped(c_val, gid);
    let ca = load_clamped(c_alp, gid);
    let ce = load_opt(c_emi, gid);
    let cn = load_opt(c_nrm, gid);
    let co = load_opt(c_opa, gid).r;
    var m = load_opt(mask, id).r;
    if P.mode_b.w == 0u { m = 1.0; }

    var smp: Sample;
    smp.col = cc.rgb / max(cc.a, EPS);
    smp.a_col = cc.a;
    smp.val = cv.xyz / max(ca.xyz, vec3<f32>(EPS));
    smp.a_val = ca;
    smp.emi = ce.rgb / max(ce.a, EPS);
    smp.a_emi = ce.a;
    smp.nrm = cn.xyz / max(cn.a, EPS);
    smp.a_nrm = cn.a;
    smp.opa = co / max(ca.w, EPS);
    // Paint layers do not write the optional groups (smp.a_x stays 0).
    let ax = array<vec4<f32>, 3>(textureLoad(a_x0, id, 0), textureLoad(a_x1, id, 0), textureLoad(a_x2, id, 0));
    let o = blend_layer(Acc(ac, am, ae, an, ax), smp, m, P.mode_a, P.mode_b, P.op_a, P.op_b);
    textureStore(o_col, id, o.col);
    textureStore(o_mrh, id, o.mrh);
    textureStore(o_emi, id, o.emi);
    textureStore(o_nrm, id, o.nrm);
    textureStore(o_x0, id, o.x[0]);
    textureStore(o_x1, id, o.x[1]);
    textureStore(o_x2, id, o.x[2]);
}

//// ENTRY folder
struct FolderParams { op_a: vec4<f32>, op_b: vec4<f32>, flags: vec4<u32> };
@group(0) @binding(0) var<uniform> P: FolderParams;
@group(0) @binding(1) var b_col: texture_2d<f32>;
@group(0) @binding(2) var b_mrh: texture_2d<f32>;
@group(0) @binding(3) var b_emi: texture_2d<f32>;
@group(0) @binding(4) var b_nrm: texture_2d<f32>;
@group(0) @binding(5) var f_col: texture_2d<f32>;
@group(0) @binding(6) var f_mrh: texture_2d<f32>;
@group(0) @binding(7) var f_emi: texture_2d<f32>;
@group(0) @binding(8) var f_nrm: texture_2d<f32>;
@group(0) @binding(9) var mask: texture_2d<f32>;
@group(0) @binding(10) var o_col: texture_storage_2d<rgba16float, write>;
@group(0) @binding(11) var o_mrh: texture_storage_2d<rgba16float, write>;
@group(0) @binding(12) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(13) var o_nrm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var b_x0: texture_2d<f32>;
@group(0) @binding(15) var b_x1: texture_2d<f32>;
@group(0) @binding(16) var b_x2: texture_2d<f32>;
@group(0) @binding(17) var f_x0: texture_2d<f32>;
@group(0) @binding(18) var f_x1: texture_2d<f32>;
@group(0) @binding(19) var f_x2: texture_2d<f32>;
@group(0) @binding(20) var o_x0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(21) var o_x1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(22) var o_x2: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    var m = 1.0;
    let md = vec2<i32>(textureDimensions(mask));
    if P.flags.x == 1u && md.x > 1 { m = textureLoad(mask, id, 0).r; }
    let wx = clamp(P.op_b.w * m, 0.0, 1.0);
    textureStore(o_x0, id, mix(textureLoad(b_x0, id, 0), textureLoad(f_x0, id, 0), wx));
    textureStore(o_x1, id, mix(textureLoad(b_x1, id, 0), textureLoad(f_x1, id, 0), wx));
    textureStore(o_x2, id, mix(textureLoad(b_x2, id, 0), textureLoad(f_x2, id, 0), wx));
    let bc = textureLoad(b_col, id, 0);
    let bm = textureLoad(b_mrh, id, 0);
    let be = textureLoad(b_emi, id, 0);
    let bn = textureLoad(b_nrm, id, 0);
    let fc = textureLoad(f_col, id, 0);
    let fm = textureLoad(f_mrh, id, 0);
    let fe = textureLoad(f_emi, id, 0);
    let fnn = textureLoad(f_nrm, id, 0);
    let c = mix(bc.rgb, fc.rgb, clamp(P.op_a.x * m, 0.0, 1.0));
    let o = mix(bc.a, fc.a, clamp(P.op_b.y * m, 0.0, 1.0));
    let r = mix(bm.x, fm.x, clamp(P.op_a.y * m, 0.0, 1.0));
    let mt = mix(bm.y, fm.y, clamp(P.op_a.z * m, 0.0, 1.0));
    let h = mix(bm.z, fm.z, clamp(P.op_a.w * m, 0.0, 1.0));
    let e = mix(be.rgb, fe.rgb, clamp(P.op_b.x * m, 0.0, 1.0));
    let n = safe_normalize(mix(bn.xyz, fnn.xyz, clamp(P.op_b.z * m, 0.0, 1.0)));
    textureStore(o_col, id, vec4<f32>(c, o));
    let we = clamp(P.op_b.w * m, 0.0, 1.0);
    textureStore(o_mrh, id, vec4<f32>(r, mt, h, mix(bm.w, fm.w, we)));
    textureStore(o_emi, id, vec4<f32>(e, mix(be.w, fe.w, we)));
    textureStore(o_nrm, id, vec4<f32>(n, mix(bn.w, fnn.w, clamp(P.op_a.w * m, 0.0, 1.0))));
}

//// ENTRY filter
struct FilterParams {
    // kind, channel bits, has_mask, unused
    k: vec4<u32>,
    op_a: vec4<f32>,
    op_b: vec4<f32>,
    p0: vec4<f32>,
    p1: vec4<f32>,
    color: vec4<f32>,
    color2: vec4<f32>,
    // Curves: RGB, red, green, blue; 8 (x, y) points each, two per vec4,
    // x < 0 past the last one.
    curve: array<vec4<f32>, 16>,
};
@group(0) @binding(0) var<uniform> P: FilterParams;

fn curve_pt(c: u32, i: u32) -> vec2<f32> {
    let v = P.curve[c * 4u + i / 2u];
    return select(v.xy, v.zw, (i & 1u) == 1u);
}

fn curve_sec(c: u32, i: u32) -> f32 {
    let a = curve_pt(c, i);
    let b = curve_pt(c, i + 1u);
    return (b.y - a.y) / max(b.x - a.x, 1e-5);
}

fn curve_slope(c: u32, i: u32, n: u32) -> f32 {
    if i == 0u { return curve_sec(c, 0u); }
    if i == n - 1u { return curve_sec(c, n - 2u); }
    let a = curve_sec(c, i - 1u);
    let b = curve_sec(c, i);
    if a * b <= 0.0 { return 0.0; }
    let lim = 3.0 * min(abs(a), abs(b));
    return clamp((a + b) * 0.5, -lim, lim);
}

// Tone curve `c` at v: monotone cubic between the points, flat past the
// ends (document.rs `curve_eval`).
fn curve(c: u32, v: f32) -> f32 {
    var n = 0u;
    for (var i = 0u; i < 8u; i++) {
        if curve_pt(c, i).x < 0.0 { break; }
        n = i + 1u;
    }
    if n == 0u { return v; }
    let first = curve_pt(c, 0u);
    let last = curve_pt(c, n - 1u);
    if v <= first.x { return first.y; }
    if v >= last.x { return last.y; }
    var i = 0u;
    loop {
        if i + 2u >= n || v <= curve_pt(c, i + 1u).x { break; }
        i += 1u;
    }
    let p0 = curve_pt(c, i);
    let p1 = curve_pt(c, i + 1u);
    let h = max(p1.x - p0.x, 1e-5);
    let t = (v - p0.x) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    return (2.0 * t3 - 3.0 * t2 + 1.0) * p0.y + (t3 - 2.0 * t2 + t) * h * curve_slope(c, i, n)
        + (-2.0 * t3 + 3.0 * t2) * p1.y + (t3 - t2) * h * curve_slope(c, i + 1u, n);
}
@group(0) @binding(1) var a_col: texture_2d<f32>;
@group(0) @binding(2) var a_mrh: texture_2d<f32>;
@group(0) @binding(3) var a_emi: texture_2d<f32>;
@group(0) @binding(4) var a_nrm: texture_2d<f32>;
@group(0) @binding(5) var l_col: texture_2d<f32>;
@group(0) @binding(6) var l_mrh: texture_2d<f32>;
@group(0) @binding(7) var l_emi: texture_2d<f32>;
@group(0) @binding(8) var l_nrm: texture_2d<f32>;
@group(0) @binding(9) var mask: texture_2d<f32>;
@group(0) @binding(10) var o_col: texture_storage_2d<rgba16float, write>;
@group(0) @binding(11) var o_mrh: texture_storage_2d<rgba16float, write>;
@group(0) @binding(12) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(13) var o_nrm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var a_x0: texture_2d<f32>;
@group(0) @binding(15) var a_x1: texture_2d<f32>;
@group(0) @binding(16) var a_x2: texture_2d<f32>;
@group(0) @binding(17) var o_x0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(18) var o_x1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(19) var o_x2: texture_storage_2d<rgba16float, write>;

// Filter on a color value. `blurred` is the low-passed value (blur/sharpen).
fn f_color(c: vec3<f32>, blurred: vec3<f32>) -> vec3<f32> {
    switch P.k.x {
        // HSL: hue shift, saturation, lightness
        case 0u: {
            var h = rgb2hsv(max(c, vec3<f32>(0.0)));
            h.x = fract(h.x + P.p0.x);
            h.y = clamp(h.y * P.p0.y, 0.0, 1.0);
            h.z = max(h.z + P.p0.z, 0.0);
            return hsv2rgb(h);
        }
        // Brightness / contrast
        case 1u: { return (c - 0.5) * P.p0.y + 0.5 + P.p0.x; }
        // Levels
        case 2u: {
            return vec3<f32>(
                levels(c.r, P.p0.x, P.p0.y, P.p0.z, P.p0.w, P.p1.x),
                levels(c.g, P.p0.x, P.p0.y, P.p0.z, P.p0.w, P.p1.x),
                levels(c.b, P.p0.x, P.p0.y, P.p0.z, P.p0.w, P.p1.x)
            );
        }
        case 3u: { return 1.0 - c; }                                              // Invert
        case 4u: { let s = max(P.p0.x, 2.0); return floor(c * s) / (s - 1.0); }   // Posterize
        case 5u: { return blurred; }                                              // Blur
        case 6u: { return c + (c - blurred) * P.p0.x; }                           // Sharpen
        case 7u: { return mix(c, c * P.color.rgb, P.p0.x); }                      // Tint
        case 8u: { return mix(P.color.rgb, P.color2.rgb, clamp(luminance(c), 0.0, 1.0)); } // Gradient map
        case 9u: { return vec3<f32>(luminance(c)); }                               // Desaturate
        case 10u: { return clamp(c, vec3<f32>(P.p0.x), vec3<f32>(P.p0.y)); }      // Clamp
        case 12u: { return vec3<f32>(hist_scan(luminance(c))); }                  // Histogram scan
        case 13u: { return c - blurred + 0.5; }                                    // Highpass
        // Color correct: brightness, contrast, gamma, saturation, hue
        case 14u: {
            var o = (c - 0.5) * P.p0.y + 0.5 + P.p0.x;
            o = pow(max(o, vec3<f32>(0.0)), vec3<f32>(1.0 / max(P.p0.z, 0.01)));
            var h = rgb2hsv(max(o, vec3<f32>(0.0)));
            h.x = fract(h.x + P.p1.x);
            h.y = clamp(h.y * P.p0.w, 0.0, 1.0);
            return hsv2rgb(h);
        }
        // Curves: each channel's, then the RGB one
        case 17u: { return vec3<f32>(curve(0u, curve(1u, c.r)), curve(0u, curve(2u, c.g)), curve(0u, curve(3u, c.b))); }
        default: { return c; }
    }
}

// Histogram scan: a threshold at `position` with a soft edge set by `contrast`.
fn hist_scan(v: f32) -> f32 {
    let w = mix(0.5, 0.002, clamp(P.p0.y, 0.0, 1.0));
    return smoothstep(P.p0.x - w, P.p0.x + w, v);
}

fn f_scalar(v: f32, blurred: f32) -> f32 {
    switch P.k.x {
        case 1u: { return (v - 0.5) * P.p0.y + 0.5 + P.p0.x; }
        case 2u: { return levels(v, P.p0.x, P.p0.y, P.p0.z, P.p0.w, P.p1.x); }
        case 3u: { return 1.0 - v; }
        case 4u: { let s = max(P.p0.x, 2.0); return floor(v * s) / (s - 1.0); }
        case 5u: { return blurred; }
        case 6u: { return v + (v - blurred) * P.p0.x; }
        case 10u: { return clamp(v, P.p0.x, P.p0.y); }
        case 11u: { return v * P.p0.x + P.p0.y; }          // Height adjust: scale, offset
        case 12u: { return hist_scan(v); }
        case 13u: { return v - blurred + 0.5; }
        case 14u: { return pow(max((v - 0.5) * P.p0.y + 0.5 + P.p0.x, 0.0), 1.0 / max(P.p0.z, 0.01)); }
        case 17u: { return curve(0u, v); }
        default: { return v; }
    }
}

fn on(bit: u32) -> bool {
    return (P.k.y & (1u << bit)) != 0u;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    // Mirror / transform run untiled (the tile is the whole texture).
    let d = T.size.xy;
    var m = 1.0;
    let md = vec2<i32>(textureDimensions(mask));
    if P.k.z == 1u && md.x > 1 { m = textureLoad(mask, id, 0).r; }
    let ac = textureLoad(a_col, id, 0);
    let am = textureLoad(a_mrh, id, 0);
    let ae = textureLoad(a_emi, id, 0);
    let an = textureLoad(a_nrm, id, 0);
    let lc = textureLoad(l_col, id, 0);
    let lm = textureLoad(l_mrh, id, 0);
    let le = textureLoad(l_emi, id, 0);
    let ln = textureLoad(l_nrm, id, 0);
    // Filters leave the optional groups as they are.
    textureStore(o_x0, id, textureLoad(a_x0, id, 0));
    textureStore(o_x1, id, textureLoad(a_x1, id, 0));
    textureStore(o_x2, id, textureLoad(a_x2, id, 0));

    // Mirror / transform: the stack resampled at other texels (UV space).
    if P.k.x == 15u || P.k.x == 16u {
        let dims = vec2<f32>(d);
        var uv = (vec2<f32>(id) + 0.5) / dims;
        if P.k.x == 15u {
            if P.p0.x < 0.5 || P.p0.x > 1.5 { uv.x = 1.0 - uv.x; }
            if P.p0.x > 0.5 { uv.y = 1.0 - uv.y; }
        } else {
            let q = rotate2(uv - 0.5 - P.p0.xy, -radians(P.p0.z)) / max(P.p0.w, 0.01);
            uv = fract(q + 0.5);
        }
        let sid = clamp(vec2<i32>(uv * dims), vec2<i32>(0), vec2<i32>(d) - vec2<i32>(1));
        let tc = textureLoad(a_col, sid, 0);
        let tm = textureLoad(a_mrh, sid, 0);
        let te = textureLoad(a_emi, sid, 0);
        let tn = textureLoad(a_nrm, sid, 0);
        let w = clamp(P.op_a.x * m, 0.0, 1.0);
        var oc = ac;
        if on(0u) { oc = vec4<f32>(mix(ac.rgb, tc.rgb, w), oc.a); }
        if on(5u) { oc.a = mix(ac.a, tc.a, w); }
        var om = am;
        if on(1u) { om.x = mix(am.x, tm.x, w); }
        if on(2u) { om.y = mix(am.y, tm.y, w); }
        if on(3u) { om.z = mix(am.z, tm.z, w); }
        var oe = ae;
        if on(4u) { oe = vec4<f32>(mix(ae.rgb, te.rgb, w), ae.w); }
        var on_ = an;
        if on(6u) { on_ = vec4<f32>(safe_normalize(mix(an.xyz, tn.xyz, w)), an.w); }
        textureStore(o_col, id, oc);
        textureStore(o_mrh, id, om);
        textureStore(o_emi, id, oe);
        textureStore(o_nrm, id, on_);
        return;
    }

    var c = ac.rgb;
    if on(0u) { c = mix(ac.rgb, f_color(ac.rgb, lc.rgb), clamp(P.op_a.x * m, 0.0, 1.0)); }
    var r = am.x;
    if on(1u) { r = mix(am.x, f_scalar(am.x, lm.x), clamp(P.op_a.y * m, 0.0, 1.0)); }
    var mt = am.y;
    if on(2u) { mt = mix(am.y, f_scalar(am.y, lm.y), clamp(P.op_a.z * m, 0.0, 1.0)); }
    var h = am.z;
    if on(3u) { h = mix(am.z, f_scalar(am.z, lm.z), clamp(P.op_a.w * m, 0.0, 1.0)); }
    var e = ae.rgb;
    if on(4u) { e = mix(ae.rgb, f_color(ae.rgb, le.rgb), clamp(P.op_b.x * m, 0.0, 1.0)); }
    var o = ac.a;
    if on(5u) { o = mix(ac.a, f_scalar(ac.a, lc.a), clamp(P.op_b.y * m, 0.0, 1.0)); }
    var n = an.xyz;
    if on(6u) {
        var target_n = ln.xyz;
        if P.k.x == 6u {
            target_n = an.xyz + (an.xyz - ln.xyz) * P.p0.x;
        } else if P.k.x != 5u {
            // Other filters edit the normal map colors (x, y encoded in
            // 0..1, like a normal texture); z is rebuilt so the normal
            // stays in the upper hemisphere.
            let e = f_color(safe_normalize(an.xyz) * 0.5 + 0.5, safe_normalize(ln.xyz) * 0.5 + 0.5);
            let xy = clamp(e.xy * 2.0 - 1.0, vec2<f32>(-1.0), vec2<f32>(1.0));
            target_n = vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
        }
        n = safe_normalize(mix(an.xyz, target_n, clamp(P.op_b.z * m, 0.0, 1.0)));
    }
    textureStore(o_col, id, vec4<f32>(c, o));
    textureStore(o_mrh, id, vec4<f32>(r, mt, h, am.w));
    textureStore(o_emi, id, vec4<f32>(e, ae.w));
    textureStore(o_nrm, id, vec4<f32>(n, an.w));
}

//// ENTRY accblur
// Separable Gaussian over an accumulator slot, weighted by UV coverage so
// island borders do not bleed into the gutters.
struct BlurParams { dir: vec4<i32>, k: vec4<f32> };   // dir.xy step, k.x radius (texels)
@group(0) @binding(0) var<uniform> B: BlurParams;
@group(0) @binding(1) var i_col: texture_2d<f32>;
@group(0) @binding(2) var i_mrh: texture_2d<f32>;
@group(0) @binding(3) var i_emi: texture_2d<f32>;
@group(0) @binding(4) var i_nrm: texture_2d<f32>;
@group(0) @binding(5) var nearest: texture_2d<u32>;
@group(0) @binding(6) var o_col: texture_storage_2d<rgba16float, write>;
@group(0) @binding(7) var o_mrh: texture_storage_2d<rgba16float, write>;
@group(0) @binding(8) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(9) var o_nrm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var i_x0: texture_2d<f32>;
@group(0) @binding(11) var i_x1: texture_2d<f32>;
@group(0) @binding(12) var i_x2: texture_2d<f32>;
@group(0) @binding(13) var o_x0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var o_x1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(15) var o_x2: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = T.size.xy;
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    let r = i32(clamp(B.k.x, 1.0, 64.0));
    let sigma = max(B.k.x * 0.5, 0.5);
    var s0 = vec4<f32>(0.0);
    var s1 = vec4<f32>(0.0);
    var s2 = vec4<f32>(0.0);
    var s3 = vec4<f32>(0.0);
    var s4 = vec4<f32>(0.0);
    var s5 = vec4<f32>(0.0);
    var s6 = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = -r; k <= r; k++) {
        let p = clamp(id + B.dir.xy * k, vec2<i32>(0), d - vec2<i32>(1));
        let pg = to_global(p);
        let n = vec2<i32>(textureLoad(nearest, pg, 0).xy);
        if !all(n == pg) && k != 0 { continue; }
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        s0 += textureLoad(i_col, p, 0) * w;
        s1 += textureLoad(i_mrh, p, 0) * w;
        s2 += textureLoad(i_emi, p, 0) * w;
        s3 += textureLoad(i_nrm, p, 0) * w;
        s4 += textureLoad(i_x0, p, 0) * w;
        s5 += textureLoad(i_x1, p, 0) * w;
        s6 += textureLoad(i_x2, p, 0) * w;
        wsum += w;
    }
    let inv = 1.0 / max(wsum, EPS);
    textureStore(o_col, id, s0 * inv);
    textureStore(o_mrh, id, s1 * inv);
    textureStore(o_emi, id, s2 * inv);
    textureStore(o_nrm, id, s3 * inv);
    textureStore(o_x0, id, s4 * inv);
    textureStore(o_x1, id, s5 * inv);
    textureStore(o_x2, id, s6 * inv);
}

//// ENTRY finalize
// Height becomes a tangent-space normal (RNM-combined with painted normals and
// the baked normal), baked AO is applied, gutters are filled from the nearest
// island texel (infinite dilation).
struct FinalParams { k: vec4<f32> };   // height scale, ao strength, optional groups on
@group(0) @binding(0) var<uniform> F: FinalParams;
@group(0) @binding(1) var a_col: texture_2d<f32>;
@group(0) @binding(2) var a_mrh: texture_2d<f32>;
@group(0) @binding(3) var a_emi: texture_2d<f32>;
@group(0) @binding(4) var a_nrm: texture_2d<f32>;
@group(0) @binding(5) var baked_normal: texture_2d<f32>;
@group(0) @binding(6) var baked_ao: texture_2d<f32>;
@group(0) @binding(7) var nearest: texture_2d<u32>;
@group(0) @binding(8) var o_base: texture_storage_2d<rgba16float, write>;
@group(0) @binding(9) var o_orm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var o_nrm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(11) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(12) var a_x0: texture_2d<f32>;
@group(0) @binding(13) var a_x1: texture_2d<f32>;
@group(0) @binding(14) var a_x2: texture_2d<f32>;
@group(0) @binding(15) var o_x0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(16) var o_x1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(17) var o_x2: texture_storage_2d<rgba16float, write>;

// p: global texel; the accumulators are read in the tile's scratch space.
fn covered(p: vec2<i32>, dims: vec2<i32>) -> bool {
    if any(p < vec2<i32>(0)) || any(p >= dims) { return false; }
    let n = vec2<i32>(textureLoad(nearest, p, 0).xy);
    return all(n == p);
}

// Height whose relief shows: the hidden part (a Replace above it) is left out.
fn height_at(p: vec2<i32>, fallback: f32, dims: vec2<i32>) -> f32 {
    if covered(p, dims) {
        let q = p - T.scratch.xy;
        return textureLoad(a_mrh, q, 0).z - textureLoad(a_nrm, q, 0).w;
    }
    return fallback;
}

// Dispatched over the texels whose nearest covered texel (themselves when
// covered, the island texel they dilate from in the gutters) lies in this
// tile's owned rect; every texel of the finals has exactly one owner.
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let dims = T.size.zw;
    let l = vec2<i32>(g.xy);
    if !in_tile(l) { return; }
    let id = l + T.origin.xy;
    if any(id >= dims) { return; }
    let src = vec2<i32>(textureLoad(nearest, id, 0).xy);
    if any(src < T.inner.xy) || any(src >= T.inner.zw) { return; }
    let ls = src - T.scratch.xy;
    let c = textureLoad(a_col, ls, 0);
    let m = textureLoad(a_mrh, ls, 0);
    let e = textureLoad(a_emi, ls, 0);
    let an = textureLoad(a_nrm, ls, 0);
    let pn = safe_normalize(an.xyz);
    let h = m.z;
    let hv = h - an.w;
    let hl = height_at(src + vec2<i32>(-1, 0), hv, dims);
    let hr = height_at(src + vec2<i32>(1, 0), hv, dims);
    let hu = height_at(src + vec2<i32>(0, -1), hv, dims);
    let hd = height_at(src + vec2<i32>(0, 1), hv, dims);
    let k = F.k.x * f32(dims.x);
    let nh = normalize(vec3<f32>(-(hr - hl) * 0.5 * k, -(hu - hd) * 0.5 * k, 1.0));
    let nb = safe_normalize(load_clamped(baked_normal, src).xyz);
    let n = rnm(rnm(nb, pn), nh);
    var ao = mix(1.0, load_clamped(baked_ao, src).r, F.k.y);
    // The optional groups (their finals are 1x1 while unused).
    if F.k.z > 0.5 {
        let x0 = clamp(textureLoad(a_x0, ls, 0), vec4<f32>(0.0), vec4<f32>(1.0));
        textureStore(o_x0, id, x0);
        textureStore(o_x1, id, clamp(textureLoad(a_x1, ls, 0), vec4<f32>(0.0), vec4<f32>(1.0)));
        textureStore(o_x2, id, clamp(textureLoad(a_x2, ls, 0), vec4<f32>(0.0), vec4<f32>(1.0)));
        // The painted ambient occlusion darkens the baked one.
        ao *= x0.x;
    }
    textureStore(o_base, id, vec4<f32>(max(c.rgb, vec3<f32>(0.0)), clamp(c.a, 0.0, 1.0)));
    textureStore(o_orm, id, vec4<f32>(ao, clamp(m.x, 0.0, 1.0), clamp(m.y, 0.0, 1.0), h));
    // Optional channels ride in the spare .w: transmission with the normal,
    // specular level with the emissive.
    textureStore(o_nrm, id, vec4<f32>(n, clamp(e.w, 0.0, 1.0)));
    textureStore(o_emi, id, vec4<f32>(max(e.rgb, vec3<f32>(0.0)), clamp(m.w, 0.0, 1.0)));
}

//// ENTRY mip
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = vec2<i32>(textureDimensions(dst));
    let l = vec2<i32>(g.xy);
    if !in_tile(l) { return; }
    let id = l + T.origin.xy;
    if id.x >= d.x || id.y >= d.y { return; }
    let sd = vec2<i32>(textureDimensions(src)) - vec2<i32>(1);
    let p = id * 2;
    let s = textureLoad(src, min(p, sd), 0) + textureLoad(src, min(p + vec2<i32>(1, 0), sd), 0)
        + textureLoad(src, min(p + vec2<i32>(0, 1), sd), 0) + textureLoad(src, min(p + vec2<i32>(1, 1), sd), 0);
    textureStore(dst, id, s * 0.25);
}
