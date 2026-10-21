// 2D view: the active texture set's channel drawn in UV space, with the UV
// wireframe on top.

struct View {
    // uv -> ndc: ndc = uv * scale + offset
    xform: vec4<f32>,
    // display mode, line alpha, unused, unused
    mode: vec4<f32>,
};

@group(0) @binding(0) var<uniform> V: View;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct QOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_quad(@builtin(vertex_index) i: u32) -> QOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)
    );
    let uv = corners[i];
    var o: QOut;
    o.pos = vec4<f32>(uv * V.xform.xy + V.xform.zw, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn data(v: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2)), 1.0);
}

@fragment
fn fs_quad(in: QOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    let m = u32(V.mode.x + 0.5);
    switch m {
        case 1u: { return data(vec3<f32>(t.r)); }
        case 2u: { return data(vec3<f32>(t.g)); }
        case 3u: { return data(vec3<f32>(t.b)); }
        case 4u: { return data(vec3<f32>(t.a)); }
        case 5u: { return data(normalize(t.xyz) * 0.5 + 0.5); }
        case 6u: { return data(vec3<f32>(t.a * 0.5 + 0.5)); }
        default: { return vec4<f32>(t.rgb, 1.0); }
    }
}

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
};

@vertex
fn vs_line(v: VsIn) -> @builtin(position) vec4<f32> {
    return vec4<f32>(v.uv * V.xform.xy + V.xform.zw, 0.0, 1.0);
}

@fragment
fn fs_line() -> @location(0) vec4<f32> {
    return vec4<f32>(0.9, 0.9, 0.95, V.mode.y);
}
