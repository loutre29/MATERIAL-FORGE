// Seamless procedural textures (engine/procgen.rs). Every generator is
// periodic over the unit square: cell counts are integers and lattice
// coordinates wrap, so the texture tiles like Painter's procedurals.
// Requires common.wgsl.

struct Gen {
    // family, variant, resolution, unused
    k: vec4<u32>,
    // seed, balance, contrast, invert
    a: vec4<f32>,
    p0: vec4<f32>,
    p1: vec4<f32>,
    // hidden constants of the variant
    v: vec4<f32>,
};
@group(0) @binding(0) var<uniform> G: Gen;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var mip_src: texture_2d<f32>;
@group(0) @binding(3) var mip_dst: texture_storage_2d<rgba16float, write>;

// ---------------------------------------------------------------- hashing

fn hash_u(x: u32) -> u32 {
    var v = x;
    v ^= v >> 16u;
    v *= 0x7feb352du;
    v ^= v >> 15u;
    v *= 0x846ca68bu;
    v ^= v >> 16u;
    return v;
}

fn seed_u() -> u32 {
    return u32(G.a.x * 1000.0 + 0.5) * 0x9e3779b9u;
}

// Hash of an integer lattice point wrapped by the period n.
fn hw(i: vec2<f32>, n: vec2<f32>, salt: u32) -> u32 {
    let w = i - n * floor(i / n);
    let x = u32(i32(w.x) & 0xffff);
    let y = u32(i32(w.y) & 0xffff);
    return hash_u(x ^ hash_u(y ^ hash_u(seed_u() ^ salt)));
}

fn rnd(i: vec2<f32>, n: vec2<f32>, salt: u32) -> f32 {
    return f32(hw(i, n, salt) & 0xffffffu) / 16777215.0;
}

fn rnd2(i: vec2<f32>, n: vec2<f32>, salt: u32) -> vec2<f32> {
    let h = hw(i, n, salt);
    return vec2<f32>(f32(h & 0xffffu), f32(h >> 16u)) / 65535.0;
}

fn hash_f(x: u32) -> f32 {
    return f32(hash_u(x ^ seed_u()) & 0xffffffu) / 16777215.0;
}

fn sstep(t: vec2<f32>) -> vec2<f32> {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// ---------------------------------------------------------------- noises

// Value noise, n cells over the unit square.
fn vnoise(uv: vec2<f32>, n: vec2<f32>, salt: u32) -> f32 {
    let p = uv * n;
    let i = floor(p);
    let f = sstep(fract(p));
    let a = rnd(i, n, salt);
    let b = rnd(i + vec2<f32>(1.0, 0.0), n, salt);
    let c = rnd(i + vec2<f32>(0.0, 1.0), n, salt);
    let d = rnd(i + vec2<f32>(1.0, 1.0), n, salt);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn grad_at(i: vec2<f32>, n: vec2<f32>, salt: u32, f: vec2<f32>) -> f32 {
    let a = rnd(i, n, salt) * 6.2831853;
    return dot(vec2<f32>(cos(a), sin(a)), f);
}

// Gradient (Perlin) noise in about [0, 1].
fn pnoise(uv: vec2<f32>, n: vec2<f32>, salt: u32) -> f32 {
    let p = uv * n;
    let i = floor(p);
    let f = fract(p);
    let u = sstep(f);
    let a = grad_at(i, n, salt, f);
    let b = grad_at(i + vec2<f32>(1.0, 0.0), n, salt, f - vec2<f32>(1.0, 0.0));
    let c = grad_at(i + vec2<f32>(0.0, 1.0), n, salt, f - vec2<f32>(0.0, 1.0));
    let d = grad_at(i + vec2<f32>(1.0, 1.0), n, salt, f - vec2<f32>(1.0, 1.0));
    return clamp(mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 0.75 + 0.5, 0.0, 1.0);
}

// Fractal sum: `oct` octaves from n cells, each doubling, amplitude * rough.
fn fbm(uv: vec2<f32>, n: vec2<f32>, oct: i32, rough: f32, perlin: bool) -> f32 {
    var s = 0.0;
    var amp = 1.0;
    var tot = 0.0;
    var f = n;
    for (var o = 0; o < oct; o++) {
        let v = select(vnoise(uv, f, u32(o) * 7919u), pnoise(uv, f, u32(o) * 7919u), perlin);
        s += v * amp;
        tot += amp;
        amp *= rough;
        f *= 2.0;
    }
    return s / max(tot, EPS);
}

// Ridged fractal: creases where the noise crosses its middle.
fn ridged(uv: vec2<f32>, n: vec2<f32>, oct: i32, rough: f32) -> f32 {
    var s = 0.0;
    var amp = 1.0;
    var tot = 0.0;
    var f = n;
    for (var o = 0; o < oct; o++) {
        let v = 1.0 - abs(pnoise(uv, f, u32(o) * 104729u) * 2.0 - 1.0);
        s += v * v * amp;
        tot += amp;
        amp *= rough;
        f *= 2.0;
    }
    return s / max(tot, EPS);
}

// Worley: (F1, F2, random value of the nearest cell, its index hash as f32).
fn worley(uv: vec2<f32>, n: vec2<f32>, jitter: f32, salt: u32) -> vec4<f32> {
    let p = uv * n;
    let i = floor(p);
    let f = fract(p);
    var d1 = 9.0;
    var d2 = 9.0;
    var id = 0.0;
    var id2 = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let c = i + o;
            let r = rnd2(c, n, salt);
            let pt = o + 0.5 + (r - 0.5) * jitter;
            let d = length(pt - f);
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = rnd(c, n, salt + 17u);
                id2 = f32(hw(c, n, salt + 31u) & 0xffffu);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    return vec4<f32>(d1, d2, id, id2);
}

// Smooth sum of Gaussian spots: k per cell, radius r (cells).
fn gauss_spots(uv: vec2<f32>, n: vec2<f32>, k: i32, r: f32, disorder: f32) -> f32 {
    let p = uv * n;
    let i = floor(p);
    let f = fract(p);
    var s = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let c = i + o;
            for (var j = 0; j < k; j++) {
                let r2 = rnd2(c, n, 101u + u32(j) * 13u);
                let pt = o + mix(vec2<f32>(0.5), r2, clamp(disorder, 0.0, 1.0));
                let size = r * mix(0.5, 1.2, rnd(c, n, 211u + u32(j) * 7u));
                let d = length(pt - f) / max(size, 1e-3);
                s += exp(-d * d * 2.0) * mix(0.4, 1.0, rnd(c, n, 307u + u32(j)));
            }
        }
    }
    return s;
}

