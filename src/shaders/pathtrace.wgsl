// Progressive path tracer (render mode). One sample per pixel per frame,
// accumulated in a storage buffer. Materials come from the texture sets'
// final maps; lighting from the environment map.

struct PT {
    inv_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // width, height, frame index, max bounces
    size: vec4<u32>,
    // env rotation (rad), env intensity, flat background, background blur lod
    env: vec4<f32>,
    // emissive intensities of sets 0-3 and 4-7
    emi_a: vec4<f32>,
    emi_b: vec4<f32>,
    // alpha mode of sets 0-3 and 4-7 (0 opaque, 1 cutout, 2 blend)
    amode_a: vec4<f32>,
    amode_b: vec4<f32>,
    // alpha cutoff of sets 0-3 and 4-7
    cut_a: vec4<f32>,
    cut_b: vec4<f32>,
};

struct Node {
    mn: vec4<f32>,
    mx: vec4<f32>,
};

// p*.w = u, n*.w = v of each vertex; tng = triangle tangent (w = sign); info.x = set slot
struct Tri {
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
    tng: vec4<f32>,
    info: vec4<u32>,
};

@group(0) @binding(0) var<uniform> U: PT;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var<storage, read> tris: array<Tri>;
@group(0) @binding(3) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(4) var out_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var env_tex: texture_2d<f32>;
@group(0) @binding(6) var samp: sampler;
@group(0) @binding(7) var b0: texture_2d<f32>;
@group(0) @binding(8) var b1: texture_2d<f32>;
@group(0) @binding(9) var b2: texture_2d<f32>;
@group(0) @binding(10) var b3: texture_2d<f32>;
@group(0) @binding(11) var b4: texture_2d<f32>;
@group(0) @binding(12) var b5: texture_2d<f32>;
@group(0) @binding(13) var b6: texture_2d<f32>;
@group(0) @binding(14) var b7: texture_2d<f32>;
@group(0) @binding(15) var o0: texture_2d<f32>;
@group(0) @binding(16) var o1: texture_2d<f32>;
@group(0) @binding(17) var o2: texture_2d<f32>;
@group(0) @binding(18) var o3: texture_2d<f32>;
@group(0) @binding(19) var o4: texture_2d<f32>;
@group(0) @binding(20) var o5: texture_2d<f32>;
@group(0) @binding(21) var o6: texture_2d<f32>;
@group(0) @binding(22) var o7: texture_2d<f32>;
@group(0) @binding(23) var n0t: texture_2d<f32>;
@group(0) @binding(24) var n1t: texture_2d<f32>;
@group(0) @binding(25) var n2t: texture_2d<f32>;
@group(0) @binding(26) var n3t: texture_2d<f32>;
@group(0) @binding(27) var n4t: texture_2d<f32>;
@group(0) @binding(28) var n5t: texture_2d<f32>;
@group(0) @binding(29) var n6t: texture_2d<f32>;
@group(0) @binding(30) var n7t: texture_2d<f32>;
@group(0) @binding(31) var e0: texture_2d<f32>;
@group(0) @binding(32) var e1: texture_2d<f32>;
@group(0) @binding(33) var e2: texture_2d<f32>;
@group(0) @binding(34) var e3: texture_2d<f32>;
@group(0) @binding(35) var e4: texture_2d<f32>;
@group(0) @binding(36) var e5: texture_2d<f32>;
@group(0) @binding(37) var e6: texture_2d<f32>;
@group(0) @binding(38) var e7: texture_2d<f32>;
// Denoiser guides of the first visible surface, averaged like the color:
// (albedo, depth), (normal, coverage); two entries per pixel.
@group(0) @binding(39) var<storage, read_write> aux: array<vec4<f32>>;

fn slot_vec(a: vec4<f32>, b: vec4<f32>, s: u32) -> f32 {
    if s < 4u { return a[s]; }
    if s < 8u { return b[s - 4u]; }
    return 0.0;
}

const PI: f32 = 3.14159265359;

