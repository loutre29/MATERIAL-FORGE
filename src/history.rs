//! Undo / redo. Two kinds of entries:
//! - `Stroke`: painted textures, restored by copying a backup over the live
//!   texture (texture identity never changes, so stack snapshots stay valid).
//!   Brush strokes back up only the tiles they touch, as they touch them;
//! - `State`: a snapshot of a texture set's layer tree, selection and settings.
//!   Layers hold their textures in `Arc`s, so snapshots are cheap;
//! - `Studio`: the packshot studio's lights, background and camera before
//!   an edit.

use crate::document::{Document, SetState};
use crate::engine::{Engine, Tex};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_ENTRIES: usize = 80;
const BUDGET: u64 = 1536 * 1024 * 1024;
/// Side of the backup tiles of brush strokes.
const BACKUP_TILE: u32 = 256;

pub enum Backup {
    Full(Tex),
    /// Tiles saved before their first change, by origin (texels).
    Tiles(HashMap<(u32, u32), Tex>),
}

impl Backup {
    fn bytes(&self) -> u64 {
        match self {
            Backup::Full(t) => t.byte_size(),
            Backup::Tiles(m) => m.values().map(|t| t.byte_size()).sum(),
        }
    }
}

pub enum Entry {
    Stroke { label: String, set: usize, textures: Vec<(Arc<Tex>, Backup)> },
    State { label: String, set: usize, state: SetState },
    /// Geometry mask: hidden triangles per submesh.
    Hidden { label: String, hidden: Vec<Vec<bool>> },
    /// A texture set's selection before the change (selections are never
    /// edited in place, so the old texture is the backup).
    Selection { label: String, set: usize, selection: Option<Arc<Tex>> },
    Studio { label: String, studio: Box<crate::studio::Studio> },
}