// Distance to a segment.
fn seg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return length(a + ab * t - p);
}

// Short strokes: k per cell, length len (cells), angle around `dir` with
// `spread`, width w. Returns the max coverage.
fn strokes(uv: vec2<f32>, n: vec2<f32>, k: i32, len: f32, dir: f32, spread: f32, w: f32, salt: u32) -> f32 {
    let p = uv * n;
    let i = floor(p);
    let f = fract(p);
    var s = 0.0;
    let reach = min(i32(ceil(len)), 6) + 1; // typed lengths can be huge
    for (var y = -reach; y <= reach; y++) {
        for (var x = -reach; x <= reach; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let c = i + o;
            for (var j = 0; j < k; j++) {
                let r = rnd2(c, n, salt + u32(j) * 29u);
                let a = dir + (rnd(c, n, salt + 5u + u32(j) * 31u) - 0.5) * spread;
                let l = len * mix(0.4, 1.0, rnd(c, n, salt + 9u + u32(j) * 37u));
                let d = vec2<f32>(cos(a), sin(a)) * l * 0.5;
                let m = o + r;
                let dist = seg(f, m - d, m + d);
                let ww = w * mix(0.5, 1.0, rnd(c, n, salt + 11u + u32(j) * 41u));
                s = max(s, (1.0 - smoothstep(0.0, ww, dist)) * mix(0.5, 1.0, rnd(c, n, salt + 13u + u32(j))));
            }
        }
    }
    return s;
}

// Periodic domain warp by two noises.
fn warp(uv: vec2<f32>, n: vec2<f32>, amount: f32) -> vec2<f32> {
    if amount <= 0.0 {
        return uv;
    }
    let w = vec2<f32>(fbm(uv, n, 3, 0.5, true), fbm(uv + vec2<f32>(0.37, 0.71), n, 3, 0.5, true)) - 0.5;
    return uv + w * amount / max(n.x, 1.0);
}

fn cells_n() -> vec2<f32> {
    let s = max(round(G.p0.x), 1.0);
    return vec2<f32>(s, s);
}

// ---------------------------------------------------------------- shapes

fn hex_dist(p: vec2<f32>) -> vec2<f32> {
    // (distance to the hexagon edge in [0, 0.5], cell id hash)
    let s = vec2<f32>(1.0, 1.7320508);
    let a = p - s * floor(p / s) - s * 0.5;
    let b = p - s * floor((p - s * 0.5) / s) - s;
    let g = select(b, a, dot(a, a) < dot(b, b));
    let d = max(abs(g.x), dot(abs(g), normalize(s)));
    return vec2<f32>(d, 0.0);
}

fn aa_step(edge: f32, x: f32) -> f32 {
    let w = 1.5 / f32(G.k.z);
    return smoothstep(edge - w, edge + w, x);
}

// ---------------------------------------------------------------- families

