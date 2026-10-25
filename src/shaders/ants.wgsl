// Marching ants: the edge of the active texture set's selection, drawn over
// the mesh (3D view, `vs_mesh`) or the UV square (2D view, `vs_quad`).
// The selection is read texel by texel through the set's nearest-covered
// map, so UV seams inside a selected area show no edge.

struct Camera {
    view_proj: mat4x4<f32>,
};

struct Ants {
    // x: dash phase (px), y: 2D view (1), zw: unused
    k: vec4<f32>,
    // 2D view: uv -> ndc = (dot(xform.xy, uv), dot(xform.zw, uv)) + offs.xy
    xform: vec4<f32>,
    offs: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var sel: texture_2d<f32>;
@group(1) @binding(1) var nearest: texture_2d<u32>;
@group(1) @binding(2) var<uniform> A: Ants;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_mesh(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = cam.view_proj * vec4<f32>(v.pos, 1.0);
    o.clip.z -= 0.0003 * o.clip.w;
    o.uv = v.uv;
    return o;
}

@vertex
fn vs_quad(@builtin(vertex_index) i: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)
    );
    var o: VsOut;
    o.uv = corners[i];
    o.clip = vec4<f32>(dot(A.xform.xy, o.uv) + A.offs.x, dot(A.xform.zw, o.uv) + A.offs.y, 0.0, 1.0);
    return o;
}

fn selected(uv: vec2<f32>) -> f32 {
    let d = vec2<i32>(textureDimensions(sel));
    if d.x <= 1 { return 0.0; }
    let t = clamp(vec2<i32>(floor(uv * vec2<f32>(d))), vec2<i32>(0), d - vec2<i32>(1));
    var p = t;
    // 3D: texels outside the islands take their nearest covered texel.
    if A.k.y < 0.5 {
        let n = vec2<i32>(textureLoad(nearest, clamp(t, vec2<i32>(0), vec2<i32>(textureDimensions(nearest)) - vec2<i32>(1)), 0).xy);
        p = clamp(n, vec2<i32>(0), d - vec2<i32>(1));
    }
    return textureLoad(sel, p, 0).r;
}

@fragment
fn fs_ants(in: VsOut) -> @location(0) vec4<f32> {
    // An edge pixel: a neighbor one pixel away is on the other side.
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let s = selected(in.uv) >= 0.5;
    let n = (selected(in.uv + dx) >= 0.5) != s || (selected(in.uv - dx) >= 0.5) != s
        || (selected(in.uv + dy) >= 0.5) != s || (selected(in.uv - dy) >= 0.5) != s;
    if !n || !s { discard; }
    let k = floor((in.clip.x + in.clip.y + A.k.x) / 5.0);
    let white = (i32(k) & 1) == 0;
    return select(vec4<f32>(0.0, 0.0, 0.0, 1.0), vec4<f32>(1.0, 1.0, 1.0, 1.0), white);
}
