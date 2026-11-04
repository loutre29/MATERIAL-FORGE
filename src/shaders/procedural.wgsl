// Procedural pattern library. Every pattern returns a value in [0, 1].
// Kinds follow `ProcKind::ALL` on the Rust side. Requires common.wgsl.

struct ProcParams {
    // seed, contrast, balance, warp
    a: vec4<f32>,
    // detail (octaves), aspect, density, roughness
    b: vec4<f32>,
};

fn hash13(p3in: vec3<f32>) -> f32 {
    var p3 = fract(p3in * 0.1031);
    p3 += dot(p3, p3.zyx + 31.32);
    return fract((p3.x + p3.y) * p3.z);
}

fn hash33(p3in: vec3<f32>) -> vec3<f32> {
    var p3 = fract(p3in * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yxz + 33.33);
    return fract((p3.xxy + p3.yxx) * p3.zyx);
}

fn vnoise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash13(i), hash13(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash13(i + vec3<f32>(0.0, 1.0, 0.0)), hash13(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash13(i + vec3<f32>(0.0, 0.0, 1.0)), hash13(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash13(i + vec3<f32>(0.0, 1.0, 1.0)), hash13(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn grad_dot(i: vec3<f32>, c: vec3<f32>, f: vec3<f32>) -> f32 {
    let g = hash33(i + c) * 2.0 - 1.0;
    return dot(g, f - c);
}

// Gradient (Perlin-style) noise in about [-1, 1].
fn gnoise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(grad_dot(i, vec3<f32>(0.0, 0.0, 0.0), f), grad_dot(i, vec3<f32>(1.0, 0.0, 0.0), f), u.x);
    let x10 = mix(grad_dot(i, vec3<f32>(0.0, 1.0, 0.0), f), grad_dot(i, vec3<f32>(1.0, 1.0, 0.0), f), u.x);
    let x01 = mix(grad_dot(i, vec3<f32>(0.0, 0.0, 1.0), f), grad_dot(i, vec3<f32>(1.0, 0.0, 1.0), f), u.x);
    let x11 = mix(grad_dot(i, vec3<f32>(0.0, 1.0, 1.0), f), grad_dot(i, vec3<f32>(1.0, 1.0, 1.0), f), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z) * 1.4;
}

fn fbm_value(p: vec3<f32>, octaves: i32, gain: f32) -> f32 {
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    var q = p;
    for (var o = 0; o < octaves; o++) {
        sum += amp * vnoise3(q);
        norm += amp;
        q = q * 2.03 + vec3<f32>(1.7, 9.2, 4.1);
        amp *= gain;
    }
    return sum / max(norm, EPS);
}

fn fbm_grad(p: vec3<f32>, octaves: i32, gain: f32) -> f32 {
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    var q = p;
    for (var o = 0; o < octaves; o++) {
        sum += amp * gnoise3(q);
        norm += amp;
        q = q * 2.01 + vec3<f32>(5.3, 1.1, 7.7);
        amp *= gain;
    }
    return clamp(sum / max(norm, EPS) * 0.5 + 0.5, 0.0, 1.0);
}

fn ridged(p: vec3<f32>, octaves: i32, gain: f32) -> f32 {
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    var q = p;
    var prev = 1.0;
    for (var o = 0; o < octaves; o++) {
        var n = 1.0 - abs(gnoise3(q));
        n = n * n * prev;
        prev = n;
        sum += amp * n;
        norm += amp;
        q = q * 2.02 + vec3<f32>(3.1, 7.3, 2.9);
        amp *= gain;
    }
    return clamp(sum / max(norm, EPS), 0.0, 1.0);
}

fn turbulence(p: vec3<f32>, octaves: i32, gain: f32) -> f32 {
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    var q = p;
    for (var o = 0; o < octaves; o++) {
        sum += amp * abs(gnoise3(q));
        norm += amp;
        q = q * 2.0 + vec3<f32>(2.7, 4.4, 8.1);
        amp *= gain;
    }
    return clamp(sum / max(norm, EPS) * 1.6, 0.0, 1.0);
}

// x = F1 distance, y = F2 distance, z = cell id hash.
fn voronoi3(p: vec3<f32>, jitter: f32) -> vec3<f32> {
    let i = floor(p);
    let f = fract(p);
    var f1 = 8.0;
    var f2 = 8.0;
    var id = 0.0;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let c = vec3<f32>(f32(x), f32(y), f32(z));
                let o = hash33(i + c) * jitter;
                let d = length(c + o - f);
                if d < f1 {
                    f2 = f1;
                    f1 = d;
                    id = hash13(i + c + 7.13);
                } else if d < f2 {
                    f2 = d;
                }
            }
        }
    }
    return vec3<f32>(f1, f2, id);
}