fn tex_base(s: u32, uv: vec2<f32>) -> vec4<f32> {
    switch s {
        case 0u: { return textureSampleLevel(b0, samp, uv, 0.0); }
        case 1u: { return textureSampleLevel(b1, samp, uv, 0.0); }
        case 2u: { return textureSampleLevel(b2, samp, uv, 0.0); }
        case 3u: { return textureSampleLevel(b3, samp, uv, 0.0); }
        case 4u: { return textureSampleLevel(b4, samp, uv, 0.0); }
        case 5u: { return textureSampleLevel(b5, samp, uv, 0.0); }
        case 6u: { return textureSampleLevel(b6, samp, uv, 0.0); }
        case 7u: { return textureSampleLevel(b7, samp, uv, 0.0); }
        default: { return vec4<f32>(0.5, 0.5, 0.5, 1.0); }
    }
}

fn tex_orm(s: u32, uv: vec2<f32>) -> vec4<f32> {
    switch s {
        case 0u: { return textureSampleLevel(o0, samp, uv, 0.0); }
        case 1u: { return textureSampleLevel(o1, samp, uv, 0.0); }
        case 2u: { return textureSampleLevel(o2, samp, uv, 0.0); }
        case 3u: { return textureSampleLevel(o3, samp, uv, 0.0); }
        case 4u: { return textureSampleLevel(o4, samp, uv, 0.0); }
        case 5u: { return textureSampleLevel(o5, samp, uv, 0.0); }
        case 6u: { return textureSampleLevel(o6, samp, uv, 0.0); }
        case 7u: { return textureSampleLevel(o7, samp, uv, 0.0); }
        default: { return vec4<f32>(1.0, 0.5, 0.0, 0.0); }
    }
}

fn tex_nrm(s: u32, uv: vec2<f32>) -> vec4<f32> {
    switch s {
        case 0u: { return textureSampleLevel(n0t, samp, uv, 0.0); }
        case 1u: { return textureSampleLevel(n1t, samp, uv, 0.0); }
        case 2u: { return textureSampleLevel(n2t, samp, uv, 0.0); }
        case 3u: { return textureSampleLevel(n3t, samp, uv, 0.0); }
        case 4u: { return textureSampleLevel(n4t, samp, uv, 0.0); }
        case 5u: { return textureSampleLevel(n5t, samp, uv, 0.0); }
        case 6u: { return textureSampleLevel(n6t, samp, uv, 0.0); }
        case 7u: { return textureSampleLevel(n7t, samp, uv, 0.0); }
        default: { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
    }
}

fn tex_emi(s: u32, uv: vec2<f32>) -> vec3<f32> {
    var e = vec3<f32>(0.0);
    switch s {
        case 0u: { e = textureSampleLevel(e0, samp, uv, 0.0).rgb * U.emi_a.x; }
        case 1u: { e = textureSampleLevel(e1, samp, uv, 0.0).rgb * U.emi_a.y; }
        case 2u: { e = textureSampleLevel(e2, samp, uv, 0.0).rgb * U.emi_a.z; }
        case 3u: { e = textureSampleLevel(e3, samp, uv, 0.0).rgb * U.emi_a.w; }
        case 4u: { e = textureSampleLevel(e4, samp, uv, 0.0).rgb * U.emi_b.x; }
        case 5u: { e = textureSampleLevel(e5, samp, uv, 0.0).rgb * U.emi_b.y; }
        case 6u: { e = textureSampleLevel(e6, samp, uv, 0.0).rgb * U.emi_b.z; }
        case 7u: { e = textureSampleLevel(e7, samp, uv, 0.0).rgb * U.emi_b.w; }
        default: {}
    }
    return e;
}

// ---------------------------------------------------------------- rng

var<private> rng_state: u32;

fn pcg() -> u32 {
    let s = rng_state * 747796405u + 2891336453u;
    rng_state = s;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rnd() -> f32 {
    return f32(pcg() >> 8u) / 16777216.0;
}

// ---------------------------------------------------------------- environment

fn env(d: vec3<f32>, lod: f32) -> vec3<f32> {
    let c = cos(U.env.x);
    let s = sin(U.env.x);
    let r = vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
    let uv = vec2<f32>(atan2(r.x, -r.z) / (2.0 * PI) + 0.5, acos(clamp(r.y, -1.0, 1.0)) / PI);
    return textureSampleLevel(env_tex, samp, uv, lod).rgb * U.env.y;
}

// ---------------------------------------------------------------- traversal

struct Isect {
    t: f32,
    tri: u32,
    u: f32,
    v: f32,
    hit: bool,
};

fn slab(n: Node, o: vec3<f32>, inv: vec3<f32>, tmax: f32) -> f32 {
    let t0 = (n.mn.xyz - o) * inv;
    let t1 = (n.mx.xyz - o) * inv;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), max(min(t0.z, t1.z), 0.0));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), min(max(t0.z, t1.z), tmax));
    return select(1e30, tn, tn <= tf);
}

