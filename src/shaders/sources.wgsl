// Fill layer content and mask effects. Split at "//// ENTRY" like stack.wgsl.
// Requires common.wgsl and procedural.wgsl.
//
// Projection modes: 0 UV, 1 triplanar, 2 planar X, 3 planar Y, 4 planar Z,
// 5 object 3D (procedurals only; textures fall back to triplanar).

//// ENTRY fill
struct ChanSrc {
    // source (0 uniform, 1 texture, 2 procedural), procedural kind, flags (1 srgb, 2 flip green), enabled
    k: vec4<u32>,
    a: vec4<f32>,
    b: vec4<f32>,
    pa: vec4<f32>,
    pb: vec4<f32>,
    // lod, texture slot, unused, unused
    t: vec4<f32>,
};
struct FillParams {
    // blend modes / opacities, as in the blend pass (mode_b.w = has mask)
    mode_a: vec4<u32>,
    mode_b: vec4<u32>,
    op_a: vec4<f32>,
    op_b: vec4<f32>,
    // mode, scale, rotation (rad), triplanar sharpness
    proj: vec4<f32>,
    // offset u, offset v, mirror u, mirror v
    offs: vec4<f32>,
    // position map space to projection space (affine rows), normal rotation rows
    x0: vec4<f32>,
    x1: vec4<f32>,
    x2: vec4<f32>,
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
    ch: array<ChanSrc, 9>,
};
@group(0) @binding(0) var<uniform> F: FillParams;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var t2: texture_2d<f32>;
@group(0) @binding(4) var t3: texture_2d<f32>;
@group(0) @binding(5) var t4: texture_2d<f32>;
@group(0) @binding(6) var t5: texture_2d<f32>;
@group(0) @binding(7) var t6: texture_2d<f32>;
@group(0) @binding(8) var samp: sampler;
@group(0) @binding(9) var b_pos: texture_2d<f32>;
@group(0) @binding(10) var b_wn: texture_2d<f32>;
@group(0) @binding(11) var a_col: texture_2d<f32>;
@group(0) @binding(12) var a_mrh: texture_2d<f32>;
@group(0) @binding(13) var a_emi: texture_2d<f32>;
@group(0) @binding(14) var a_nrm: texture_2d<f32>;
@group(0) @binding(15) var mask: texture_2d<f32>;
@group(0) @binding(16) var o_col: texture_storage_2d<rgba16float, write>;
@group(0) @binding(17) var o_mrh: texture_storage_2d<rgba16float, write>;
@group(0) @binding(18) var o_emi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(19) var o_nrm: texture_storage_2d<rgba16float, write>;

fn tex_at(slot: u32, uv: vec2<f32>, lod: f32) -> vec4<f32> {
    switch slot {
        case 0u: { return textureSampleLevel(t0, samp, uv, lod); }
        case 1u: { return textureSampleLevel(t1, samp, uv, lod); }
        case 2u: { return textureSampleLevel(t2, samp, uv, lod); }
        case 3u: { return textureSampleLevel(t3, samp, uv, lod); }
        case 4u: { return textureSampleLevel(t4, samp, uv, lod); }
        case 5u: { return textureSampleLevel(t5, samp, uv, lod); }
        default: { return textureSampleLevel(t6, samp, uv, lod); }
    }
}

// Mirror U / V signs (0 in old uniforms means no mirror).
fn fill_mirror() -> vec2<f32> {
    return vec2<f32>(select(1.0, F.offs.z, F.offs.z != 0.0), select(1.0, F.offs.w, F.offs.w != 0.0));
}

fn uv_xform(uv: vec2<f32>) -> vec2<f32> {
    return rotate2((uv - 0.5) * fill_mirror(), F.proj.z) * F.proj.y + 0.5 + F.offs.xy;
}

fn tri_weights(n: vec3<f32>) -> vec3<f32> {
    let w = pow(abs(n), vec3<f32>(max(F.proj.w, 1.0)));
    return w / max(w.x + w.y + w.z, EPS);
}

// The projection's 3D placement (viewport manipulator).
fn proj_pos(p: vec3<f32>) -> vec3<f32> {
    let h = vec4<f32>(p, 1.0);
    return vec3<f32>(dot(F.x0, h), dot(F.x1, h), dot(F.x2, h));
}

fn proj_nrm(n: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(F.n0.xyz, n), dot(F.n1.xyz, n), dot(F.n2.xyz, n));
}

