// Brush painting. The mesh is rasterized in UV space (one fragment per
// texel). Each texel is weighted by every stamp of the frame; hardware
// blending accumulates the result into premultiplied layer textures.
//
// Brush modes (params.w): 0 camera (screen-space stamps, depth tested),
// 1 wrap (world-space sphere stamps on the surface), 2 UV (stamps in UV
// space, used by the 2D view), 3 full (every rasterized texel, polygon fill).

struct Brush {
    view_proj: mat4x4<f32>,
    // xyz eye, w unused
    eye: vec4<f32>,
    // width, height (physical px), near, far
    viewport: vec4<f32>,
    // base color rgb, opacity value
    color: vec4<f32>,
    // roughness, metallic, height, mask value
    values: vec4<f32>,
    emissive: vec4<f32>,
    normal: vec4<f32>,
    // hardness, angle fade (0/1), stamp count, mode
    params: vec4<f32>,
    // round (1) or alpha texture (0), stencil on, stencil drives color, stencil drives height
    shape: vec4<f32>,
    // stencil center xy (px), size (px), rotation (rad)
    stencil: vec4<f32>,
    // stencil aspect (w/h), height strength, orthographic camera (1), unused
    stencil2: vec4<f32>,
    // camera right vector, used to orient wrap stamps
    right: vec4<f32>,
    // x: paint the evaluated material (group 1) instead of uniform values,
    // y: the textures are a final-map snapshot (clone / smudge),
    // zw: source offset in texels
    material: vec4<f32>,
};

struct Stamp {
    // camera: xy px; wrap: xyz world; uv: xy uv. w = radius (px, world or uv)
    pos: vec4<f32>,
    // wrap: surface normal xyz; w = weight
    nrm: vec4<f32>,
    // x = angle (rad)
    ext: vec4<f32>,
};

@group(0) @binding(0) var<uniform> brush: Brush;
@group(0) @binding(1) var<storage, read> stamps: array<Stamp>;
@group(0) @binding(2) var scene_depth: texture_depth_multisampled_2d;
@group(0) @binding(3) var alpha_tex: texture_2d<f32>;
@group(0) @binding(4) var stencil_tex: texture_2d<f32>;
@group(0) @binding(5) var samp: sampler;

// Brush material evaluated over the texture set (texel aligned with the UV raster).
@group(1) @binding(0) var bm_col: texture_2d<f32>;
@group(1) @binding(1) var bm_mrh: texture_2d<f32>;
@group(1) @binding(2) var bm_emi: texture_2d<f32>;
@group(1) @binding(3) var bm_nrm: texture_2d<f32>;

struct Mat {
    col: vec4<f32>,
    mrh: vec4<f32>,
    emi: vec4<f32>,
    nrm: vec4<f32>,
};

fn brush_mat(frag: vec4<f32>) -> Mat {
    var m: Mat;
    m.col = vec4<f32>(brush.color.rgb, brush.color.w);
    m.mrh = vec4<f32>(brush.values.xyz, 0.0);
    m.emi = vec4<f32>(brush.emissive.rgb, 0.0);
    m.nrm = vec4<f32>(normalize(brush.normal.xyz), 0.0);
    let d = vec2<i32>(textureDimensions(bm_col));
    let p = clamp(vec2<i32>(frag.xy + brush.material.zw), vec2<i32>(0), d - vec2<i32>(1));
    let c = textureLoad(bm_col, p, 0);
    var r = textureLoad(bm_mrh, p, 0);
    if brush.material.y > 0.5 {
        // Final ORM layout: (ao, roughness, metallic, height)
        r = vec4<f32>(r.yzw, 0.0);
    }
    let e = textureLoad(bm_emi, p, 0);
    let n = textureLoad(bm_nrm, p, 0);
    if brush.material.x > 0.5 && d.x > 1 {
        m.col = c;
        m.mrh = r;
        m.emi = e;
        m.nrm = vec4<f32>(normalize(n.xyz), 0.0);
    }
    return m;
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
};