fn safe_inv(v: f32) -> f32 {
    if abs(v) < 1e-20 { return select(-1e30, 1e30, v >= 0.0); }
    return 1.0 / v;
}

fn trace(o: vec3<f32>, d: vec3<f32>, any_hit: bool) -> Isect {
    var best: Isect;
    best.hit = false;
    best.t = 1e30;
    let inv = vec3<f32>(safe_inv(d.x), safe_inv(d.y), safe_inv(d.z));
    var stack: array<u32, 48>;
    var sp = 1u;
    stack[0] = 0u;
    while sp > 0u {
        sp -= 1u;
        let n = nodes[stack[sp]];
        if slab(n, o, inv, best.t) >= 1e29 { continue; }
        let start = bitcast<u32>(n.mn.w);
        let count = bitcast<u32>(n.mx.w);
        if count > 0u {
            for (var k = start; k < start + count; k++) {
                let tr = tris[k];
                let e1 = tr.p1.xyz - tr.p0.xyz;
                let e2 = tr.p2.xyz - tr.p0.xyz;
                let p = cross(d, e2);
                let det = dot(e1, p);
                if abs(det) < 1e-14 { continue; }
                let id = 1.0 / det;
                let s = o - tr.p0.xyz;
                let u = dot(s, p) * id;
                if u < 0.0 || u > 1.0 { continue; }
                let q = cross(s, e1);
                let v = dot(d, q) * id;
                if v < 0.0 || u + v > 1.0 { continue; }
                let t = dot(e2, q) * id;
                if t > 0.0 && t < best.t {
                    best.t = t;
                    best.tri = k;
                    best.u = u;
                    best.v = v;
                    best.hit = true;
                    if any_hit { return best; }
                }
            }
        } else if sp + 2u <= 48u {
            let l = start;
            let r = start + 1u;
            let dl = slab(nodes[l], o, inv, best.t);
            let dr = slab(nodes[r], o, inv, best.t);
            if dl < 1e29 && dr < 1e29 {
                if dl <= dr {
                    stack[sp] = r;
                    stack[sp + 1u] = l;
                } else {
                    stack[sp] = l;
                    stack[sp + 1u] = r;
                }
                sp += 2u;
            } else if dl < 1e29 {
                stack[sp] = l;
                sp += 1u;
            } else if dr < 1e29 {
                stack[sp] = r;
                sp += 1u;
            }
        }
    }
    return best;
}

// ---------------------------------------------------------------- shading

fn ortho(n: vec3<f32>) -> mat3x3<f32> {
    let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
    let t = normalize(cross(a, n));
    let b = cross(n, t);
    return mat3x3<f32>(t, b, n);
}

fn fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - clamp(c, 0.0, 1.0), 5.0);
}