fn brick(p: vec2<f32>, mortar: f32) -> f32 {
    var q = p * vec2<f32>(1.0, 2.0);
    let row = floor(q.y);
    q.x += select(0.0, 0.5, fract(row * 0.5) > 0.25);
    let f = fract(q);
    let m = mortar * 0.5;
    let ex = smoothstep(0.0, m, f.x) * smoothstep(0.0, m, 1.0 - f.x);
    let ey = smoothstep(0.0, m * 2.0, f.y) * smoothstep(0.0, m * 2.0, 1.0 - f.y);
    return ex * ey;
}

fn proc_raw(kind: u32, p: vec3<f32>, pp: ProcParams) -> f32 {
    let oct = i32(clamp(pp.b.x, 1.0, 8.0));
    let gain = clamp(pp.b.w, 0.1, 0.9);
    let density = pp.b.z;
    switch kind {
        // Clouds (value fBm)
        case 0u: { return fbm_value(p, oct, gain); }
        // Perlin noise
        case 1u: { return fbm_grad(p, oct, gain); }
        // Ridged noise
        case 2u: { return ridged(p, oct, gain); }
        // Cells (flat random value per cell)
        case 3u: { return voronoi3(p, 1.0).z; }
        // Cracks (cell borders)
        case 4u: {
            let v = voronoi3(p, 1.0);
            return 1.0 - smoothstep(0.0, 0.03 + density * 0.12, v.y - v.x);
        }
        // Grunge (warped fBm with high-frequency speckles)
        case 5u: {
            let n = fbm_value(p, oct, gain);
            let s = fbm_value(p * 6.3 + 11.0, 3, 0.5);
            return smoothstep(0.42, 0.62, n * 0.75 + s * 0.25);
        }
        // Scratches (thin directional lines, broken up)
        case 6u: {
            let q = vec3<f32>(p.x * 0.25, p.y * 18.0, p.z * 0.25);
            let line = 1.0 - smoothstep(0.0, 0.05 + density * 0.1, abs(gnoise3(q)));
            let breakup = smoothstep(0.45, 0.65, fbm_value(p * 1.7 + 3.0, 3, 0.5));
            return line * breakup;
        }
        // Dots / spots
        case 7u: {
            let v = voronoi3(p, 0.8);
            let r = 0.12 + density * 0.35;
            return 1.0 - smoothstep(r * 0.8, r, v.x);
        }
        // Fibers (stretched noise)
        case 8u: { return fbm_value(vec3<f32>(p.x * 0.15, p.y * 6.0, p.z * 0.15), oct, gain); }
        // Wood rings
        case 9u: {
            let r = length(p.xz) * 4.0 + fbm_value(p * vec3<f32>(1.0, 0.15, 1.0) * 2.0, 3, 0.5) * 2.5;
            let ring = fract(r);
            return smoothstep(0.0, 0.7, ring) * (1.0 - smoothstep(0.85, 1.0, ring));
        }
        // Bricks
        case 10u: { return brick(p.xy, 0.05 + density * 0.2); }
        // Checker
        case 11u: {
            let c = floor(p.x) + floor(p.y) + floor(p.z);
            return select(0.0, 1.0, fract(c * 0.5) > 0.25);
        }
        // Linear gradient along the pattern Y axis
        case 12u: { return clamp(p.y, 0.0, 1.0); }
        // Turbulence
        case 13u: { return turbulence(p, oct, gain); }
        // Marble
        case 14u: { return 0.5 + 0.5 * sin(p.x * 3.0 + fbm_grad(p, oct, gain) * 9.0); }
        // Leather
        case 15u: {
            let v = voronoi3(p, 1.0);
            let grain = smoothstep(0.0, 0.18, v.y - v.x);
            return grain * (0.75 + 0.25 * fbm_value(p * 3.0, 3, 0.5));
        }
        // Rust spots (layered thresholded noise)
        case 16u: {
            let a = fbm_value(p, oct, gain);
            let b = fbm_value(p * 3.7 + 5.0, oct, gain);
            return clamp(smoothstep(0.5, 0.65, a) + smoothstep(0.62, 0.7, b) * 0.6, 0.0, 1.0);
        }
        // Fabric weave
        case 17u: {
            let w = sin(p.x * 2.0 * PI) * sin(p.y * 2.0 * PI);
            let warp = 0.5 + 0.5 * sin(p.x * 2.0 * PI * 8.0) * 0.3;
            return clamp(0.5 + 0.5 * sign(w) * abs(w) + warp * 0.2 - 0.1, 0.0, 1.0);
        }
        // Grain (white noise)
        case 18u: { return hash13(floor(p * 64.0)); }
        // Hexagon tiles
        case 19u: {
            let s = vec2<f32>(1.0, 1.7320508);
            let q = p.xy;
            let a = (q - s * floor(q / s + 0.0)) - s * 0.5;
            let b = (q - s * floor((q - s * 0.5) / s)) - s;
            let g = select(b, a, dot(a, a) < dot(b, b));
            let d = max(abs(g.x), dot(abs(g), normalize(s)));
            return smoothstep(0.5, 0.5 - 0.04 - density * 0.1, d);
        }
        // Painter's position-based 3D noises.
        // 3D Perlin noise: contrasted soft blobs
        case 20u: { return clamp(0.5 + gnoise3(p * 2.0) * 1.6, 0.0, 1.0); }
        // 3D Perlin noise fractal
        case 21u: { return fbm_grad(p * 2.0, oct, gain); }
        // 3D Ridged noise fractal
        case 22u: { return ridged(p * 2.0, oct, gain); }
        // 3D Simplex noise: smoother, rounder blobs
        case 23u: { return clamp(0.5 + gnoise3(p * 1.5) * 1.1, 0.0, 1.0); }
        // 3D Voronoi: bright cells falling off to the borders
        case 24u: { return clamp(1.0 - voronoi3(p * 2.0, 1.0).x * 1.2, 0.0, 1.0); }
        // 3D Voronoi fractal
        case 25u: {
            var s = 0.0;
            var amp = 0.5;
            var q = p * 2.0;
            for (var o = 0; o < 4; o++) {
                s += clamp(1.0 - voronoi3(q, 1.0).x * 1.2, 0.0, 1.0) * amp;
                amp *= 0.5;
                q *= 2.0;
            }
            return clamp(s / 0.9375, 0.0, 1.0);
        }
        // 3D Worley noise: distance to the nearest feature point
        case 26u: { return clamp(voronoi3(p * 2.0, 1.0).x * 1.3, 0.0, 1.0); }
        // Flakes: a gray per flake (a sparkle mask), see flake_normal.
        case 27u: { return flake(p, pp).w; }
        // Brush strokes (hand-painted look)
        case 28u: { return brush_strokes(p, density, gain); }
        // Paint splatter (spray paint: splashes, droplets, drips)
        case 29u: { return splatter(p, density); }
        // Streaks running down (rust, grime, water)
        case 30u: { return streaks(p, density); }
        default: { return 0.5; }
    }
}

