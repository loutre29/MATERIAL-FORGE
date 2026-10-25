// Selection: merges a shape into a texture set's selection (r8, UV space).
// The mesh is rasterized in UV space like the brush (one fragment per
// texel); each texel looks its coverage up in a shape mask and hardware
// blending applies the operation (add: max, subtract: dst * (1 - c),
// intersect: dst * c; replace clears first, then adds).
//
// Spaces (params.x): 0 screen (the 3D view: the texel is projected with the
// camera and, when params.y is set, must be visible), 1 mask in UV space
// (the 2D view, or a texture-set sized mask: mask uv = uv * uvx.xy + uvx.zw),
// 2 every rasterized texel (select all / faces).

struct Sel {
    view_proj: mat4x4<f32>,
    // width, height (physical px), near, far
    viewport: vec4<f32>,
    // space, visible only, orthographic, unused
    params: vec4<f32>,
    uvx: vec4<f32>,
    // xyz eye
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> S: Sel;
@group(0) @binding(1) var scene_depth: texture_depth_multisampled_2d;
@group(0) @binding(2) var shape: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

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
    let n = S.viewport.z;
    let f = S.viewport.w;
    if S.params.z > 0.5 {
        return n + z * (f - n);
    }
    return n * f / (f - z * (f - n));
}

fn screen_coverage(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let clip = S.view_proj * vec4<f32>(world, 1.0);
    if clip.w <= 0.0 { return 0.0; }
    let ndc = clip.xyz / clip.w;
    if abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z < 0.0 || ndc.z > 1.0 { return 0.0; }
    let px = vec2<f32>((ndc.x * 0.5 + 0.5) * S.viewport.x, (0.5 - ndc.y * 0.5) * S.viewport.y);
    if S.params.y > 0.5 {
        if dot(normalize(normal), normalize(S.eye.xyz - world)) <= 0.0 { return 0.0; }
        let dims = vec2<i32>(textureDimensions(scene_depth));
        let ip = clamp(vec2<i32>(px), vec2<i32>(0), dims - vec2<i32>(1));
        let scene = linear_depth(textureLoad(scene_depth, ip, 0));
        let mine = linear_depth(ndc.z);
        if mine > scene * 1.004 + S.viewport.z * 0.5 { return 0.0; }
    }
    return textureSampleLevel(shape, samp, px / S.viewport.xy, 0.0).r;
}

@fragment
fn fs_select(in: VsOut) -> @location(0) vec4<f32> {
    let space = u32(S.params.x + 0.5);
    var c = 1.0;
    if space == 0u {
        c = screen_coverage(in.world, in.normal);
    } else if space == 1u {
        c = textureSampleLevel(shape, samp, in.uv * S.uvx.xy + S.uvx.zw, 0.0).r;
    }
    return vec4<f32>(c, c, c, c);
}