fn g1(ndx: f32, a2: f32) -> f32 {
    return 2.0 * ndx / (ndx + sqrt(a2 + (1.0 - a2) * ndx * ndx));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let w = U.size.x;
    let h = U.size.y;
    if g.x >= w || g.y >= h { return; }
    let idx = g.y * w + g.x;
    rng_state = idx * 1973u + U.size.z * 9277u + 26699u;
    let px = vec2<f32>(f32(g.x) + rnd(), f32(g.y) + rnd());
    let ndc = vec2<f32>(px.x / f32(w) * 2.0 - 1.0, 1.0 - px.y / f32(h) * 2.0);
    let a4 = U.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let b4 = U.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    var o = a4.xyz / a4.w;
    var d = normalize(b4.xyz / b4.w - o);

    var radiance = vec3<f32>(0.0);
    var throughput = vec3<f32>(1.0);
    let eps = U.eye.w * 1e-4;
    // Denoiser guides of the first surface shaded.
    var g_alb = vec3<f32>(1.0);
    var g_nrm = vec3<f32>(0.0);
    var g_depth = 1e9;
    var guided = false;
    var passes = 0u;
    for (var bounce = 0u; bounce <= U.size.w; bounce++) {
        let hit = trace(o, d, false);
        if !hit.hit {
            if bounce == 0u {
                if U.env.z > 1.5 {
                    // Black (material previews take their alpha from the
                    // coverage guide).
                } else if U.env.z > 0.5 {
                    let gr = mix(vec3<f32>(0.018), vec3<f32>(0.06), ndc.y * 0.5 + 0.5);
                    radiance += gr;
                } else {
                    radiance += env(d, U.env.w);
                }
            } else {
                radiance += throughput * env(d, 0.0);
            }
            break;
        }
        let tr = tris[hit.tri];
        let bw = vec3<f32>(1.0 - hit.u - hit.v, hit.u, hit.v);
        let uv = vec2<f32>(dot(bw, vec3<f32>(tr.p0.w, tr.p1.w, tr.p2.w)), dot(bw, vec3<f32>(tr.n0.w, tr.n1.w, tr.n2.w)));
        let p = o + d * hit.t;
        var ng = normalize(cross(tr.p1.xyz - tr.p0.xyz, tr.p2.xyz - tr.p0.xyz));
        var ns = normalize(tr.n0.xyz * bw.x + tr.n1.xyz * bw.y + tr.n2.xyz * bw.z);
        if dot(ng, d) > 0.0 { ng = -ng; }
        if dot(ns, ng) < 0.0 { ns = -ns; }
        let slot = tr.info.x;
        let base4 = tex_base(slot, uv);
        let base = base4.rgb;
        // Cutout and alpha blend: the ray passes through where the surface
        // is (partly) transparent.
        // Guides from the first surface met (also when the ray passes
        // through it): stable from sample to sample.
        if !guided {
            guided = true;
            g_alb = max(base, vec3<f32>(0.02));
            g_nrm = ns;
            g_depth = length(p - a4.xyz / a4.w);
        }
        let amode = slot_vec(U.amode_a, U.amode_b, slot);
        if passes < 16u && ((amode > 1.5 && rnd() > base4.a) || (amode > 0.5 && amode < 1.5 && base4.a < slot_vec(U.cut_a, U.cut_b, slot))) {
            o = p + d * eps * 4.0;
            passes += 1u;
            bounce -= 1u;
            continue;
        }
        let orm = tex_orm(slot, uv);
        let n4 = tex_nrm(slot, uv);
        let nts = n4.xyz;
        let transmission = clamp(n4.w, 0.0, 1.0);
        // Hit from inside (geometric normal facing the ray): leaving glass.
        let ng_raw = normalize(cross(tr.p1.xyz - tr.p0.xyz, tr.p2.xyz - tr.p0.xyz));
        let inside = dot(ng_raw, d) > 0.0;
        radiance += throughput * tex_emi(slot, uv);
        var t = tr.tng.xyz - ns * dot(ns, tr.tng.xyz);
        if dot(t, t) < 1e-10 { t = ortho(ns)[0]; }
        t = normalize(t);
        let b = cross(ns, t) * select(1.0, -1.0, tr.tng.w < 0.0);
        var n = normalize(t * nts.x + b * nts.y + ns * max(nts.z, 1e-3));
        if dot(n, -d) < 0.0 { n = ns; }

        let rough = clamp(orm.g, 0.03, 1.0);
        let metal = clamp(orm.b, 0.0, 1.0);
        let a = rough * rough;
        let a2 = a * a;
        let v = -d;
        let ndv = max(dot(n, v), 1e-4);
        let f0 = mix(vec3<f32>(0.04), base, metal);
        let fv = fresnel(f0, ndv);
        let p_spec = clamp(max(fv.r, max(fv.g, fv.b)) * (1.0 - 0.5 * rough) + metal * 0.5, 0.1, 0.95);
        let basis = ortho(n);
        var nd: vec3<f32>;
        if rnd() < p_spec {
            // GGX half vector
            let u1 = rnd();
            let u2 = rnd();
            let cos_t = sqrt((1.0 - u1) / (1.0 + (a2 - 1.0) * u1));
            let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
            let phi = 2.0 * PI * u2;
            let hv = basis * vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t);
            nd = reflect(d, hv);
            let ndl = dot(n, nd);
            if ndl <= 0.0 { break; }
            let ndh = max(dot(n, hv), 1e-4);
            let vdh = max(dot(v, hv), 1e-4);
            let gg = g1(ndv, a2) * g1(ndl, a2);
            throughput *= fresnel(f0, vdh) * gg * vdh / (ndh * ndv) / p_spec;
        } else if rnd() < transmission * (1.0 - metal) {
            // Transmission: refraction through a rough interface (GGX
            // microfacet), tinted by the base color; IOR 1.45.
            let u1 = rnd();
            let u2 = rnd();
            let cos_t = sqrt((1.0 - u1) / (1.0 + (a2 - 1.0) * u1));
            let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
            let phi = 2.0 * PI * u2;
            var hv = basis * vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t);
            let eta = select(1.0 / 1.45, 1.45, inside);
            var rd = refract(d, hv, eta);
            if dot(rd, rd) < 1e-8 {
                // Total internal reflection.
                rd = reflect(d, hv);
                o = p + ng * eps;
            } else {
                o = p - ng * eps;
                throughput *= select(base, vec3<f32>(1.0), inside) * (1.0 - fv) / (1.0 - p_spec);
            }
            d = normalize(rd);
            if bounce >= 2u {
                let q = clamp(max(throughput.r, max(throughput.g, throughput.b)), 0.05, 0.95);
                if rnd() > q { break; }
                throughput /= q;
            }
            continue;
        } else {
            let u1 = rnd();
            let u2 = rnd();
            let r = sqrt(u1);
            let phi = 2.0 * PI * u2;
            nd = basis * vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(1.0 - u1, 0.0)));
            throughput *= base * (1.0 - metal) * (1.0 - fv) / (1.0 - p_spec);
        }
        if dot(nd, ng) <= 0.0 { break; }
        o = p + ng * eps;
        d = normalize(nd);
        if bounce >= 2u {
            let q = clamp(max(throughput.r, max(throughput.g, throughput.b)), 0.05, 0.95);
            if rnd() > q { break; }
            throughput /= q;
        }
    }
    radiance = min(radiance, vec3<f32>(64.0));
    let f = f32(U.size.z);
    var acc = radiance;
    if U.size.z > 0u {
        acc = (accum[idx].rgb * f + radiance) / (f + 1.0);
    }
    accum[idx] = vec4<f32>(acc, 1.0);
    textureStore(out_tex, vec2<i32>(g.xy), vec4<f32>(acc, 1.0));
    // Guides, averaged over the samples (the pixel jitters).
    var ga = vec4<f32>(g_alb, g_depth);
    var gn = vec4<f32>(g_nrm, select(0.0, 1.0, guided));
    if U.size.z > 0u {
        ga = (aux[idx * 2u] * f + ga) / (f + 1.0);
        gn = (aux[idx * 2u + 1u] * f + gn) / (f + 1.0);
    }
    aux[idx * 2u] = ga;
    aux[idx * 2u + 1u] = gn;
}