// Brush strokes: flat strokes of paint laid over each other in the xy
// plane (each z slab its own layout), three per cell, one gray per stroke,
// ragged borders and bristle streaks along the stroke; they run within 40
// degrees of x. Meant for tri-planar projection (`proc_tri`), where every
// face gets its own strokes. density: stroke width, roughness (gain):
// bristle streaks.
fn brush_strokes(p: vec3<f32>, density: f32, bristle: f32) -> f32 {
    let i = floor(p);
    let f = vec2<f32>(fract(p.xy));
    let w = 0.15 + clamp(density, 0.0, 1.0) * 0.25;
    let rag = (vnoise3(p * 9.0) - 0.5) * 0.4 + (vnoise3(p * 23.0 + 5.0) - 0.5) * 0.12;
    var top = -1.0;
    var val = 0.5;
    var near = 1e9;
    var dab = 0.5;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = vec2<f32>(f32(x), f32(y));
            for (var s = 0; s < 3; s++) {
                let k = i + vec3<f32>(c, 0.0) + f32(s) * vec3<f32>(17.1, 3.9, 29.3);
                let h = hash33(k);
                let h2 = hash33(k + vec3<f32>(71.3, 19.1, 43.7));
                let q = f - c - h.xy;
                let ang = (h2.x - 0.5) * 1.4;
                let d = vec2<f32>(cos(ang), sin(ang));
                let t = dot(q, d);
                let e = q - d * t;
                // Up to one cell long: the 9 cells hold every stroke reaching p.
                let len = 0.75 + h.x * 0.25;
                let wid = w * (0.6 + h.y * 0.6);
                let gray = fract(h.z * 7.31 + h2.z * 3.17);
                let streak = (vnoise3(vec3<f32>(e * 26.0, 0.0) + h * 37.0) - 0.5) * bristle;
                let u = abs(t) / len;
                // Flat-ended ellipse, ragged, bristles breaking the ends.
                let sd = length(vec2<f32>(u * u * u, length(e) / wid)) + rag + abs(streak) * 1.5 * smoothstep(0.55, 1.0, u);
                let pr = fract(h2.z * 13.7 + h.y * 5.3 + f32(s) * 0.37);
                if sd < 1.0 && pr > top {
                    top = pr;
                    val = gray + streak * 0.4;
                }
                let dq = dot(q, q);
                if dq < near {
                    near = dq;
                    dab = gray;
                }
            }
        }
    }
    return clamp(select(dab, val, top >= 0.0), 0.0, 1.0);
}