fn family(uv: vec2<f32>) -> f32 {
    let n = cells_n();
    let dis = G.p0.y;
    let variant = G.k.y;
    switch G.k.x {
        // Perlin noise: scale, disorder.
        case 0u: {
            let q = warp(uv, n, dis * 2.0);
            return clamp((pnoise(q, n, 1u) - 0.5) * 1.5 + 0.5, 0.0, 1.0);
        }
        // Clouds: scale, disorder; v = (octaves, roughness, perlin, contrast).
        case 1u: {
            let q = warp(uv, n, dis * 2.0);
            let v = fbm(q, n, i32(G.v.x), G.v.y, G.v.z > 0.5);
            return clamp((v - 0.5) * G.v.w + 0.5, 0.0, 1.0);
        }
        // Fractal sum: scale, roughness, min level, max level.
        case 2u: {
            let lo = i32(clamp(G.p0.z, 0.0, 6.0));
            let hi = i32(clamp(G.p0.w, f32(lo) + 1.0, 10.0));
            var s = 0.0;
            var amp = 1.0;
            var tot = 0.0;
            var f = n * exp2(f32(lo));
            for (var o = lo; o < hi; o++) {
                s += pnoise(uv, f, u32(o) * 977u) * amp;
                tot += amp;
                amp *= clamp(G.p0.y, 0.05, 1.0);
                f *= 2.0;
            }
            let v = s / max(tot, EPS);
            return clamp((v - 0.5) * G.v.x + 0.5, 0.0, 1.0);
        }
        // Cells: scale, disorder; variant 0 distance, 1 borders, 2 flat values, 3 bulges.
        case 3u: {
            let w = worley(warp(uv, n, dis * 0.5), n, 1.0, 3u);
            let e = w.y - w.x;
            switch variant {
                // Cells 1: white, cracks gathered in clusters
                case 0u: {
                    let cl = smoothstep(0.35, 0.6, fbm(uv, n * 0.25, 4, 0.55, true));
                    return 1.0 - (1.0 - smoothstep(0.02, 0.24, e)) * cl;
                }
                // Cells 2: thick dark borders
                case 1u: { return smoothstep(0.05, 0.13, e); }
                // Cells 3: thin dark borders
                case 2u: { return smoothstep(0.0, 0.035, e); }
                // Cells 4: flat random tone per cell
                default: { return w.z; }
            }
        }
        // BnW spots: scale, roughness; v = (threshold, softness, octaves, 0).
        case 4u: {
            let v = fbm(uv, n, i32(G.v.z), clamp(G.p0.y, 0.1, 0.95), true);
            return clamp((v - G.v.x) * G.v.y + 0.5, 0.0, 1.0);
        }
        // Crystal: shaded facets of a Voronoi tessellation.
        case 5u: {
            if variant == 0u {
                // Crystal 1: dark facets, bright ridges where cells meet
                let w = worley(warp(uv, n, 0.9 + dis), n, 1.0, 5u);
                let ridge = pow(clamp(1.0 - (w.y - w.x) * 2.2, 0.0, 1.0), 5.0);
                let glow = vnoise(uv, n * 2.0, 7u);
                return clamp(ridge * mix(0.3, 1.2, glow) + w.x * w.z * 0.25, 0.0, 1.0);
            }
            // Crystal 2: long sharp shards (cells stretched vertically)
            let nn = vec2<f32>(n.x, max(round(n.y * 0.15), 1.0));
            let w = worley(uv, nn, 1.0, 6u);
            return clamp(w.z * 0.6 + (1.0 - w.x) * 0.5 - 0.1, 0.0, 1.0);
        }
        case 105u: {
            let p = uv * n;
            let i = floor(p);
            let f = fract(p);
            var best = 9.0;
            var shade = 0.0;
            for (var y = -1; y <= 1; y++) {
                for (var x = -1; x <= 1; x++) {
                    let o = vec2<f32>(f32(x), f32(y));
                    let c = i + o;
                    let pt = o + rnd2(c, n, 5u);
                    let d = length(pt - f);
                    if d < best {
                        best = d;
                        let a = rnd(c, n, 9u) * 6.2831853;
                        shade = rnd(c, n, 13u) * mix(0.6, 1.0, G.v.x) + dot(f - pt, vec2<f32>(cos(a), sin(a))) * G.v.y;
                    }
                }
            }
            return clamp(shade, 0.0, 1.0);
        }
        // Dirt: soft blotches with speckles; v = (blotch, speckle, threshold, 0).
        case 6u: {
            // Sparse bright specks on black, denser in soft blotches.
            let px = floor(uv * f32(G.k.z) / max(G.v.w, 1.0));
            let r = f32(hash_u(u32(px.x) + u32(px.y) * 65536u ^ seed_u()) & 0xffffu) / 65535.0;
            let blot = fbm(warp(uv, n, 0.6 + dis), n, 6, 0.65, true);
            let speck = pow(r, G.v.z);
            if variant == 1u {
                // Dirt gradient: dark top, bright cloudy bottom
                return clamp(uv.y * 0.85 + (blot - 0.5) * 0.7 + speck * 0.2 - 0.1, 0.0, 1.0);
            }
            return clamp(speck * mix(0.3, 1.3, blot) * G.v.y + (blot - 0.3) * 0.25 * G.v.x, 0.0, 1.0);
        }
        // Gaussian noise (smooth random), white noise, blue noise.
        case 7u: {
            switch variant {
                case 0u: { return clamp((fbm(uv, n * 2.0, 2, 0.3, true) - 0.5) * 1.8 + 0.5, 0.0, 1.0); }
                case 1u: {
                    let px = floor(uv * max(G.v.x, 64.0));
                    return f32(hash_u(u32(px.x) + u32(px.y) * 65536u ^ seed_u()) & 0xffffu) / 65535.0;
                }
                default: {
                    // High-passed white noise: flat spectrum without clumps.
                    let px = floor(uv * 256.0);
                    var c = 0.0;
                    for (var y = -1; y <= 1; y++) {
                        for (var x = -1; x <= 1; x++) {
                            let q = px + vec2<f32>(f32(x), f32(y));
                            let r = f32(hash_u(u32(q.x) + u32(q.y) * 65536u ^ seed_u()) & 0xffffu) / 65535.0;
                            c += select(-0.125 * r, r, x == 0 && y == 0);
                        }
                    }
                    return clamp(c * 0.9 + 0.5, 0.0, 1.0);
                }
            }
        }
        // Gaussian spots: scale, disorder; v = (radius, count, gain, 0).
        case 8u: {
            let s = gauss_spots(uv, n, i32(G.v.y), G.v.x, 1.0);
            return clamp(s * G.v.z, 0.0, 1.0);
        }
        // Directional noise: scale, stretch, angle (quarter turns), disorder.
        case 9u: {
            let quarter = u32(round(G.p0.z / 90.0)) % 2u;
            var q = uv;
            if quarter == 1u { q = uv.yx; }
            let st = max(round(G.p0.w), 1.0);
            let nn = vec2<f32>(max(n.x / st, 1.0), n.y * st * 0.5);
            let w = warp(q, vec2<f32>(n.x, n.x), dis);
            return fbm(w, nn, i32(G.v.x), G.v.y, G.v.z > 0.5);
        }
        // Scratches: density, length, width, angle spread.
        case 10u: {
            let a = G.p0.z * 0.0174533;
            let s = strokes(uv, n, i32(clamp(G.p1.x, 1.0, 4.0)), G.v.x, a, G.p0.w * 0.0174533, G.v.y, 41u);
            return s;
        }
        // Fibers / fur: scale, length, angle, spread; v = (width, count, curl, 0).
        case 11u: {
            var a = G.p0.z * 0.0174533;
            if variant == 1u {
                // Fur 3: the hair parts in the middle of each half tile
                a += (fract(uv.x * 2.0) - 0.5) * 2.2;
            }
            let s = strokes(warp(uv, n, G.v.z), n, i32(G.v.y), G.p0.y, a, G.p0.w * 0.0174533, G.v.x, 53u);
            if variant == 2u {
                // Messy fibers 3: gray lines on white
                return 1.0 - s * 0.6;
            }
            if variant == 3u {
                // Messy fibers 1: soft light fibers over gray
                return clamp(0.3 + s * 0.6 + fbm(uv, n, 3, 0.5, true) * 0.15, 0.0, 1.0);
            }
            return s;
        }
        // Moisture: scale, disorder; soft blobs with noisy edges.
        case 12u: {
            let v = fbm(warp(uv, n, 1.2 + dis), n, 7, 0.62, true);
            return clamp((v - 0.5) * G.v.x + 0.55, 0.0, 1.0);
        }
        // Plasma: interference of periodic waves, warped.
        case 13u: {
            // Bright blobs cut by dark contours where the noise crosses its middle.
            let v = fbm(warp(uv, n, 0.4 + dis), n, 2, 0.4, true);
            let line = smoothstep(0.0, 0.06, abs(v - 0.5));
            return clamp(line * (0.25 + v * 0.95), 0.0, 1.0);
        }
        // Waveform: horizontal waves; scale (waves), amplitude, disorder.
        case 14u: {
            // Audio-like waveforms: a jagged envelope around the middle line.
            let k = max(round(G.p0.z), 1.0);
            let env = pow(sin(3.14159265 * fract(uv.x * k)), 2.0);
            let jag = vnoise(uv, vec2<f32>(n.x * 32.0, 1.0), 3u) * 0.7 + vnoise(uv, vec2<f32>(n.x * 128.0, 1.0), 4u) * 0.3;
            let amp = env * G.p0.w * (0.1 + jag * jag);
            return 1.0 - aa_step(amp, abs(uv.y - 0.5));
        }
        // Voronoi: scale, disorder; variant 0 edges, 1 cells, 2 fractal edges.
        case 15u: {
            let j = clamp(1.0 - dis, 0.0, 1.0);
            if variant == 2u {
                var s = 0.0;
                var amp = 0.5;
                var f = n;
                for (var o = 0; o < 4; o++) {
                    let w = worley(uv, f, j, 61u + u32(o));
                    s += clamp(1.0 - w.x * 1.3, 0.0, 1.0) * amp;
                    amp *= 0.5;
                    f *= 2.0;
                }
                return clamp(s / 0.9375, 0.0, 1.0);
            }
            let w = worley(uv, n, j, 61u);
            if variant == 1u { return w.z; }
            // Bright centers falling off to dark borders.
            return clamp(pow(1.0 - w.x * 0.95, 1.6), 0.0, 1.0) * 0.9 + 0.05;
        }
        // Creased: folded ridges.
        case 16u: {
            let q = warp(uv, n, G.v.x + dis);
            let v = fbm(q, vec2<f32>(n.x * 4.0, max(round(n.y * 0.5), 1.0)), 3, 0.4, true);
            return clamp((v - 0.5) * G.v.w + 0.5 + G.v.z * 0.0, 0.0, 1.0);
        }
        // Fluid / liquid: heavily warped fractal noise.
        case 17u: {
            if variant == 1u {
                // Fluid: long vertical drips (cells stretched along Y)
                let nn = vec2<f32>(n.x * 2.0, max(round(n.y * 0.5), 1.0));
                let w = worley(warp(uv, vec2<f32>(n.x, n.x), 0.1 + dis), nn, 0.9, 81u);
                let d = smoothstep(0.8, 0.2, w.x);
                return clamp(d * mix(0.55, 1.0, w.z) * (0.75 + 0.25 * (1.0 - fract(uv.y * nn.y))), 0.0, 1.0);
            }
            var q = warp(uv, n, G.v.x + dis);
            q = warp(q, n * 2.0, G.v.y);
            let v = fbm(q, n, 5, 0.55, true);
            return 0.5 + 0.4 * sin(v * G.v.z * 6.2831853) + (vnoise(uv, n * 8.0, 9u) - 0.5) * 0.2;
        }
        // Microscope view: rings around cell centers.
        case 18u: {
            // Small bubbles and rings on a dark background.
            let w = worley(warp(uv, n, dis * 0.5), n, 1.0, 71u);
            let r = mix(0.12, 0.35, w.z);
            let ring = exp(-pow((w.x - r) / 0.03, 2.0));
            let filled = select(0.0, smoothstep(r, r * 0.5, w.x) * 0.35, w.z > 0.7);
            let bg = fbm(uv, n * 0.5, 3, 0.5, false) * 0.15;
            return clamp(ring * mix(0.5, 1.0, fract(w.z * 7.0)) + filled + bg, 0.0, 1.0);
        }
        case 19u: { return grunge(uv, n, dis, variant); }
        // Geometric shapes; variant picks the shape, p0 = (scale, size, ...).
        case 20u: {
            let p = uv * n;
            let f = fract(p) - 0.5;
            let size = G.p0.y;
            switch variant {
                // Checker
                case 0u: { return select(0.0, 1.0, (i32(floor(p.x)) + i32(floor(p.y))) % 2 != 0); }
                // Circles
                case 1u: { return 1.0 - aa_step(size * 0.5, length(f)); }
                // Hexagons (filled)
                case 2u: { return 1.0 - aa_step(size * 0.5, hex_dist(uv * vec2<f32>(n.x, n.y * 1.1547)).x); }
                // Hexagon borders
                case 3u: { let d = hex_dist(uv * vec2<f32>(n.x, n.y * 1.1547)).x; return aa_step(0.5 - size * 0.1, d); }
                // Polygon (square rotated by the angle)
                case 4u: {
                    let a = G.p0.z * 0.0174533;
                    let r = vec2<f32>(f.x * cos(a) - f.y * sin(a), f.x * sin(a) + f.y * cos(a));
                    return 1.0 - aa_step(size * 0.5, max(abs(r.x), abs(r.y)));
                }
                // Stripes
                case 5u: { return 1.0 - aa_step(size * 0.5, abs(f.x)); }
                // Hexagon variants: border / branch / double / wave
                case 8u, 9u, 10u, 11u: {
                    let q = uv * vec2<f32>(n.x, n.y * 1.1547);
                    var d = hex_dist(q).x;
                    if variant == 11u {
                        d += sin(uv.x * 6.2831853 * n.x * 2.0) * 0.03;
                    }
                    if variant == 10u {
                        return max(aa_step(0.5 - size * 0.1, d), 1.0 - aa_step(0.32 - size * 0.05, d) - (1.0 - aa_step(0.25, d)));
                    }
                    if variant == 9u {
                        let a = atan2(fract(q.y) - 0.5, fract(q.x) - 0.5);
                        return max(aa_step(0.5 - size * 0.1, d), (1.0 - aa_step(0.02 + size * 0.03, abs(sin(a * 3.0)) * d)) * (1.0 - aa_step(0.45, d)));
                    }
                    return aa_step(0.5 - size * 0.1, d);
                }
                // Hexagons with triangles / triangle borders
                case 12u, 13u: {
                    let q = uv * vec2<f32>(n.x, n.y * 1.1547);
                    let d = hex_dist(q).x;
                    let f2 = fract(q) - 0.5;
                    let tri = max(abs(f2.x) * 0.866 + f2.y * 0.5, -f2.y);
                    if variant == 13u { return max(aa_step(0.5 - size * 0.1, d), 1.0 - aa_step(0.02, abs(tri - 0.2))); }
                    return max(aa_step(0.5 - size * 0.1, d), 1.0 - aa_step(0.2, tri));
                }
                // Polygon 1: a single hexagon in the tile
                case 17u: {
                    let q = abs(f);
                    let d = max(q.x, q.x * 0.5 + q.y * 0.866);
                    return 1.0 - aa_step(size * 0.45, d);
                }
                // Polygon 2: a triangle shaded toward its center
                case 14u: {
                    let tri = max(abs(f.x) * 0.866 + f.y * 0.5, -f.y);
                    return clamp(1.0 - tri / max(size * 0.45, 0.01), 0.0, 1.0) * (1.0 - aa_step(size * 0.45, tri));
                }
                // Diagonal stripes
                case 15u: { return 1.0 - aa_step(size * 0.25, abs(fract((uv.x + uv.y) * n.x) - 0.5)); }
                // Checker 2: rotated checker
                case 16u: {
                    let r = vec2<f32>(uv.x + uv.y, uv.x - uv.y) * n.x;
                    return select(0.0, 1.0, (i32(floor(r.x)) + i32(floor(r.y))) % 2 != 0);
                }
                // Dots in a hexagonal grid
                case 6u: {
                    let q = uv * vec2<f32>(n.x, n.y * 1.1547);
                    let h = hex_dist(q).x;
                    return 1.0 - aa_step(size * 0.3, 0.5 - h);
                }
                // Triangles
                default: {
                    let tri = max(abs(f.x) * 0.866 + f.y * 0.5, -f.y);
                    return 1.0 - aa_step(size * 0.3, tri);
                }
            }
        }
        // Gradients; variant picks the shape.
        case 21u: {
            let p = uv * n;
            let f = fract(p);
            switch variant {
                // Linear 1: white top to black bottom
                case 0u: { return 1.0 - f.y; }
                // Linear 2: soft, white in the middle
                case 1u: { return 1.0 - pow(abs(f.y * 2.0 - 1.0), 2.0); }
                // Linear 3: sharp white middle line
                case 2u: { return 1.0 - abs(f.y * 2.0 - 1.0); }
                // Circular: angle around the center
                case 3u: { let d = f - 0.5; return fract(atan2(-d.y, d.x) / 6.2831853 + 1.0); }
                // Axial: diagonal ramp
                case 9u: { return clamp((f.x + f.y) * 0.5, 0.0, 1.0); }
                // Axial reflected
                case 10u: { return 1.0 - abs(f.x + f.y - 1.0); }
                // Radial
                case 11u: { return clamp(1.0 - length(f - 0.5) * 2.0, 0.0, 1.0); }
                // Hue-like sweep: sine bands
                case 12u: { return 0.5 + 0.5 * sin(f.x * 6.2831853); }
                // Weave: alternating horizontal / vertical ramps
                case 13u: { let c = (i32(floor(p.x * 2.0)) + i32(floor(p.y * 2.0))) % 2 != 0; return select(fract(p.x * 2.0), fract(p.y * 2.0), c); }
                // Dot (soft)
                case 4u: { let d = length(f - 0.5) * 2.0; return exp(-d * d * 4.0); }
                // Checker gradient
                case 5u: { let c = (i32(floor(p.x)) + i32(floor(p.y))) % 2 != 0; return select(f.x, f.y, c); }
                // Alternate
                case 6u: { return select(f.x, 1.0 - f.x, i32(floor(p.y)) % 2 != 0); }
                // Flakes: random gradient per cell
                case 7u: {
                    let i = floor(p);
                    let a = rnd(i, n, 7u) * 6.2831853;
                    return clamp(dot(f - 0.5, vec2<f32>(cos(a), sin(a))) + 0.5, 0.0, 1.0);
                }
                // Disc overlap
                default: { return clamp(1.0 - length(f - 0.5) * 1.4, 0.0, 1.0) * 0.6 + clamp(1.0 - length(fract(p + 0.5) - 0.5) * 1.4, 0.0, 1.0) * 0.4; }
            }
        }
        // Patterns; variant picks one.
        case 22u: {
            switch variant {
                // Bricks: rows, bricks per row, mortar, bevel
                case 0u, 1u: {
                    let rows = max(round(G.p0.x), 1.0);
                    let cols = max(round(G.p0.z), 1.0);
                    let q = vec2<f32>(uv.x * cols, uv.y * rows);
                    let row = floor(q.y);
                    let qx = q.x + select(0.0, 0.5, i32(row) % 2 != 0);
                    let f = vec2<f32>(fract(qx), fract(q.y));
                    // Brick units: x spans cols per tile, y rows; make the
                    // mortar and bevel the same width both ways.
                    let k = rows / cols;
                    let m = G.p0.y * 0.5;
                    let dx = min(f.x, 1.0 - f.x) / k - m;
                    let dy = min(f.y, 1.0 - f.y) - m;
                    let d = min(dx, dy);
                    if d <= 0.0 { return 0.0; }
                    let bev = clamp(d / max(G.p0.w, 1e-3), 0.0, 1.0);
                    let tone = mix(1.0, rnd(vec2<f32>(floor(qx), row), vec2<f32>(cols * 2.0, rows), 9u), G.v.x);
                    return mix(0.3, 1.0, bev) * tone * smoothstep(0.0, 0.004, d);
                }
                // Tiles: grid of square tiles with random tones
                case 2u, 3u: {
                    let p = uv * n;
                    let f = fract(p);
                    let m = G.p0.y * 0.5;
                    let e = smoothstep(m, m + 0.02, f.x) * smoothstep(m, m + 0.02, 1.0 - f.x) * smoothstep(m, m + 0.02, f.y) * smoothstep(m, m + 0.02, 1.0 - f.y);
                    let tone = mix(1.0, rnd(floor(p), n, 13u), select(0.0, 0.8, variant == 3u));
                    return e * tone;
                }
                // Triangle grid: randomly shaded triangles
                case 4u: {
                    let p = uv * n;
                    let i = floor(p);
                    let f = fract(p);
                    let upper = f.x > f.y;
                    let tone = rnd(i * 2.0 + select(vec2<f32>(0.0), vec2<f32>(1.0, 0.0), upper), n * 2.0, 4u);
                    let shade = select(f.y, f.x, upper);
                    let d = min(min(f.y, abs(f.x - f.y)), min(1.0 - f.y, min(f.x, 1.0 - f.x)));
                    return clamp(tone * 0.7 + shade * 0.3, 0.0, 1.0) * aa_step(G.p0.y * 0.05, d);
                }
                // Tile random: random rectangles with outlines
                case 15u: {
                    // Each cell splits once along one axis, each half maybe again
                    // along the other: rectangles of mixed sizes.
                    let p = uv * vec2<f32>(max(round(n.x * 0.4), 1.0));
                    let nn = vec2<f32>(max(round(n.x * 0.4), 1.0));
                    let i = floor(p);
                    let f = fract(p);
                    let r = rnd2(i, nn, 15u);
                    let vert = r.y > 0.5;
                    var g = select(f, f.yx, vert);
                    let c1 = mix(0.3, 0.7, r.x);
                    let side = select(0.0, 1.0, g.x > c1);
                    let lo = select(0.0, c1, side > 0.5);
                    let hi = select(c1, 1.0, side > 0.5);
                    let c2 = mix(0.3, 0.7, rnd(i * 2.0 + vec2<f32>(side, 0.0), nn * 2.0, 16u));
                    let split2 = rnd(i * 2.0 + vec2<f32>(side, 1.0), nn * 2.0, 17u) > 0.35;
                    var d = min(min(g.x - lo, hi - g.x), min(g.y, 1.0 - g.y));
                    if split2 { d = min(d, abs(g.y - c2)); }
                    return aa_step(0.012 + G.p0.y * 0.05, d);
                }
                // Arc pavement: rows of stones bent into arcs
                case 16u: {
                    let fx = fract(uv.x * n.x * 0.5) - 0.5;
                    let q = vec2<f32>(uv.x, uv.y - (1.0 - sqrt(max(1.0 - fx * fx * 3.0, 0.0))) * 1.5 / n.y);
                    let p = q * vec2<f32>(n.x * 2.0, n.y);
                    let row = floor(p.y);
                    let x = p.x + select(0.0, 0.5, i32(row) % 2 != 0);
                    let f = vec2<f32>(fract(x), fract(p.y));
                    let m = 0.08;
                    return smoothstep(0.0, m, f.x) * smoothstep(0.0, m, 1.0 - f.x) * smoothstep(0.0, m * 2.0, f.y) * smoothstep(0.0, m * 2.0, 1.0 - f.y);
                }
                // Wood: rings with grain
                case 5u: {
                    let q = warp(uv, vec2<f32>(2.0, 8.0), 0.6);
                    let r = (q.x + fbm(q, vec2<f32>(2.0, 16.0), 3, 0.5, true) * 0.6) * n.x;
                    let ring = fract(r);
                    let grain = vnoise(uv, vec2<f32>(8.0, 256.0), 3u);
                    return clamp(smoothstep(0.0, 0.7, ring) * (1.0 - smoothstep(0.8, 1.0, ring)) * 0.8 + grain * 0.2, 0.0, 1.0);
                }
                // Fence / grates: crossing bars
                case 6u, 7u, 8u, 9u, 10u: {
                    let p = uv * n;
                    let f = abs(fract(p) - 0.5);
                    let w = G.p0.y * 0.5;
                    let bars = max(1.0 - aa_step(w, f.x), 1.0 - aa_step(w, f.y));
                    if variant == 8u {
                        return 1.0 - aa_step(0.5 - w, length(fract(p) - 0.5));
                    }
                    if variant == 7u { return 1.0 - aa_step(w, f.y); }
                    if variant == 6u {
                        let d = abs(fract(p.x + p.y) - 0.5);
                        let d2 = abs(fract(p.x - p.y) - 0.5);
                        return max(1.0 - aa_step(w, d), 1.0 - aa_step(w, d2));
                    }
                    return bars;
                }
                // Pavement: irregular stones
                case 11u: {
                    let w = worley(warp(uv, n, 0.3), n, 0.8, 91u);
                    return smoothstep(0.0, 0.08 + G.p0.y * 0.2, w.y - w.x) * mix(0.7, 1.0, w.z);
                }
                // Camo: layered thresholded noise in 3 tones
                case 12u, 13u: {
                    let a = fbm(warp(uv, n, 0.6), n, 4, 0.5, true);
                    let b = fbm(warp(uv + 0.3, n, 0.6), n, 4, 0.5, true);
                    return select(0.0, 0.5, a > 0.5) + select(0.0, 0.5, b > 0.55);
                }
                // Scratches generator
                case 14u: {
                    return strokes(uv, n, 1, 0.9, G.p0.z * 0.0174533, G.p0.w * 0.0174533, 0.008, 97u) * 0.8;
                }
                // Paper path: random walk lines
                default: {
                    let v = fbm(uv, n, 4, 0.5, true);
                    return 1.0 - smoothstep(0.0, 0.04, abs(fract(v * 6.0) - 0.5));
                }
            }
        }
        // Marble: warped veins; v = (vein freq, sharpness, warp, 0).
        case 23u: {
            let q = warp(uv, n, G.v.z + dis);
            let t = fbm(q, n, 5, 0.55, true);
            let s = 0.5 + 0.5 * sin((q.x * n.x + t * G.v.x) * 6.2831853);
            return pow(s, G.v.y);
        }
        // Anisotropic noise: fine streaks along X.
        case 24u: {
            let nn = vec2<f32>(max(n.x * 0.25, 1.0), n.y * 16.0);
            let v = fbm(warp(uv, n, dis), nn, 4, 0.5, false);
            return clamp((v - 0.5) * 2.0 + 0.5, 0.0, 1.0);
        }
        default: { return 0.5; }
    }
}

