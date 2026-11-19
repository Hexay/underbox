//! Per-frame clears limited to what the last frame drew. A pass that confines its writes to a
//! rect (a scissor, or quads it places itself) leaves its layer at the clear value everywhere
//! else. While no other write reaches the texture (`render::Texture::writes`), the next frame only
//! needs that rect cleared, not the layer: lavapipe's full clears of the shadow cascades and the
//! morph composite cost ~70 MB of DRAM reads a frame (docs/perf-bandwidth.md).

use crate::render::texture::Texture;
use std::sync::{Arc, Mutex, Weak};

/// A layer whose texels outside `dirty` hold its clear value, as of `writes`.
struct Known {
    texture: Weak<Texture>,
    slice: u32,
    writes: u64,
    dirty: Option<[u32; 4]>,
}

static KNOWN: Mutex<Vec<Known>> = Mutex::new(Vec::new());

/// Rects covering more than this share of the layer clear it all instead (one memset per tile).
const RECT_SHARE: f32 = 0.75;

/// A layer brought to its clear value by [`prepare`], for [`finish`].
pub struct Prepared {
    /// The caller must still clear the whole layer (its own pass's clear).
    pub full: bool,
    writes: u64,
    ours: u64,
}

fn same(k: &Known, t: &Arc<Texture>) -> bool {
    Weak::as_ptr(&k.texture) == Arc::as_ptr(t)
}

/// Clear what `slice` of `t` may hold besides `color` / `depth` (by its format), as far as
/// the record of its last [`finish`] allows; the rest is the caller's (`Prepared::full`).
pub fn prepare(t: &Arc<Texture>, slice: u32, color: render::wgpu::Color, depth: f32) -> Prepared {
    let Some((g, gpu)) = t.gpu.as_ref().zip(crate::render::device::gpu()) else { return Prepared { full: true, writes: 0, ours: 0 } };
    let writes = g.writes();
    let dirty = KNOWN.lock().unwrap().iter().find(|k| k.slice == slice && same(k, t) && k.writes == writes && k.texture.strong_count() > 0).map(|k| k.dirty);
    let small = |[_, _, w, h]: [u32; 4]| (w * h) as f32 <= RECT_SHARE * (g.desc.width * g.desc.height) as f32;
    match dirty {
        Some(None) => Prepared { full: false, writes, ours: 0 },
        Some(Some(r)) if small(r) && crate::render::context::rect_clearer(gpu).clear(gpu, g, slice, r, color, depth) => Prepared { full: false, writes, ours: 1 },
        _ => Prepared { full: true, writes, ours: 0 },
    }
}

/// Carry on from the last [`finish`] without clearing (a later pass of the same frame): the record
/// survives only if nothing else wrote since.
pub fn resume(t: &Arc<Texture>, slice: u32) -> Prepared {
    let current = t.gpu.as_ref().map(|g| g.writes());
    let writes = KNOWN.lock().unwrap().iter().find(|k| k.slice == slice && same(k, t)).map(|k| k.writes).filter(|w| Some(*w) == current);
    Prepared { full: false, writes: writes.unwrap_or(u64::MAX), ours: 0 }
}

/// Record that `slice` of `t` holds its clear value outside `dirty`, after the prepared clear and
/// `passes` more writes of ours. A count that doesn't match means another write landed in
/// between: the layer is forgotten. Other layers of `t` known before stay known, since these
/// writes touched only `slice`.
pub fn finish(t: &Arc<Texture>, slice: u32, p: Prepared, passes: u64, dirty: Option<[u32; 4]>) {
    let Some(g) = &t.gpu else { return };
    let now = g.writes();
    let mut known = KNOWN.lock().unwrap();
    known.retain(|k| k.texture.strong_count() > 0 && !(k.slice == slice && same(k, t)));
    if Some(now) != p.writes.checked_add(p.ours + passes) {
        return;
    }
    for k in known.iter_mut().filter(|k| same(k, t) && k.writes == p.writes) {
        k.writes = now;
    }
    known.push(Known { texture: Arc::downgrade(t), slice, writes: now, dirty });
}
