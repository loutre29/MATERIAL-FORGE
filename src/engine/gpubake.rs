//! GPU ray pass for the baker: ambient occlusion and thickness rays traced
//! against the occluder BVH in compute, in chunks (each under the OS GPU
//! timeout). Same sample pattern as the CPU path in bake.rs.

use crate::bake::{Bvh, RayBackend};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Params {
    // first sample, count, ao samples, thickness samples
    k: vec4<u32>,
    // ao distance, thickness distance, epsilon, unused
    d: vec4<f32>,
};
struct Node { mn: vec4<f32>, mx: vec4<f32> };
struct Tri { p0: vec4<f32>, p1: vec4<f32>, p2: vec4<f32> };
// p.w = texel index bits, n, t
struct Sample { p: vec4<f32>, n: vec4<f32>, t: vec4<f32> };

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var<storage, read> tris: array<Tri>;
@group(0) @binding(3) var<storage, read> samples: array<Sample>;
@group(0) @binding(4) var<storage, read_write> outv: array<vec2<f32>>;

fn safe_inv(v: f32) -> f32 {
    if abs(v) < 1e-20 { return select(-1e30, 1e30, v >= 0.0); }
    return 1.0 / v;
}

fn slab(n: Node, o: vec3<f32>, inv: vec3<f32>, tmax: f32) -> f32 {
    let t0 = (n.mn.xyz - o) * inv;
    let t1 = (n.mx.xyz - o) * inv;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), max(min(t0.z, t1.z), 0.0));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), min(max(t0.z, t1.z), tmax));
    return select(1e30, tn, tn <= tf);
}

// Closest hit distance (or tmax); any_hit stops at the first one.
fn trace(o: vec3<f32>, d: vec3<f32>, tmax: f32, any_hit: bool) -> f32 {
    var best = tmax;
    let inv = vec3<f32>(safe_inv(d.x), safe_inv(d.y), safe_inv(d.z));
    var stack: array<u32, 48>;
    var sp = 1u;
    stack[0] = 0u;
    while sp > 0u {
        sp -= 1u;
        let n = nodes[stack[sp]];
        if slab(n, o, inv, best) >= 1e29 { continue; }
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
                if t > 0.0 && t < best {
                    best = t;
                    if any_hit { return best; }
                }
            }
        } else if sp + 2u <= 48u {
            stack[sp] = start;
            stack[sp + 1u] = start + 1u;
            sp += 2u;
        }
    }
    return best;
}

fn hash(i: u32) -> f32 {
    var x = i * 0x9E3779B9u;
    x ^= x >> 16u;
    x = x * 0x85EBCA6Bu;
    x ^= x >> 13u;
    return f32(x & 0xFFFFFFu) / 16777216.0;
}

fn cosine_dir(k: u32, n: u32, rot: f32, nn: vec3<f32>, t: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let u1 = (f32(k) + 0.5) / f32(n);
    let u2 = fract(f32(reverseBits(k)) * 2.3283064e-10 + rot);
    let r = sqrt(u1);
    let phi = u2 * 6.28318530718;
    return t * (r * cos(phi)) + b * (r * sin(phi)) + nn * sqrt(max(1.0 - u1, 0.0));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let local = g.x + g.y * 65535u * 64u;
    if local >= P.k.y { return; }
    let i = P.k.x + local;
    let s = samples[i];
    let p = s.p.xyz;
    let n = normalize(s.n.xyz);
    let t = normalize(s.t.xyz);
    let b = cross(n, t);
    let rot = hash(bitcast<u32>(s.p.w));
    var open = 0u;
    for (var k = 0u; k < P.k.z; k++) {
        let d = cosine_dir(k, P.k.z, rot, n, t, b);
        if trace(p + n * P.d.z, d, P.d.x, true) >= P.d.x { open += 1u; }
    }
    var tsum = 0.0;
    for (var k = 0u; k < P.k.w; k++) {
        let d = cosine_dir(k, P.k.w, rot, -n, t, -b);
        tsum += trace(p - n * P.d.z, d, P.d.y, false);
    }
    outv[i] = vec2<f32>(f32(open) / f32(max(P.k.z, 1u)), clamp(tsum / f32(max(P.k.w, 1u)) / max(P.d.y, 1e-9), 0.0, 1.0));
}
"#;

pub struct GpuBaker {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::ComputePipeline,
    nodes: wgpu::Buffer,
    tris: wgpu::Buffer,
}

impl GpuBaker {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, bvh: &Bvh) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpubake"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpubake"),
            layout: None,
            module: &module,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        let (nodes, _) = bvh.flatten();
        let tris = bvh.flatten_positions();
        let nodes = if nodes.is_empty() { vec![[0.0f32; 8]] } else { nodes };
        let tris = if tris.is_empty() { vec![[0.0f32; 12]] } else { tris };
        let buf = |data: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("bake bvh"), contents: data, usage: wgpu::BufferUsages::STORAGE })
        };
        let nodes = buf(bytemuck::cast_slice(&nodes));
        let tris = buf(bytemuck::cast_slice(&tris));
        Self { device, queue, pipeline, nodes, tris }
    }
}

impl RayBackend for GpuBaker {
    fn ao_thickness(
        &self,
        samples: &[[f32; 12]],
        ao_samples: u32,
        ao_dist: f32,
        th_samples: u32,
        th_dist: f32,
        eps: f32,
        cancel: &AtomicBool,
        on_progress: &dyn Fn(f32),
    ) -> Option<Vec<[f32; 2]>> {
        let n = samples.len();
        if n == 0 {
            return Some(Vec::new());
        }
        let d = &self.device;
        let input = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bake samples"),
            contents: bytemuck::cast_slice(samples),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let size = (n * 8) as u64;
        let output = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bake out"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bake readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        // Rays per chunk kept small so one submission stays well under the
        // Windows GPU timeout even on dense meshes.
        let rays = (ao_samples + th_samples).max(1) as usize;
        let chunk = (4_000_000 / rays).clamp(1024, 262_144);
        let mut first = 0usize;
        while first < n {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let count = chunk.min(n - first);
            let params: [u32; 8] = [
                first as u32,
                count as u32,
                ao_samples,
                th_samples,
                ao_dist.to_bits(),
                th_dist.to_bits(),
                eps.to_bits(),
                0,
            ];
            let ub = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bg = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.nodes.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.tris.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: input.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: output.as_entire_binding() },
                ],
            });
            let mut enc = d.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bg, &[]);
                let groups = count.div_ceil(64) as u32;
                pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
            }
            self.queue.submit(Some(enc.finish()));
            d.poll(wgpu::Maintain::Wait);
            first += count;
            on_progress(first as f32 / n as f32);
        }
        let mut enc = d.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
        self.queue.submit(Some(enc.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range();
        let out: Vec<[f32; 2]> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        readback.unmap();
        Some(out)
    }
}
