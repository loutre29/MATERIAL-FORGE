// GGX-prefiltered environment for the viewport's reflections: mip k of the
// output holds the environment convolved with a GGX lobe of roughness
// k / (levels - 1) (Karis' split sum, n = v = r), sampled with filtered
// importance sampling from the source environment's mip chain.

struct P {
    // roughness, source width, source height, sample count
    a: vec4<f32>,
};

@group(0) @binding(0) var<uniform> U: P;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var dst: texture_storage_2d<rgba16float, write>;

const PI: f32 = 3.14159265;

fn dir_from_uv(uv: vec2<f32>) -> vec3<f32> {
    let phi = (uv.x - 0.5) * 2.0 * PI;
    let theta = uv.y * PI;
    return vec3<f32>(sin(theta) * sin(phi), cos(theta), -sin(theta) * cos(phi));
}

fn uv_from_dir(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(d.x, -d.z) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    var b = i;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return vec2<f32>(f32(i) / f32(n), f32(b) * 2.3283064365386963e-10);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let size = textureDimensions(dst);
    if g.x >= size.x || g.y >= size.y { return; }
    let uv = (vec2<f32>(g.xy) + 0.5) / vec2<f32>(size);
    let n = dir_from_uv(uv);
    let rough = U.a.x;
    let src_w = U.a.y;
    let src_h = U.a.z;
    // Mirror level: the source at this level's resolution.
    if rough <= 0.0 {
        let lod = max(log2(src_w / f32(size.x)), 0.0);
        textureStore(dst, g.xy, vec4<f32>(textureSampleLevel(src, samp, uv, lod).rgb, 1.0));
        return;
    }
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.y) < 0.999);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    let a = rough * rough;
    let a2 = a * a;
    let count = u32(U.a.w);
    // Average solid angle of a source texel.
    let sa_texel = 4.0 * PI / (src_w * src_h);
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var i = 0u; i < count; i++) {
        let xi = hammersley(i, count);
        let cos_t = sqrt((1.0 - xi.y) / (1.0 + (a2 - 1.0) * xi.y));
        let sin_t = sqrt(1.0 - cos_t * cos_t);
        let phi = 2.0 * PI * xi.x;
        let h = tx * (sin_t * cos(phi)) + ty * (sin_t * sin(phi)) + n * cos_t;
        let l = 2.0 * dot(n, h) * h - n;
        let ndl = dot(n, l);
        if ndl <= 0.0 { continue; }
        let ndh = max(dot(n, h), 0.0);
        let d = ndh * ndh * (a2 - 1.0) + 1.0;
        let dggx = a2 / (PI * d * d);
        // n = v: pdf of l is D * (n.h) / (4 (v.h)) = D / 4.
        let pdf = dggx * 0.25;
        let sa_sample = 1.0 / (f32(count) * pdf + 1e-4);
        let lod = clamp(0.5 * log2(sa_sample / sa_texel) + 1.0, 0.0, 16.0);
        acc += textureSampleLevel(src, samp, uv_from_dir(l), lod).rgb * ndl;
        wsum += ndl;
    }
    textureStore(dst, g.xy, vec4<f32>(acc / max(wsum, 1e-4), 1.0));
}