fn contrast(v: f32, c: f32, mid: f32) -> f32 {
    return clamp((v - mid) * c + 0.5, 0.0, 1.0);
}

fn speckle(uv: vec2<f32>, cell: f32, thr: f32, salt: u32) -> f32 {
    let px = floor(uv * f32(G.k.z) / cell);
    let r = f32(hash_u(u32(px.x) + u32(px.y) * 65536u ^ seed_u() ^ salt) & 0xffffu) / 65535.0;
    return smoothstep(thr, 1.0, r);
}

// Vertical streaks: noise stretched along Y.
fn streaks(uv: vec2<f32>, nx: f32, ny: f32, salt: u32) -> f32 {
    return fbm(uv + vec2<f32>(f32(salt) * 0.013, 0.0), vec2<f32>(nx, ny), 5, 0.55, true);
}

// Grunge maps 001-015 and the named grunges. n is the scale.
fn grunge(uv: vec2<f32>, n: vec2<f32>, dis: f32, id: u32) -> f32 {
    // Barely warped: Painter's grunges are granular and blotchy, not swirly.
    let w1 = warp(uv, n, 0.12 + dis * 0.5);
    // Detailed fractal noise: the grain that Painter's grunges carry.
    // Mid-scale granular breakup on top of the blotches: crunchy, not soft.
    let blotch = fbm(w1, n, 6, 0.6, true);
    let crunch = fbm(uv, n * 4.0, 6, 0.85, false);
    let grain = clamp(blotch * 0.6 + crunch * 0.55 - 0.08, 0.0, 1.0);
    switch id {
        // 001: white, dark detailed patches
        case 1u: { return contrast(grain, 2.6, 0.4); }
        // 002: grainy vertical streaks
        case 2u: {
            let st = fbm(warp(uv, n, 0.3 + dis), vec2<f32>(n.x * 4.0, max(round(n.y * 0.5), 1.0)), 7, 0.7, true);
            return contrast(st * 0.75 + grain * 0.25, 2.8, 0.5);
        }
        // 003: vertical streaks with dark bands
        case 3u: {
            let st = fbm(uv, vec2<f32>(n.x * 3.0, max(round(n.y * 0.5), 1.0)), 7, 0.7, true);
            let band = fbm(uv, vec2<f32>(max(round(n.x * 0.5), 1.0), n.y * 2.0), 5, 0.6, true);
            return contrast(st * 0.5 + band * 0.3 + grain * 0.2, 2.8, 0.45);
        }
        // 004: white and gray blobs on black
        case 4u: {
            let g = gauss_spots(warp(uv, n, 0.4), n * 2.0, 2, 0.32, 1.0);
            let b = fbm(uv, n, 5, 0.6, true);
            let v = g * 0.7 + b * 0.5;
            return smoothstep(0.62, 0.7, v) * mix(0.55, 1.0, step(0.78, v));
        }
        // 005: wavy vertical lines with grain
        case 5u: {
            let q = warp(uv, vec2<f32>(n.x * 0.5, n.y * 0.25), 1.2 + dis);
            let lines = 0.5 + 0.5 * cos(q.x * 6.2831853 * n.x * 2.0);
            return contrast(lines * 0.75 + grain * 0.25, 2.0, 0.45);
        }
        // 006: light top, dark leaking bottom
        case 6u: {
            let st = fbm(uv, vec2<f32>(n.x * 6.0, 1.0), 7, 0.7, true);
            let ramp = 0.5 + 0.5 * cos(uv.y * 6.2831853);
            return contrast(ramp * 0.65 + st * 0.35 + grain * 0.15, 2.4, 0.6);
        }
        // 007: dark mottled clouds with light areas
        case 7u: { return contrast(grain, 2.6, 0.55); }
        // 008: white with dark scribbled cells
        case 8u: {
            let w = worley(w1, n * 4.0, 1.0, 8u);
            let cracks = 1.0 - smoothstep(0.0, 0.08, w.y - w.x);
            let zone = smoothstep(0.4, 0.6, fbm(uv, n, 5, 0.6, true));
            return clamp(1.0 - cracks * zone * 0.8 - (1.0 - grain) * 0.35 * zone, 0.0, 1.0);
        }
        // 009: white with dark splatters
        case 9u: {
            let g = gauss_spots(w1, n * 3.0, 2, 0.3, 1.0);
            return clamp(1.0 - smoothstep(0.55, 0.75, g * 0.6 + grain * 0.6), 0.0, 1.0);
        }
        // 010: gray with dark spots and fine cracks
        case 10u: {
            let spots = smoothstep(0.6, 0.8, gauss_spots(uv, n * 4.0, 2, 0.22, 1.0));
            let w = worley(w1, n * 6.0, 1.0, 10u);
            let cr = 1.0 - smoothstep(0.0, 0.05, w.y - w.x);
            return clamp(contrast(grain, 1.8, 0.45) - spots * 0.7 - cr * 0.3, 0.0, 1.0);
        }
        // 011: dark with horizontal streaks and scratches
        case 11u: {
            let st = fbm(uv, vec2<f32>(max(round(n.x * 0.5), 1.0), n.y * 6.0), 7, 0.7, true);
            let scr = strokes(uv, n * 3.0, 2, 1.8, 0.1, 0.4, 0.015, 11u);
            return clamp(contrast(st * 0.7 + grain * 0.3, 2.6, 0.55) + scr * 0.5, 0.0, 1.0);
        }
        // 012: almost white, fine dark smudges
        case 12u: {
            let smudge = smoothstep(0.62, 0.8, 1.0 - grain);
            let scr = strokes(uv, n * 3.0, 1, 1.0, 0.0, 6.28, 0.01, 12u);
            return clamp(1.0 - smudge * 0.8 - scr * 0.6, 0.0, 1.0);
        }
        // 013: dark clouds with bright fringes
        case 13u: { return clamp(contrast(grain, 3.2, 0.52) + speckle(uv, 1.0, 0.97, 13u) * 0.4, 0.0, 1.0); }
        // 014: large dark blobs over a textured gray
        case 14u: {
            let blob = smoothstep(0.55, 0.7, gauss_spots(uv, vec2<f32>(max(round(n.x * 1.5), 1.0)), 1, 0.4, 0.6));
            return clamp(contrast(grain, 2.0, 0.45) - blob * 0.85, 0.0, 1.0);
        }
        // 015: dark mottled with white specks
        case 15u: { return clamp(contrast(grain, 2.4, 0.55) + speckle(uv, 1.0, 0.96, 15u) * 0.6, 0.0, 1.0); }
        // Concrete: fine low-contrast grain
        case 20u: { return contrast(fbm(uv, n, 9, 0.75, true), 0.9, 0.5); }
        // Damas: damascus bands
        case 21u: {
            let q = warp(uv, n, 2.5 + dis);
            return 0.5 + 0.5 * sin(fbm(q, n, 4, 0.5, true) * 40.0);
        }
        // Galvanic large: mottled light / dark patches with grain
        case 22u: { return contrast(fbm(warp(uv, n, 1.0 + dis), n, 9, 0.74, true), 2.0, 0.5); }
        // Galvanic small: small swirls
        case 23u: {
            let q = warp(uv, n * 2.0, 1.2 + dis);
            return contrast(ridged(q, n * 2.0, 5, 0.55), 1.8, 0.45);
        }
        // Leaks: horizontal bands that drip
        case 24u: {
            let st = fbm(uv, vec2<f32>(n.x * 8.0, 1.0), 7, 0.7, true);
            let band = fbm(uv, vec2<f32>(1.0, max(round(n.y * 1.5), 2.0)), 4, 0.5, true);
            return contrast(st * 0.4 + band * 0.6, 3.0, 0.5);
        }
        // Leaky paint: gray streaks with blotches
        case 25u: { return contrast(fbm(uv, vec2<f32>(n.x * 6.0, 1.0), 7, 0.7, true) * 0.6 + grain * 0.4, 1.4, 0.5); }
        // Rough dirty: low contrast gray, faint vertical streaks
        case 26u: { return contrast(grain * 0.6 + fbm(uv, vec2<f32>(n.x * 8.0, 1.0), 6, 0.6, true) * 0.4, 0.9, 0.5); }
        // Rust fine: light gray patches with grain
        case 27u: { return contrast(grain, 1.7, 0.42); }
        // Scratches dirty / fine: flat gray with faint scratches
        case 28u, 29u: {
            let scr = strokes(uv, n * 2.0, 2, 2.0, 0.3, 1.2, select(0.02, 0.01, id == 29u), 28u + id);
            return clamp(0.5 + (grain - 0.5) * 0.12 + scr * 0.1, 0.0, 1.0);
        }
        // Scratches rough: thin bright scratches on black
        case 30u: { return strokes(uv, n * 2.0, 2, 1.5, 0.0, 6.28, 0.012, 30u) * 0.9; }
        // Shavings: fine specks on black
        case 31u: { return speckle(uv, 4.0, 0.9, 31u) * mix(0.6, 1.2, grain); }
        // Splashes dusty: small bright curved splashes and dust on black
        case 32u: {
            let s1 = strokes(warp(uv, n * 2.0, 1.2), n * 4.0, 2, 0.6, 0.0, 6.28, 0.04, 32u);
            return clamp(s1 * 0.8 + speckle(uv, 1.0, 0.985, 33u) * 0.5, 0.0, 1.0);
        }
        // Spots / spots dirty: sparse white spots on black
        case 33u, 34u: {
            let g = gauss_spots(uv, n * 6.0, 2, 0.16, 1.0);
            let v = smoothstep(0.25, 0.6, g);
            return clamp(v + select(0.0, speckle(uv, 2.0, 0.96, 34u) * 0.7, id == 34u), 0.0, 1.0);
        }
        default: { return grain; }
    }
}

@compute @workgroup_size(8, 8)
fn generate(@builtin(global_invocation_id) g: vec3<u32>) {
    let res = G.k.z;
    if g.x >= res || g.y >= res { return; }
    let uv = (vec2<f32>(g.xy) + 0.5) / f32(res);
    var v = family(uv);
    // Balance / contrast / invert, as the generators expose them.
    v = clamp((v - G.a.y) * G.a.z + 0.5, 0.0, 1.0);
    if G.a.w > 0.5 { v = 1.0 - v; }
    textureStore(out_tex, vec2<i32>(g.xy), vec4<f32>(v, v, v, 1.0));
}

@compute @workgroup_size(8, 8)
fn mip(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = vec2<i32>(textureDimensions(mip_dst));
    let id = vec2<i32>(g.xy);
    if id.x >= d.x || id.y >= d.y { return; }
    let p = id * 2;
    let s = textureLoad(mip_src, p, 0) + textureLoad(mip_src, p + vec2<i32>(1, 0), 0)
        + textureLoad(mip_src, p + vec2<i32>(0, 1), 0) + textureLoad(mip_src, p + vec2<i32>(1, 1), 0);
    textureStore(mip_dst, id, s * 0.25);
}
