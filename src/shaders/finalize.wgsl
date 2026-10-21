// Turns the composited stack into the final material maps: height becomes a
// tangent-space normal (combined with the baked normal by reoriented normal
// mapping), baked AO is applied, and every texel outside the UV islands is
// filled from its nearest island texel (infinite dilation).

struct FinalParams {
    height_scale: f32,
    ao_strength: f32,
    pad0: f32,
    pad1: f32,
};

@group(0) @binding(0) var<uniform> F: FinalParams;
@group(0) @binding(1) var acc_color: texture_2d<f32>;
@group(0) @binding(2) var acc_mrh: texture_2d<f32>;
@group(0) @binding(3) var baked_normal: texture_2d<f32>;
@group(0) @binding(4) var baked_ao: texture_2d<f32>;
@group(0) @binding(5) var nearest: texture_2d<u32>;
@group(0) @binding(6) var out_base: texture_storage_2d<rgba16float, write>;
@group(0) @binding(7) var out_orm: texture_storage_2d<rgba16float, write>;
@group(0) @binding(8) var out_normal: texture_storage_2d<rgba16float, write>;

fn covered(p: vec2<i32>, dims: vec2<i32>) -> bool {
    if any(p < vec2<i32>(0)) || any(p >= dims) {
        return false;
    }
    let n = vec2<i32>(textureLoad(nearest, p, 0).xy);
    return all(n == p);
}

fn height_at(p: vec2<i32>, fallback: f32, dims: vec2<i32>) -> f32 {
    if covered(p, dims) {
        return textureLoad(acc_mrh, p, 0).z;
    }
    return fallback;
}

@compute @workgroup_size(8, 8)
fn cs_finalize(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = vec2<i32>(textureDimensions(acc_color));
    let id = vec2<i32>(gid.xy);
    if id.x >= dims.x || id.y >= dims.y {
        return;
    }
    let src = vec2<i32>(textureLoad(nearest, id, 0).xy);
    let c = textureLoad(acc_color, src, 0);
    let m = textureLoad(acc_mrh, src, 0);
    let h = m.z;

    let hl = height_at(src + vec2<i32>(-1, 0), h, dims);
    let hr = height_at(src + vec2<i32>(1, 0), h, dims);
    let hu = height_at(src + vec2<i32>(0, -1), h, dims);
    let hd = height_at(src + vec2<i32>(0, 1), h, dims);
    let k = F.height_scale * f32(dims.x);
    let dhdu = (hr - hl) * 0.5 * k;
    // OpenGL tangent space: +v points up the image (toward row 0).
    let dhdv = (hu - hd) * 0.5 * k;
    let nh = normalize(vec3<f32>(-dhdu, -dhdv, 1.0));

    var nb = textureLoad(baked_normal, src, 0).xyz;
    if dot(nb, nb) < 1e-8 {
        nb = vec3<f32>(0.0, 0.0, 1.0);
    }
    nb = normalize(nb);
    let t = nb + vec3<f32>(0.0, 0.0, 1.0);
    let u = nh * vec3<f32>(-1.0, -1.0, 1.0);
    let n = normalize(t * dot(t, u) / t.z - u);

    let ao = mix(1.0, textureLoad(baked_ao, src, 0).r, F.ao_strength);
    textureStore(out_base, id, vec4<f32>(max(c.rgb, vec3<f32>(0.0)), 1.0));
    textureStore(out_orm, id, vec4<f32>(ao, clamp(m.x, 0.0, 1.0), clamp(m.y, 0.0, 1.0), h));
    textureStore(out_normal, id, vec4<f32>(n, 1.0));
}