// Samples a texture slot under the layer projection.
fn sample_proj(slot: u32, lod: f32, uv: vec2<f32>, pos_in: vec3<f32>, wn_in: vec3<f32>) -> vec4<f32> {
    let mode = u32(F.proj.x + 0.5);
    let s = F.proj.y;
    let pos = proj_pos(pos_in);
    let wn = proj_nrm(wn_in);
    switch mode {
        case 0u: { return tex_at(slot, uv_xform(uv), lod); }
        case 2u: { return tex_at(slot, pos.zy * s + F.offs.xy, lod); }
        case 3u: { return tex_at(slot, pos.xz * s + F.offs.xy, lod); }
        case 4u: { return tex_at(slot, pos.xy * s + F.offs.xy, lod); }
        default: {
            let w = tri_weights(wn);
            return tex_at(slot, pos.zy * s + F.offs.xy, lod) * w.x
                + tex_at(slot, pos.xz * s + F.offs.xy, lod) * w.y
                + tex_at(slot, pos.xy * s + F.offs.xy, lod) * w.z;
        }
    }
}

fn proc_coords(uv: vec2<f32>, pos_in: vec3<f32>) -> vec3<f32> {
    let mode = u32(F.proj.x + 0.5);
    let s = F.proj.y;
    let pos = proj_pos(pos_in);
    switch mode {
        case 0u: { return vec3<f32>(uv_xform(uv), 0.0); }
        case 2u: { return vec3<f32>(pos.zy * s + F.offs.xy, 0.0); }
        case 3u: { return vec3<f32>(pos.xz * s + F.offs.xy, 0.0); }
        case 4u: { return vec3<f32>(pos.xy * s + F.offs.xy, 0.0); }
        default: { return pos * s + vec3<f32>(F.offs.xy, 0.0); }
    }
}

