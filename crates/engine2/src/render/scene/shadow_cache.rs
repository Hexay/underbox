//! Shadow-map caching: a shadow view whose inputs ([`Key`]) equal those of the last render into
//! its target layer, with no other write to the texture since, keeps that render: no clear, no
//! caster draws. What the key holds and why: docs/lighting.md ("Shadow-map cache").
//! `UNDERBOX_SHADOW_CACHE=0` turns it off, `=stats` logs the hit rate every 1000 views.

use crate::render::draw::Draw;
use crate::render::texture::Texture;
use glam::Mat4;
use std::any::Any;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// A shadow view's inputs, byte for byte. Objects named by address are pinned (weakly): the
/// allocation outlives the object, so the address can't come back as another one while a key
/// holds it.
#[derive(Default)]
pub struct Key {
    bytes: Vec<u8>,
    pinned: Vec<Weak<dyn Any + Send + Sync>>,
    /// Something drawn can't be keyed (GPU-written draw arguments): never cached.
    uncacheable: bool,
}

impl Hasher for Key {
    fn write(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn finish(&self) -> u64 {
        unreachable!("a Key is compared whole, never reduced")
    }
}

impl Key {
    pub fn pin<T: Any + Send + Sync>(&mut self, a: &Arc<T>) {
        self.write_usize(Arc::as_ptr(a).cast::<()>() as usize);
        self.pinned.push(Arc::downgrade(a) as Weak<dyn Any + Send + Sync>);
    }

    pub fn floats(&mut self, f: &[f32]) {
        f.iter().for_each(|x| self.write_u32(x.to_bits()));
    }

    pub fn mat(&mut self, m: &Mat4) {
        self.floats(&m.to_cols_array());
    }

    /// One draw as the GPU sees it: buffers by identity and CPU write count, ranges, uniforms,
    /// state, caster instances, and a material program by identity (its depth variant reads
    /// only positions: the vertex cache's come from the object's pose, keyed by the caller).
    pub fn draw(&mut self, d: &Draw) {
        self.pin(&d.vertices);
        self.write_u64(d.vertices.writes());
        (d.vertex_offset, &d.vertex, d.range.clone(), d.base_vertex, d.instances, d.state).hash(self);
        match &d.indices {
            Some((b, format)) => {
                self.pin(b);
                (b.writes(), format).hash(self);
            }
            None => self.write_u8(0),
        }
        self.mat(&d.uniforms.mvp);
        self.mat(&d.uniforms.model);
        self.floats(&d.uniforms.tint);
        let instances = d.caster.as_deref().unwrap_or_default();
        instances.len().hash(self);
        for i in instances {
            self.mat(&i.world);
            self.floats(&i.tint);
        }
        d.shaded.is_some().hash(self);
        if let Some(s) = &d.shaded {
            self.pin(&s.shading);
            self.pin(&s.material);
            s.vertex.hash(self);
            self.mat(&s.view_proj);
            self.mat(&s.instance.world);
            self.floats(&s.instance.tint);
            s.context.slots.iter().for_each(|(slot, t)| (*slot, t.addr()).hash(self));
            // Managed's instance ids are GPU data the key can't see.
            self.uncacheable |= s.context.instance_ids.is_some();
        }
        self.uncacheable |= d.cull.indirect.is_some();
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.pinned.clear();
        self.uncacheable = false;
    }
}

/// The last render into one layer: its key, and the texture's write count right after it.
struct Entry {
    texture: Weak<Texture>,
    slice: u32,
    writes: u64,
    key: Key,
}

static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
/// Cleared keys for reuse, so a steady frame allocates none.
static SPARE: Mutex<Vec<Key>> = Mutex::new(Vec::new());
static VIEWS: AtomicU64 = AtomicU64::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);

fn mode() -> &'static str {
    static MODE: OnceLock<String> = OnceLock::new();
    MODE.get_or_init(|| std::env::var("UNDERBOX_SHADOW_CACHE").unwrap_or_default())
}

pub fn enabled() -> bool {
    mode() != "0"
}

/// An empty key.
pub fn key() -> Key {
    SPARE.lock().unwrap().pop().unwrap_or_default()
}

fn recycle(mut k: Key) {
    k.clear();
    let mut spare = SPARE.lock().unwrap();
    if spare.len() < 16 {
        spare.push(k);
    }
}

fn same(e: &Entry, t: &Arc<Texture>) -> bool {
    Weak::as_ptr(&e.texture) == Arc::as_ptr(t)
}

/// Whether `slice` of `t` still holds the render `key` would make: its last render had the same
/// key and nothing has written the texture since. Takes the key back either way on a hit.
pub fn holds(t: &Arc<Texture>, slice: u32, key: Key) -> Result<(), Key> {
    let writes = t.gpu.as_ref().map(render::Texture::writes);
    let hit = !key.uncacheable && ENTRIES.lock().unwrap().iter().any(|e| e.slice == slice && same(e, t) && Some(e.writes) == writes && e.key.bytes == key.bytes);
    count(hit);
    if !hit {
        return Err(key);
    }
    recycle(key);
    Ok(())
}

fn count(hit: bool) {
    let views = VIEWS.fetch_add(1, Ordering::Relaxed) + 1;
    let hits = HITS.fetch_add(u64::from(hit), Ordering::Relaxed) + u64::from(hit);
    if mode() == "stats" && views.is_multiple_of(1000) {
        abi::rt::log::log(abi::rt::log::Level::Info, &format!("shadow cache: {hits} of {views} views kept"));
    }
}

/// Record that `slice` of `t` now holds `key`'s render, made by writes of ours only from
/// `before` on (`ours`, from render/dirty_clear.rs's count): other layers of `t` recorded at
/// `before` stay valid. Without `ours` the layer is forgotten.
pub fn rendered(t: &Arc<Texture>, slice: u32, before: u64, ours: bool, key: Key) {
    let Some(g) = &t.gpu else { return recycle(key) };
    let now = g.writes();
    let mut entries = ENTRIES.lock().unwrap();
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        if e.texture.strong_count() == 0 || (e.slice == slice && same(e, t)) {
            recycle(entries.swap_remove(i).key);
        } else {
            i += 1;
        }
    }
    if !ours || key.uncacheable {
        return recycle(key);
    }
    for e in entries.iter_mut().filter(|e| same(e, t) && e.writes == before) {
        e.writes = now;
    }
    entries.push(Entry { texture: Arc::downgrade(t), slice, writes: now, key });
}

#[cfg(test)]
#[path = "shadow_cache_tests.rs"]
mod tests;
