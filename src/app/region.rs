//! Texel rect a frame of brush stamps can touch: the paint pass is scissored
//! to it and the compositor only recomposites it, so a stroke on a 4K set
//! costs what the brush covers, not the whole texture.

use crate::brush::{Space, View};
use crate::engine::composite::Region;
use crate::engine::paint::Stamp;
use crate::mesh::MeshData;
use glam::{Vec3, Vec4};

/// Stamp shapes reach sqrt(2) radii (square alpha textures); a margin on top.
const REACH: f32 = 1.5;

struct Tri {
    /// Screen bbox (physical px), infinite when the triangle crosses the eye plane.
    screen: [f32; 4],
    world_min: Vec3,
    world_max: Vec3,
    /// Texel bbox, max exclusive; empty when the UVs leave the texture.
    uv: Region,
    /// Some vertex normal faces the camera (camera mode skips the others).
    front: bool,
}

#[derive(Default)]
pub struct RegionFinder {
    key: Option<(usize, usize, u32, [u32; 16], [u32; 5])>,
    tris: Vec<Tri>,
}

fn texel_rect(min: [f32; 2], max: [f32; 2], res: u32) -> Region {
    let r = res as f32;
    let x0 = ((min[0] * r).floor() - 2.0).clamp(0.0, r) as u32;
    let y0 = ((min[1] * r).floor() - 2.0).clamp(0.0, r) as u32;
    let x1 = ((max[0] * r).ceil() + 2.0).clamp(0.0, r) as u32;
    let y1 = ((max[1] * r).ceil() + 2.0).clamp(0.0, r) as u32;
    [x0, y0, x1, y1]
}

fn grow(a: Option<Region>, b: Region) -> Option<Region> {
    if b[2] <= b[0] || b[3] <= b[1] {
        return a;
    }
    Some(match a {
        Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
        None => b,
    })
}

impl RegionFinder {
    fn prepare(&mut self, mesh: &MeshData, sub: usize, res: u32, view: &View) {
        let key = (
            mesh as *const MeshData as usize,
            sub,
            res,
            view.view_proj.to_cols_array().map(f32::to_bits),
            [view.size.x.to_bits(), view.size.y.to_bits(), view.eye.x.to_bits(), view.eye.y.to_bits(), view.eye.z.to_bits()],
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.tris.clear();
        let Some(sm) = mesh.submeshes.get(sub) else { return };
        let (w, h) = (view.size.x, view.size.y);
        for t in sm.indices.chunks_exact(3) {
            let v = [&mesh.vertices[t[0] as usize], &mesh.vertices[t[1] as usize], &mesh.vertices[t[2] as usize]];
            let p = v.map(|v| Vec3::from(v.pos));
            let mut screen = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for q in p {
                let c = view.view_proj * Vec4::new(q.x, q.y, q.z, 1.0);
                if c.w <= 1e-6 {
                    screen = [f32::MIN, f32::MIN, f32::MAX, f32::MAX];
                    break;
                }
                let (x, y) = ((c.x / c.w * 0.5 + 0.5) * w, (0.5 - c.y / c.w * 0.5) * h);
                screen = [screen[0].min(x), screen[1].min(y), screen[2].max(x), screen[3].max(y)];
            }
            // Back facing for sure only when every normal faces away from
            // every corner (the shader tests the interpolated normal).
            let front = v.iter().any(|a| p.iter().any(|&q| Vec3::from(a.normal).dot(view.eye - q) > 0.0));
            let (u0, u1) = (v.map(|v| v.uv[0]), v.map(|v| v.uv[1]));
            let uv_min = [u0[0].min(u0[1]).min(u0[2]), u1[0].min(u1[1]).min(u1[2])];
            let uv_max = [u0[0].max(u0[1]).max(u0[2]), u1[0].max(u1[1]).max(u1[2])];
            self.tris.push(Tri {
                screen,
                world_min: p[0].min(p[1]).min(p[2]),
                world_max: p[0].max(p[1]).max(p[2]),
                uv: texel_rect(uv_min, uv_max, res),
                front,
            });
        }
    }

    /// Texels the stamps can reach on submesh `sub` (None: nothing under them).
    pub fn region(&mut self, mesh: &MeshData, sub: usize, res: u32, view: &View, space: Space, stamps: &[Stamp]) -> Option<Region> {
        if stamps.is_empty() {
            return None;
        }
        let mut out = None;
        match space {
            Space::Uv => {
                for s in stamps {
                    let r = s.pos[3] * REACH;
                    out = grow(out, texel_rect([s.pos[0] - r, s.pos[1] - r], [s.pos[0] + r, s.pos[1] + r], res));
                }
            }
            Space::Camera => {
                self.prepare(mesh, sub, res, view);
                let boxes: Vec<[f32; 4]> = stamps
                    .iter()
                    .map(|s| {
                        let r = s.pos[3] * REACH;
                        [s.pos[0] - r, s.pos[1] - r, s.pos[0] + r, s.pos[1] + r]
                    })
                    .collect();
                let all = boxes.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
                let hit = |t: &[f32; 4], b: &[f32; 4]| t[0] <= b[2] && t[2] >= b[0] && t[1] <= b[3] && t[3] >= b[1];
                for t in self.tris.iter().filter(|t| t.front && hit(&t.screen, &all)) {
                    if boxes.iter().any(|b| hit(&t.screen, b)) {
                        out = grow(out, t.uv);
                    }
                }
            }
            Space::Wrap => {
                self.prepare(mesh, sub, res, view);
                for t in &self.tris {
                    let near = stamps.iter().any(|s| {
                        let c = Vec3::new(s.pos[0], s.pos[1], s.pos[2]);
                        let r = s.pos[3] * REACH;
                        let q = c.clamp(t.world_min, t.world_max);
                        q.distance_squared(c) <= r * r
                    });
                    if near {
                        out = grow(out, t.uv);
                    }
                }
            }
        }
        out
    }
}