// Value of channel c as rgb (scalars in .x). Returns w = coverage.
fn channel(c: u32, uv: vec2<f32>, pos: vec3<f32>, wn: vec3<f32>) -> vec4<f32> {
    let src = F.ch[c];
    if src.k.w == 0u { return vec4<f32>(0.0); }
    switch src.k.x {
        case 1u: {
            var t = sample_proj(u32(src.t.y), src.t.x, uv, pos, wn);
            if (src.k.z & 1u) != 0u { t = vec4<f32>(srgb_to_linear(t.rgb), t.a); }
            if c == 6u {
                var n = t.rgb * 2.0 - 1.0;
                if (src.k.z & 2u) != 0u { n.y = -n.y; }
                return vec4<f32>(mix(vec3<f32>(0.0, 0.0, 1.0), safe_normalize(n), src.a.x), 1.0);
            }
            // Generated procedurals (flag 4): the gray value ramps between the two values.
            if (src.k.z & 4u) != 0u {
                if c == 6u { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
                return vec4<f32>(mix(src.a.rgb, src.b.rgb, t.r), 1.0);
            }
            if c == 0u || c == 4u { return vec4<f32>(t.rgb * src.a.rgb, 1.0); }
            return vec4<f32>(mix(src.a.x, src.b.x, t.r), 0.0, 0.0, 1.0);
        }
        case 2u: {
            let p = proc_coords(uv, pos) * select(1.0, src.t.w, src.t.w > 0.0);
            let v = proc_eval(src.k.y, p, ProcParams(src.pa, src.pb));
            if c == 6u { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
            return vec4<f32>(mix(src.a.rgb, src.b.rgb, v), 1.0);
        }
        default: {
            if c == 6u { return vec4<f32>(safe_normalize(src.a.xyz), 1.0); }
            return vec4<f32>(src.a.rgb, 1.0);
        }
    }
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    let gid = to_global(id);
    let uv = (vec2<f32>(gid) + 0.5) / vec2<f32>(T.size.zw);
    let pos = textureLoad(b_pos, gid, 0).xyz;
    let wn = safe_normalize(textureLoad(b_wn, gid, 0).xyz);
    let col = channel(0u, uv, pos, wn);
    let r = channel(1u, uv, pos, wn);
    let m = channel(2u, uv, pos, wn);
    let h = channel(3u, uv, pos, wn);
    let e = channel(4u, uv, pos, wn);
    let o = channel(5u, uv, pos, wn);
    let n = channel(6u, uv, pos, wn);
    var smp: Sample;
    smp.col = col.rgb;
    smp.a_col = col.w;
    smp.val = vec3<f32>(r.x, m.x, h.x);
    smp.a_val = vec4<f32>(r.w, m.w, h.w, o.w);
    smp.emi = e.rgb;
    smp.a_emi = e.w;
    smp.nrm = n.xyz;
    smp.a_nrm = n.w;
    smp.opa = o.x;
    let x0 = channel(7u, uv, pos, wn);
    let x1 = channel(8u, uv, pos, wn);
    smp.ext = vec2<f32>(x0.x, x1.x);
    smp.a_ext = vec2<f32>(x0.w, x1.w);
    let md = vec2<i32>(textureDimensions(mask));
    var mk = 1.0;
    if F.mode_b.w == 1u && md.x > 1 { mk = textureLoad(mask, id, 0).r; }
    let acc = Acc(textureLoad(a_col, id, 0), textureLoad(a_mrh, id, 0), textureLoad(a_emi, id, 0), textureLoad(a_nrm, id, 0));
    let res_acc = blend_layer(acc, smp, mk, F.mode_a, F.mode_b, F.op_a, F.op_b);
    textureStore(o_col, id, res_acc.col);
    textureStore(o_mrh, id, res_acc.mrh);
    textureStore(o_emi, id, res_acc.emi);
    textureStore(o_nrm, id, res_acc.nrm);
}

//// ENTRY mask
// One mask effect: m_out = blend(m_in, effect, opacity).
struct MaskParams {
    // kind (0 clear, 1 paint, 2 fill, 3 generator, 4 levels, 5 invert, 6 color select,
    // 7 threshold), blend, generator kind, source (0 uniform 1 texture 2 procedural)
    k: vec4<u32>,
    // opacity, base/uniform value, texture lod, flags (1 srgb)
    o: vec4<f32>,
    // generator: level, contrast, invert, noise amount
    g0: vec4<f32>,
    // generator: noise scale, seed, axis, painted-height reactivity
    g1: vec4<f32>,
    // light direction xyz, color tolerance
    dir: vec4<f32>,
    // levels in lo, in hi, gamma, out lo
    lv: vec4<f32>,
    // out hi, threshold, unused, color count
    lv2: vec4<f32>,
    // fill value range (a, b), procedural kind, unused
    sv: vec4<f32>,
    pa: vec4<f32>,
    pb: vec4<f32>,
    // projection mode, scale, rotation, sharpness
    proj: vec4<f32>,
    offs: vec4<f32>,
    // projection placement, as in the fill pass
    x0: vec4<f32>,
    x1: vec4<f32>,
    x2: vec4<f32>,
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
    colors: array<vec4<f32>, 8>,
};
@group(0) @binding(0) var<uniform> M: MaskParams;
@group(0) @binding(1) var m_in: texture_2d<f32>;
@group(0) @binding(2) var paint: texture_2d<f32>;
@group(0) @binding(3) var tex: texture_2d<f32>;
@group(0) @binding(4) var samp: sampler;
@group(0) @binding(5) var b_ao: texture_2d<f32>;
@group(0) @binding(6) var b_curv: texture_2d<f32>;
@group(0) @binding(7) var b_pos: texture_2d<f32>;
@group(0) @binding(8) var b_wn: texture_2d<f32>;
@group(0) @binding(9) var b_thick: texture_2d<f32>;
@group(0) @binding(10) var b_id: texture_2d<f32>;
@group(0) @binding(11) var m_out: texture_storage_2d<r32float, write>;
// Stack accumulated below this layer: (roughness, metallic, height, 0).
@group(0) @binding(12) var acc_mrh: texture_2d<f32>;

fn mblend(mode: u32, a: f32, b: f32) -> f32 {
    switch mode {
        case 1u: { return a * b; }                       // Multiply
        case 2u: { return a + b; }                       // Add
        case 3u: { return a - b; }                       // Subtract
        case 4u: { return max(a, b); }                   // Lighten (max)
        case 5u: { return min(a, b); }                   // Darken (min)
        case 6u: { return 1.0 - (1.0 - a) * (1.0 - b); } // Screen
        case 7u: { return select(1.0 - 2.0 * (1.0 - a) * (1.0 - b), 2.0 * a * b, a < 0.5); } // Overlay
        case 8u: { return abs(a - b); }                  // Difference
        default: { return b; }                           // Normal
    }
}

fn m_pos(p: vec3<f32>) -> vec3<f32> {
    let h = vec4<f32>(p, 1.0);
    return vec3<f32>(dot(M.x0, h), dot(M.x1, h), dot(M.x2, h));
}

fn m_tex(uv: vec2<f32>, pos_in: vec3<f32>, wn_in: vec3<f32>) -> f32 {
    let mode = u32(M.proj.x + 0.5);
    let s = M.proj.y;
    let pos = m_pos(pos_in);
    let wn = vec3<f32>(dot(M.n0.xyz, wn_in), dot(M.n1.xyz, wn_in), dot(M.n2.xyz, wn_in));
    var t: vec4<f32>;
    switch mode {
        case 0u: { t = textureSampleLevel(tex, samp, rotate2((uv - 0.5) * vec2<f32>(select(1.0, M.offs.z, M.offs.z != 0.0), select(1.0, M.offs.w, M.offs.w != 0.0)), M.proj.z) * s + 0.5 + M.offs.xy, M.o.z); }
        case 2u: { t = textureSampleLevel(tex, samp, pos.zy * s + M.offs.xy, M.o.z); }
        case 3u: { t = textureSampleLevel(tex, samp, pos.xz * s + M.offs.xy, M.o.z); }
        case 4u: { t = textureSampleLevel(tex, samp, pos.xy * s + M.offs.xy, M.o.z); }
        default: {
            var w = pow(abs(wn), vec3<f32>(max(M.proj.w, 1.0)));
            w = w / max(w.x + w.y + w.z, EPS);
            t = textureSampleLevel(tex, samp, pos.zy * s + M.offs.xy, M.o.z) * w.x
                + textureSampleLevel(tex, samp, pos.xz * s + M.offs.xy, M.o.z) * w.y
                + textureSampleLevel(tex, samp, pos.xy * s + M.offs.xy, M.o.z) * w.z;
        }
    }
    var v = t.r;
    if (u32(M.o.w) & 1u) != 0u { v = srgb_to_linear(t.rgb).r; }
    return v;
}

fn m_proc(uv: vec2<f32>, pos_in: vec3<f32>) -> f32 {
    let mode = u32(M.proj.x + 0.5);
    let s = M.proj.y;
    let pos = m_pos(pos_in);
    var p: vec3<f32>;
    switch mode {
        case 0u: { p = vec3<f32>(rotate2((uv - 0.5) * vec2<f32>(select(1.0, M.offs.z, M.offs.z != 0.0), select(1.0, M.offs.w, M.offs.w != 0.0)), M.proj.z) * s + 0.5 + M.offs.xy, 0.0); }
        case 2u: { p = vec3<f32>(pos.zy * s + M.offs.xy, 0.0); }
        case 3u: { p = vec3<f32>(pos.xz * s + M.offs.xy, 0.0); }
        case 4u: { p = vec3<f32>(pos.xy * s + M.offs.xy, 0.0); }
        default: { p = pos * s + vec3<f32>(M.offs.xy, 0.0); }
    }
    return proc_eval(u32(M.sv.z + 0.5), p * select(1.0, M.sv.w, M.sv.w > 0.0), ProcParams(M.pa, M.pb));
}

// id: tile local, gid: global texel.
fn generator(id: vec2<i32>, gid: vec2<i32>, pos: vec3<f32>, wn: vec3<f32>) -> f32 {
    let ao = load_clamped(b_ao, gid).r;
    let curv = load_clamped(b_curv, gid).r;
    let thick = load_clamped(b_thick, gid).r;
    var c = (curv - 0.5) * 2.0;
    // Painted height below the layer adds its own convexity (raised details
    // read as edges, engraved ones as cavities).
    let ad = vec2<i32>(textureDimensions(acc_mrh));
    if M.g1.w > 0.0 && ad.x > 1 {
        let h = textureLoad(acc_mrh, id, 0).z;
        // Ring of 8 samples about 0.6% of the texture away.
        let r = max(f32(T.size.z) / 170.0, 2.0);
        var ring = 0.0;
        for (var k = 0; k < 8; k++) {
            let a = f32(k) * 0.785398;
            let o = vec2<i32>(vec2<f32>(cos(a), sin(a)) * r);
            ring += textureLoad(acc_mrh, clamp(id + o, vec2<i32>(0), T.size.xy - 1), 0).z;
        }
        let lap = h - ring / 8.0;
        c = clamp(c + lap * M.g1.w * 8.0, -1.0, 1.0);
    }
    var v = 0.0;
    switch M.k.z {
        case 0u: { v = max(c, 0.0); }                                   // Edges
        case 1u: { v = max(-c, 0.0); }                                  // Cavity
        case 2u: { v = 1.0 - ao; }                                      // Ambient occlusion
        case 3u: { v = (1.0 - ao) * 0.6 + max(-c, 0.0) * 0.6; }         // Dirt
        case 4u: { v = max(c, 0.0) * 1.2; }                             // Edge wear
        case 5u: {                                                      // Position gradient
            let ax = u32(M.g1.z + 0.5);
            v = select(select(pos.z, pos.y, ax == 1u), pos.x, ax == 0u);
        }
        case 6u: { v = dot(wn, safe_normalize(M.dir.xyz)) * 0.5 + 0.5; } // Directional light
        case 7u: { v = thick; }                                         // Thickness
        case 8u: { v = curv; }                                          // Curvature
        default: { v = 1.0; }
    }
    if M.g0.w > 0.0 {
        let n = fbm_value(pos * M.g1.x + vec3<f32>(M.g1.y * 17.3, M.g1.y * 5.1, M.g1.y * 9.7), 5, 0.5);
        v = v + (n - 0.5) * 2.0 * M.g0.w;
    }
    v = clamp((v - M.g0.x) * M.g0.y + 0.5, 0.0, 1.0);
    if M.g0.z > 0.5 { v = 1.0 - v; }
    return v;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    let gid = to_global(id);
    let uv = (vec2<f32>(gid) + 0.5) / vec2<f32>(T.size.zw);
    let md = vec2<i32>(textureDimensions(m_in));
    var m = textureLoad(m_in, min(id, md - vec2<i32>(1)), 0).r;
    let pos = textureLoad(b_pos, gid, 0).xyz;
    let wn = safe_normalize(textureLoad(b_wn, gid, 0).xyz);
    let pd = vec2<i32>(textureDimensions(paint));
    let pt = textureLoad(paint, min(gid, pd - vec2<i32>(1)), 0);
    let tv = m_tex(uv, pos, wn);
    let idc = load_clamped(b_id, gid).rgb;
    var e = m;
    var a = M.o.x;
    switch M.k.x {
        case 0u: { textureStore(m_out, id, vec4<f32>(M.o.y, 0.0, 0.0, 0.0)); return; }
        case 1u: {
            // Painted value with its own coverage
            e = mblend(M.k.y, m, pt.r / max(pt.g, EPS));
            a = M.o.x * clamp(pt.g, 0.0, 1.0);
            textureStore(m_out, id, vec4<f32>(mix(m, e, a), 0.0, 0.0, 0.0));
            return;
        }
        case 2u: {
            var v = M.o.y;
            if M.k.w == 1u { v = mix(M.sv.x, M.sv.y, tv); }
            if M.k.w == 2u { v = mix(M.sv.x, M.sv.y, m_proc(uv, pos)); }
            e = mblend(M.k.y, m, v);
        }
        case 3u: { e = mblend(M.k.y, m, generator(id, gid, pos, wn)); }
        case 4u: { e = levels(m, M.lv.x, M.lv.y, M.lv.z, M.lv.w, M.lv2.x); }
        case 5u: { e = 1.0 - m; }
        case 6u: {
            var sel = 0.0;
            let count = u32(M.lv2.w);
            for (var i = 0u; i < min(count, 8u); i++) {
                let dist = length(idc - M.colors[i].rgb);
                sel = max(sel, 1.0 - smoothstep(M.dir.w, M.dir.w + 0.02, dist));
            }
            e = mblend(M.k.y, m, sel);
        }
        case 7u: { e = select(0.0, 1.0, m >= M.lv2.y); }
        default: {}
    }
    textureStore(m_out, id, vec4<f32>(mix(m, e, clamp(a, 0.0, 1.0)), 0.0, 0.0, 0.0));
}

//// ENTRY maskblur
struct MBlur { dir: vec4<i32>, k: vec4<f32> };   // k.x radius, k.y opacity (vertical pass only)
@group(0) @binding(0) var<uniform> B: MBlur;
@group(0) @binding(1) var m_in: texture_2d<f32>;
@group(0) @binding(2) var m_orig: texture_2d<f32>;
@group(0) @binding(3) var nearest: texture_2d<u32>;
@group(0) @binding(4) var m_out: texture_storage_2d<r32float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = T.size.xy;
    let id = vec2<i32>(g.xy);
    if !in_tile(id) { return; }
    let r = i32(clamp(B.k.x, 1.0, 64.0));
    let sigma = max(B.k.x * 0.5, 0.5);
    var s = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k++) {
        let p = clamp(id + B.dir.xy * k, vec2<i32>(0), d - vec2<i32>(1));
        let pg = to_global(p);
        let n = vec2<i32>(textureLoad(nearest, pg, 0).xy);
        if !all(n == pg) && k != 0 { continue; }
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        s += textureLoad(m_in, p, 0).r * w;
        wsum += w;
    }
    var v = s / max(wsum, EPS);
    if B.dir.y != 0 {
        v = mix(textureLoad(m_orig, id, 0).r, v, B.k.y);
    }
    textureStore(m_out, id, vec4<f32>(v, 0.0, 0.0, 0.0));
}