@vertex
fn vs_uv(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = vec4<f32>(v.uv.x * 2.0 - 1.0, 1.0 - v.uv.y * 2.0, 0.0, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.uv = v.uv;
    return o;
}

fn linear_depth(z: f32) -> f32 {
    let n = brush.viewport.z;
    let f = brush.viewport.w;
    if brush.stencil2.z > 0.5 {
        return n + z * (f - n);
    }
    return n * f / (f - z * (f - n));
}

struct Screen {
    px: vec2<f32>,
    visible: bool,
};

fn to_screen(world: vec3<f32>) -> Screen {
    var s: Screen;
    s.visible = false;
    s.px = vec2<f32>(-1e6);
    let clip = brush.view_proj * vec4<f32>(world, 1.0);
    if clip.w <= 0.0 { return s; }
    let ndc = clip.xyz / clip.w;
    s.px = vec2<f32>((ndc.x * 0.5 + 0.5) * brush.viewport.x, (0.5 - ndc.y * 0.5) * brush.viewport.y);
    if abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z < 0.0 || ndc.z > 1.0 { return s; }
    let dims = vec2<i32>(textureDimensions(scene_depth));
    let ip = clamp(vec2<i32>(s.px), vec2<i32>(0), dims - vec2<i32>(1));
    let scene = linear_depth(textureLoad(scene_depth, ip, 0));
    let mine = linear_depth(ndc.z);
    s.visible = mine <= scene * 1.004 + brush.viewport.z * 0.5;
    return s;
}

// Alpha of a stamp at local coordinates in [-1, 1].
fn shape(local: vec2<f32>) -> f32 {
    if brush.shape.x > 0.5 {
        let r = length(local);
        if r >= 1.0 { return 0.0; }
        return 1.0 - smoothstep(clamp(brush.params.x, 0.0, 0.99), 1.0, r);
    }
    if abs(local.x) >= 1.0 || abs(local.y) >= 1.0 { return 0.0; }
    // local.y points down the screen in every mode, like image rows.
    return textureSampleLevel(alpha_tex, samp, local * 0.5 + 0.5, 0.0).r;
}

struct Weight {
    w: f32,
    stencil: vec4<f32>,
};

fn brush_weight(world: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>) -> Weight {
    var out: Weight;
    out.w = 0.0;
    out.stencil = vec4<f32>(1.0);
    let mode = u32(brush.params.w + 0.5);
    let n = normalize(normal);
    let scr = to_screen(world);

    // Stencil: an image fixed on screen; texels take its color and alpha.
    if brush.shape.y > 0.5 {
        let rel = rotate_inv(scr.px - brush.stencil.xy, brush.stencil.w) / max(brush.stencil.z, 1.0);
        let suv = vec2<f32>(rel.x / max(brush.stencil2.x, 0.01), rel.y) + 0.5;
        if any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0)) || (mode != 2u && !scr.visible) {
            return out;
        }
        out.stencil = textureSampleLevel(stencil_tex, samp, suv, 0.0);
    }

    if mode == 3u {
        out.w = 1.0 * out.stencil.a;
        return out;
    }

    var fade = 1.0;
    if mode == 0u {
        if !scr.visible { return out; }
        let ndv = dot(n, normalize(brush.eye.xyz - world));
        if ndv <= 0.0 { return out; }
        if brush.params.y > 0.5 { fade = smoothstep(0.0, 0.3, ndv); }
    }

    let count = u32(brush.params.z);
    var keep = 1.0;
    for (var i = 0u; i < count; i++) {
        let s = stamps[i];
        var local: vec2<f32>;
        var f = 1.0;
        if mode == 0u {
            local = (scr.px - s.pos.xy) / s.pos.w;
        } else if mode == 1u {
            let d = world - s.pos.xyz;
            let sn = s.nrm.xyz;
            let facing = dot(n, sn);
            if facing <= 0.05 || abs(dot(d, sn)) > s.pos.w { continue; }
            // Orient the stamp with the camera right vector; fall back to a
            // world axis when the surface faces along it.
            var t = brush.right.xyz - sn * dot(brush.right.xyz, sn);
            if dot(t, t) < 0.01 {
                t = cross(select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(sn.y) > 0.9), sn);
            }
            t = normalize(t);
            let b = cross(sn, t);
            local = vec2<f32>(dot(d, t), -dot(d, b)) / s.pos.w;
            if brush.params.y > 0.5 { f = smoothstep(0.05, 0.35, facing); }
        } else {
            local = (uv - s.pos.xy) / s.pos.w;
        }
        if dot(local, local) > 2.0 { continue; }
        local = rotate_inv(local, s.ext.x);
        let a = shape(local);
        if a > 0.0 {
            keep *= 1.0 - clamp(s.nrm.w * a * f * fade, 0.0, 1.0);
        }
    }
    out.w = (1.0 - keep) * out.stencil.a;
    return out;
}