// Paint splatter: in some cells (density) a splash of spray paint, a blob
// with a spiky border, droplets thrown around it and drips running down
// (-y) from it, ending in a drop. 1 on paint. Planar (each z slab its own
// layout), for tri-planar projection like brush strokes.
fn splatter(p: vec3<f32>, density: f32) -> f32 {
    let i = floor(p);
    let f = p.xy - i.xy;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let c = vec2<f32>(f32(x), f32(y));
            let k = vec3<f32>(i.xy + c, i.z);
            let h = hash33(k);
            let h2 = hash33(k + vec3<f32>(41.7, 13.3, 7.9));
            if h2.x > clamp(density, 0.0, 1.0) {
                continue;
            }
            let o = c + 0.2 + h.xy * 0.6;
            let q = f - o;
            let r = 0.12 + h.z * 0.2;
            let dl = length(q);
            let dir = q / max(dl, 1e-4);
            let bumps = vnoise3(vec3<f32>(dir * 2.5, h.x * 50.0));
            let spikes = pow(vnoise3(vec3<f32>(dir * 9.0, h.y * 50.0)), 4.0);
            if dl < r * (0.75 + bumps * 0.5 + spikes * 1.4) {
                return 1.0;
            }
            for (var n = 0; n < 8; n++) {
                let hn = hash33(k + f32(n) * vec3<f32>(3.1, 7.7, 1.3) + 9.0);
                let a = hn.x * 6.2831853;
                let dp = o + vec2<f32>(cos(a), sin(a)) * r * (1.3 + hn.y * 1.7);
                if length(f - dp) < r * (0.03 + 0.12 * hn.z * hn.z) {
                    return 1.0;
                }
            }
            for (var n = 0; n < 3; n++) {
                let hn = hash33(k + f32(n) * vec3<f32>(5.3, 2.9, 8.1) + 21.0);
                if hn.z < 0.3 {
                    continue;
                }
                let x0 = o.x + (hn.x - 0.5) * r * 1.4;
                let len = 0.3 + hn.y * 1.0;
                let wd = r * 0.1 * (0.6 + hn.z);
                let dy = o.y - f.y;
                let wob = sin(dy * 7.0 + hn.x * 9.0) * 0.01;
                if dy > 0.0 && dy < len && abs(f.x - x0 + wob) < wd * (1.0 - 0.35 * dy / len) {
                    return 1.0;
                }
                if length(vec2<f32>(f.x - x0, (o.y - len) - f.y)) < wd * 1.5 {
                    return 1.0;
                }
            }
        }
    }
    return 0.0;
}