fn copy_region(enc: &mut wgpu::CommandEncoder, src: &Tex, so: (u32, u32), dst: &Tex, d_o: (u32, u32), w: u32, h: u32) {
    enc.copy_texture_to_texture(
        wgpu::ImageCopyTexture { texture: &src.texture, mip_level: 0, origin: wgpu::Origin3d { x: so.0, y: so.1, z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyTexture { texture: &dst.texture, mip_level: 0, origin: wgpu::Origin3d { x: d_o.0, y: d_o.1, z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// Size of the backup tile at `origin` of `t` (edge tiles are smaller).
fn tile_size(t: &Tex, origin: (u32, u32)) -> (u32, u32) {
    (BACKUP_TILE.min(t.width - origin.0), BACKUP_TILE.min(t.height - origin.1))
}

impl Entry {
    pub fn label(&self) -> &str {
        match self {
            Entry::Stroke { label, .. } | Entry::State { label, .. } | Entry::Hidden { label, .. } | Entry::Selection { label, .. } | Entry::Studio { label, .. } => label,
        }
    }

    fn bytes(&self) -> u64 {
        match self {
            Entry::Stroke { textures, .. } => textures.iter().map(|(_, b)| b.bytes()).sum(),
            Entry::Selection { selection, .. } => selection.as_ref().map_or(0, |t| t.byte_size()),
            Entry::State { .. } | Entry::Hidden { .. } | Entry::Studio { .. } => 0,
        }
    }

    /// A tiled stroke that never touched the mesh.
    pub fn is_empty_stroke(&self) -> bool {
        matches!(self, Entry::Stroke { textures, .. } if textures.iter().all(|(_, b)| matches!(b, Backup::Tiles(m) if m.is_empty())))
    }

    /// Backs up `textures` whole before they get painted.
    pub fn stroke(engine: &Engine, label: &str, set: usize, textures: Vec<Arc<Tex>>) -> Entry {
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let pairs = textures
            .into_iter()
            .map(|t| {
                let b = t.duplicate(&engine.device, &mut enc, "undo");
                (t, Backup::Full(b))
            })
            .collect();
        engine.queue.submit(Some(enc.finish()));
        Entry::Stroke { label: label.to_string(), set, textures: pairs }
    }

    /// A brush stroke: nothing is copied yet, `backup_region` saves tiles
    /// right before the stroke first paints them.
    pub fn stroke_tiles(label: &str, set: usize, textures: Vec<Arc<Tex>>) -> Entry {
        let pairs = textures.into_iter().map(|t| (t, Backup::Tiles(HashMap::new()))).collect();
        Entry::Stroke { label: label.to_string(), set, textures: pairs }
    }

    /// Saves the tiles of `region` (texels, max exclusive) not saved yet.
    pub fn backup_region(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, region: [u32; 4]) {
        let Entry::Stroke { textures, .. } = self else { return };
        for (live, backup) in textures {
            let Backup::Tiles(tiles) = backup else { continue };
            let (x1, y1) = (region[2].min(live.width), region[3].min(live.height));
            for ty in (region[1] / BACKUP_TILE)..y1.div_ceil(BACKUP_TILE) {
                for tx in (region[0] / BACKUP_TILE)..x1.div_ceil(BACKUP_TILE) {
                    let o = (tx * BACKUP_TILE, ty * BACKUP_TILE);
                    if tiles.contains_key(&o) {
                        continue;
                    }
                    let (w, h) = tile_size(live, o);
                    let t = Tex::new(device, "undo tile", w, h, live.format, live.usage);
                    copy_region(enc, live, o, &t, (0, 0), w, h);
                    tiles.insert(o, t);
                }
            }
        }
    }
}

#[derive(Default)]
pub struct History {
    pub undo: Vec<Entry>,
    pub redo: Vec<Entry>,
    /// Bumped by every push / undo / redo / clear.
    pub version: u64,
}

impl History {
    pub fn clear(&mut self) {
        self.version += 1;
        self.undo.clear();
        self.redo.clear();
    }

    pub fn push(&mut self, e: Entry) {
        self.version += 1;
        self.undo.push(e);
        self.redo.clear();
        self.trim();
    }

    /// Drops the oldest entries over the count or memory budget (strokes grow
    /// while they paint, so this also runs when one ends).
    pub fn trim(&mut self) {
        while self.undo.len() > MAX_ENTRIES
            || (self.undo.len() > 1 && self.undo.iter().map(|e| e.bytes()).sum::<u64>() > BUDGET)
        {
            self.undo.remove(0);
        }
    }

    /// Applies an entry and returns its inverse.
    fn apply(engine: &Engine, doc: &mut Document, e: Entry) -> Entry {
        match e {
            Entry::Stroke { label, set, textures } => {
                let mut enc = engine.device.create_command_encoder(&Default::default());
                let swapped = textures
                    .into_iter()
                    .map(|(live, backup)| {
                        let now = match backup {
                            Backup::Full(backup) => {
                                let now = live.duplicate(&engine.device, &mut enc, "redo");
                                backup.copy_to(&mut enc, &live);
                                Backup::Full(now)
                            }
                            Backup::Tiles(tiles) => {
                                let mut now = HashMap::new();
                                for (o, t) in tiles {
                                    let (w, h) = (t.width, t.height);
                                    let cur = Tex::new(&engine.device, "redo tile", w, h, live.format, live.usage);
                                    copy_region(&mut enc, &live, o, &cur, (0, 0), w, h);
                                    copy_region(&mut enc, &t, (0, 0), &live, o, w, h);
                                    now.insert(o, cur);
                                }
                                Backup::Tiles(now)
                            }
                        };
                        (live, now)
                    })
                    .collect();
                engine.queue.submit(Some(enc.finish()));
                if let Some(s) = doc.sets.get_mut(set) {
                    s.dirty = true;
                    // Which layer owned the textures is not recorded: invalidate all caches.
                    crate::document::visit_mut(&mut s.layers, &mut |l| l.rev = crate::document::next_rev());
                }
                Entry::Stroke { label, set, textures: swapped }
            }
            Entry::State { label, set, state } => {
                let old = doc.sets[set].restore(state);
                Entry::State { label, set, state: old }
            }
            Entry::Hidden { label, hidden } => {
                let old = std::mem::replace(&mut doc.hidden, hidden);
                doc.refresh_visibility(engine);
                Entry::Hidden { label, hidden: old }
            }
            Entry::Selection { label, set, selection } => {
                let old = match doc.sets.get_mut(set) {
                    Some(s) => std::mem::replace(&mut s.selection, selection),
                    None => selection,
                };
                Entry::Selection { label, set, selection: old }
            }
            Entry::Studio { label, studio } => {
                let old = std::mem::replace(&mut doc.studio, *studio);
                Entry::Studio { label, studio: Box::new(old) }
            }
        }
    }

    pub fn undo(&mut self, engine: &Engine, doc: &mut Document) -> Option<String> {
        let e = self.undo.pop()?;
        self.version += 1;
        let label = e.label().to_string();
        let inv = Self::apply(engine, doc, e);
        self.redo.push(inv);
        doc.modified = true;
        Some(label)
    }

    pub fn redo(&mut self, engine: &Engine, doc: &mut Document) -> Option<String> {
        let e = self.redo.pop()?;
        self.version += 1;
        let label = e.label().to_string();
        let inv = Self::apply(engine, doc, e);
        self.undo.push(inv);
        doc.modified = true;
        Some(label)
    }
}