fn rotate_inv(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x + s * p.y, -s * p.x + c * p.y);
}

struct OutA {
    @location(0) color: vec4<f32>,
    @location(1) values: vec4<f32>,
    @location(2) alphas: vec4<f32>,
};

struct OutB {
    @location(0) emissive: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) opacity: vec4<f32>,
};

@fragment
fn fs_paint_a(in: VsOut) -> OutA {
    let r = brush_weight(in.world, in.normal, in.uv);
    let w = r.w;
    let bm = brush_mat(in.clip);
    if w <= 0.0 { discard; }
    var color = bm.col.rgb;
    var height = bm.mrh.z;
    if brush.shape.y > 0.5 && brush.shape.z > 0.5 { color = r.stencil.rgb; }
    if brush.shape.y > 0.5 && brush.shape.w > 0.5 {
        height = (dot(r.stencil.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) - 0.5) * 2.0 * brush.stencil2.y;
    }
    var o: OutA;
    o.color = vec4<f32>(color * w, w);
    o.values = vec4<f32>(bm.mrh.x * w, bm.mrh.y * w, height * w, w);
    o.alphas = vec4<f32>(w);
    return o;
}

@fragment
fn fs_paint_b(in: VsOut) -> OutB {
    let w = brush_weight(in.world, in.normal, in.uv).w;
    let bm = brush_mat(in.clip);
    if w <= 0.0 { discard; }
    var o: OutB;
    o.emissive = vec4<f32>(bm.emi.rgb * w, w);
    o.normal = vec4<f32>(bm.nrm.xyz * w, w);
    o.opacity = vec4<f32>(bm.col.w * w, 0.0, 0.0, w);
    return o;
}

@fragment
fn fs_erase_a(in: VsOut) -> OutA {
    let w = brush_weight(in.world, in.normal, in.uv).w;
    if w <= 0.0 { discard; }
    var o: OutA;
    o.color = vec4<f32>(0.0, 0.0, 0.0, w);
    o.values = vec4<f32>(0.0, 0.0, 0.0, w);
    o.alphas = vec4<f32>(w);
    return o;
}

@fragment
fn fs_erase_b(in: VsOut) -> OutB {
    let w = brush_weight(in.world, in.normal, in.uv).w;
    if w <= 0.0 { discard; }
    var o: OutB;
    o.emissive = vec4<f32>(0.0, 0.0, 0.0, w);
    o.normal = vec4<f32>(0.0, 0.0, 0.0, w);
    o.opacity = vec4<f32>(0.0, 0.0, 0.0, w);
    return o;
}

// Mask paint effect texture (rg16f): r = value * coverage, g = coverage.
@fragment
fn fs_mask(in: VsOut) -> @location(0) vec4<f32> {
    let r = brush_weight(in.world, in.normal, in.uv);
    let w = r.w;
    if w <= 0.0 { discard; }
    var v = brush.values.w;
    if brush.shape.y > 0.5 && brush.shape.z > 0.5 {
        v = dot(r.stencil.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    }
    return vec4<f32>(v * w, w, 0.0, w);
}

@fragment
fn fs_mask_erase(in: VsOut) -> @location(0) vec4<f32> {
    let w = brush_weight(in.world, in.normal, in.uv).w;
    if w <= 0.0 { discard; }
    return vec4<f32>(0.0, 0.0, 0.0, w);
}