// Streaks: thin runs down the object (rust, grime, water), varying across
// x and z and stretched along y, broken up along their length. density:
// how many.
fn streaks(p: vec3<f32>, density: f32) -> f32 {
    let s = fbm_value(vec3<f32>(p.x * 6.0, p.y * 0.3, p.z * 6.0), 4, 0.5);
    let b = fbm_value(vec3<f32>(p.x * 1.5, p.y * 0.7, p.z * 1.5) + 13.0, 3, 0.5);
    let runs = smoothstep(0.62 - density * 0.2, 0.75 - density * 0.2, s);
    return runs * smoothstep(0.35, 0.6, b);
}

// Tri-planar evaluation of a planar pattern (`planar_kind`): each texel
// takes the plane its normal faces most, with a ragged border between
// planes and no blend, so strokes stay crisp. p: projected position times
// the scale, n: the projected normal.
// Patterns drawn in the xy plane: tri-planar gives each face its plane.
fn planar_kind(kind: u32) -> bool {
    return kind == 10u || kind == 19u || kind == 28u || kind == 29u;
}

// (Coordinates only: the caller evaluates once, since every proc_eval call
// site inlines the whole pattern switch and several made the driver's
// compile take minutes.)
fn proc_tri(p: vec3<f32>, n: vec3<f32>, offs: vec2<f32>) -> vec3<f32> {
    let j = vec3<f32>(vnoise3(p * 2.0), vnoise3(p * 2.0 + 7.1), vnoise3(p * 2.0 + 3.3)) - 0.5;
    let a = abs(n) + j * 0.3;
    if a.x >= a.y && a.x >= a.z {
        return vec3<f32>(p.zy + offs, 11.0);
    }
    if a.y >= a.z {
        return vec3<f32>(p.xz + offs, 23.0);
    }
    return vec3<f32>(p.xy + offs, 37.0);
}

// Metallic flakes: each cell of a jittered grid is a small mirror tilted at
// random. pp.b = (scale in cells per unit / 8, tilt 0..1, coverage 0..1).
// Returns the tangent normal (xyz) and a gray per flake (w, 0 off a flake).
fn flake(pin: vec3<f32>, pp: ProcParams) -> vec4<f32> {
    let seed = pp.a.x;
    let p = pin * max(pp.b.x, 1.0) * 8.0 + vec3<f32>(seed * 13.17, seed * 7.31, seed * 3.77);
    let v = voronoi3(p, 1.0);
    let r = hash33(vec3<f32>(v.z * 91.7, v.z * 13.3 + 4.1, v.z * 5.9 + 1.7));
    if r.z > clamp(pp.b.z, 0.0, 1.0) {
        return vec4<f32>(0.0, 0.0, 1.0, 0.0);
    }
    // Random direction, tilt up to about 60 degrees.
    let a = r.x * 6.2831853;
    let t = sqrt(r.y) * clamp(pp.b.y, 0.0, 1.5) * 1.7;
    let n = normalize(vec3<f32>(cos(a) * t, sin(a) * t, 1.0));
    return vec4<f32>(n, 0.25 + 0.75 * fract(v.z * 17.31));
}

// Full evaluation: seed offset, domain warp, then balance and contrast.
fn proc_eval(kind: u32, pin: vec3<f32>, pp: ProcParams) -> f32 {
    let seed = pp.a.x;
    var p = pin * vec3<f32>(max(pp.b.y, 0.01), 1.0, 1.0);
    // Flakes read pp.b as their own parameters (no expansion, own seed).
    if kind == 27u {
        return clamp((flake(pin, pp).w - pp.a.z) * pp.a.y + 0.5, 0.0, 1.0);
    }
    if kind != 12u {
        p += vec3<f32>(seed * 13.17, seed * 7.31, seed * 3.77);
    }
    if pp.a.w > 0.0 {
        let w = vec3<f32>(
            fbm_value(p * 0.7 + 1.3, 3, 0.5),
            fbm_value(p * 0.7 + 7.7, 3, 0.5),
            fbm_value(p * 0.7 + 3.1, 3, 0.5)
        ) - 0.5;
        p += w * pp.a.w * 2.0;
    }
    var v = proc_raw(kind, p, pp);
    v = clamp((v - pp.a.z) * pp.a.y + 0.5, 0.0, 1.0);
    return v;
}
