// Post-processing: bloom chain (threshold + downsample, tent upsample) and
// the final tonemap / grading pass into the sRGB viewport texture.

struct Post {
    // exposure, tonemap (0 ACES, 1 filmic, 2 AgX-like, 3 linear), bloom intensity, bloom threshold
    a: vec4<f32>,
    // vignette, saturation, contrast, raw (1 = display data unchanged)
    b: vec4<f32>,
    // texel size of the source (xy), unused
    texel: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: Post;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: FsIn;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return o;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hable(x: vec3<f32>) -> vec3<f32> {
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}

fn agx_like(x: vec3<f32>) -> vec3<f32> {
    // Log encoding then a sigmoid; a light approximation of AgX's look.
    let l = clamp((log2(max(x, vec3<f32>(1e-6))) + 10.0) / 16.5, vec3<f32>(0.0), vec3<f32>(1.0));
    let s = l * l * (3.0 - 2.0 * l);
    return pow(s, vec3<f32>(2.2)) * 1.1;
}

@fragment
fn fs_bright(in: FsIn) -> @location(0) vec4<f32> {
    let t = P.texel.xy;
    var c = textureSample(src, samp, in.uv + vec2<f32>(-t.x, -t.y)).rgb
        + textureSample(src, samp, in.uv + vec2<f32>(t.x, -t.y)).rgb
        + textureSample(src, samp, in.uv + vec2<f32>(-t.x, t.y)).rgb
        + textureSample(src, samp, in.uv + vec2<f32>(t.x, t.y)).rgb;
    c = c * 0.25 * P.a.x;
    let l = max(c.r, max(c.g, c.b));
    let k = max(l - P.a.w, 0.0) / max(l, 1e-4);
    return vec4<f32>(c * k, 1.0);
}

@fragment
fn fs_down(in: FsIn) -> @location(0) vec4<f32> {
    let t = P.texel.xy;
    let c = textureSample(src, samp, in.uv + vec2<f32>(-t.x, -t.y)).rgb
        + textureSample(src, samp, in.uv + vec2<f32>(t.x, -t.y)).rgb
        + textureSample(src, samp, in.uv + vec2<f32>(-t.x, t.y)).rgb
        + textureSample(src, samp, in.uv + vec2<f32>(t.x, t.y)).rgb;
    return vec4<f32>(c * 0.25, 1.0);
}

@fragment
fn fs_up(in: FsIn) -> @location(0) vec4<f32> {
    let t = P.texel.xy;
    var c = textureSample(src, samp, in.uv).rgb * 4.0;
    c += textureSample(src, samp, in.uv + vec2<f32>(t.x, 0.0)).rgb * 2.0;
    c += textureSample(src, samp, in.uv - vec2<f32>(t.x, 0.0)).rgb * 2.0;
    c += textureSample(src, samp, in.uv + vec2<f32>(0.0, t.y)).rgb * 2.0;
    c += textureSample(src, samp, in.uv - vec2<f32>(0.0, t.y)).rgb * 2.0;
    c += textureSample(src, samp, in.uv + t).rgb;
    c += textureSample(src, samp, in.uv - t).rgb;
    c += textureSample(src, samp, in.uv + vec2<f32>(t.x, -t.y)).rgb;
    c += textureSample(src, samp, in.uv + vec2<f32>(-t.x, t.y)).rgb;
    return vec4<f32>(c / 16.0, 1.0);
}

@fragment
fn fs_final(in: FsIn) -> @location(0) vec4<f32> {
    let hdr = textureSample(src, samp, in.uv).rgb;
    if P.b.w > 0.5 {
        return vec4<f32>(hdr, 1.0);
    }
    var c = hdr * P.a.x + textureSample(bloom, samp, in.uv).rgb * P.a.z;
    let mode = u32(P.a.y + 0.5);
    switch mode {
        case 1u: { c = hable(c * 2.0) / hable(vec3<f32>(11.2)); }
        case 2u: { c = agx_like(c); }
        case 3u: { c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)); }
        default: { c = aces(c); }
    }
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(l), c, P.b.y);
    c = clamp((c - 0.18) * P.b.z + 0.18, vec3<f32>(0.0), vec3<f32>(1.0));
    let d = in.uv - 0.5;
    c *= 1.0 - P.b.x * smoothstep(0.2, 0.8, dot(d, d) * 2.0);
    return vec4<f32>(c, 1.0);
}
