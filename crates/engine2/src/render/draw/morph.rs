//! Morph targets, applied on the CPU per object: its flex controller values (`anim::flex::values`)
//! run through each mesh's flex rules (`MorphSet::weights`) weigh the flex descriptors, and each
//! of the mesh's targets named like a descriptor adds weight × its deltas to the float positions
//! and normals of a copy of the mesh's vertex buffer, which the object draws instead.

use crate::mesh::buffers::VertexBuffer;
use crate::mesh::layout::format;
use crate::mesh::render_mesh::{Morph, RenderMesh};
use crate::scene::object::{self, SceneObject};
use assets::compiled::morph::MorphSet;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

/// Each morph target's weight, by name.
pub type Weights = HashMap<String, f32>;

/// One object's weights for each mesh of its model (content order).
pub struct ObjectMorphs {
    object: Weak<SceneObject>,
    key: usize,
    meshes: Vec<Weights>,
}

impl ObjectMorphs {
    /// The weights of mesh `index` (content order).
    pub fn mesh(&self, index: usize) -> Option<&Weights> {
        self.meshes.get(index)
    }
}

/// Each mesh's morph set (flex rules), in content order.
type Sets = Arc<Vec<Option<MorphSet>>>;

/// The sets of the compiled model `name`, read once.
fn sets(name: &str) -> Sets {
    static SETS: LazyLock<Mutex<HashMap<String, Sets>>> = LazyLock::new(Default::default);
    let key = name.to_ascii_lowercase();
    if let Some(s) = SETS.lock().unwrap().get(&key) {
        return s.clone();
    }
    let sets = Arc::new(crate::model::compiled::read(name).map(|m| m.meshes.into_iter().map(|mm| mm.morph).collect()).unwrap_or_default());
    SETS.lock().unwrap().entry(key).or_insert(sets).clone()
}

/// Descriptor weights for controller `values`: `set`'s flex rules, or without a set each
/// controller weighs the target of its own name.
pub fn weights(set: Option<&MorphSet>, controllers: &[String], values: &[f32]) -> Weights {
    match set.filter(|s| !s.descriptors.is_empty()) {
        Some(s) => s.descriptors.iter().cloned().zip(s.weights(values)).collect(),
        None => controllers.iter().cloned().zip(values.iter().copied()).collect(),
    }
}

/// The morph weights of object `r` now; `None` when its model has no morph targets or no
/// controller is set. Locks the object's state: call it before taking that lock.
pub fn of(r: object::Ref) -> Option<ObjectMorphs> {
    let scene = object::get(r)?;
    let handle = scene.state.lock().unwrap().model;
    let model = crate::model::get(handle)?;
    if model.content.meshes.iter().all(|m| m.mesh.state().morphs.is_empty()) {
        return None;
    }
    let values = crate::anim::flex::values(r.cast());
    if values.iter().all(|v| *v == 0.0) {
        return None;
    }
    let controllers = &crate::anim::data::of(handle).anim.flex;
    let sets = sets(&model.name);
    let meshes = (0..model.content.meshes.len()).map(|i| weights(sets.get(i).and_then(Option::as_ref), controllers, &values)).collect();
    Some(ObjectMorphs { object: Arc::downgrade(&scene), key: r.addr(), meshes })
}

/// `vb` with `morphs` added at `weights`; `None` when no target has weight or the layout's
/// position isn't float.
pub fn apply(vb: &VertexBuffer, morphs: &[Morph], weights: &Weights) -> Option<VertexBuffer> {
    let active: Vec<(&Morph, f32)> = morphs.iter().filter_map(|m| weights.get(&m.name).filter(|w| **w != 0.0).map(|w| (m, *w))).collect();
    if active.is_empty() {
        return None;
    }
    let field = |s: &str| vb.meta.fields.iter().find(|f| f.semantic == s && f.index == 0 && matches!(f.format, format::RGB32F | format::RGBA32F)).map(|f| f.offset);
    let (position, normal) = (field("position")?, field("normal"));
    let stride = vb.stride().max(1);
    let mut bytes = vb.read().clone();
    let count = bytes.len() / stride;
    for (m, w) in active {
        for d in &m.deltas {
            let Some(v) = usize::try_from(d.vertex).ok().filter(|v| *v < count) else { continue };
            add(&mut bytes, v * stride + position, d.position, w);
            if let Some(n) = normal {
                add(&mut bytes, v * stride + n, d.normal, w);
            }
        }
    }
    Some(VertexBuffer::new(vb.meta.clone(), bytes))
}

fn add(bytes: &mut [u8], at: usize, delta: [f32; 3], weight: f32) {
    for (i, d) in delta.iter().enumerate() {
        let Some(c) = bytes.get_mut(at + 4 * i..at + 4 * i + 4) else { return };
        let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]) + d * weight;
        c.copy_from_slice(&v.to_le_bytes());
    }
}

struct Cached {
    object: Weak<SceneObject>,
    base: Weak<VertexBuffer>,
    version: u64,
    weights: Weights,
    morphed: Arc<VertexBuffer>,
}

/// Morphed copies by (object, mesh), kept while the weights and base buffer stay the same so the
/// GPU copy is reused (gpu_mesh keys uploads by buffer identity).
static CACHE: LazyLock<Mutex<HashMap<(usize, usize), Cached>>> = LazyLock::new(Default::default);

/// The vertex buffer mesh `index` of an object with `morphs` draws in place of its own, if any.
pub fn vertices(morphs: &ObjectMorphs, index: usize, mesh: &RenderMesh) -> Option<Arc<VertexBuffer>> {
    let weights = morphs.meshes.get(index)?;
    let s = mesh.state();
    let vb = s.vertices.as_ref()?;
    let key = (morphs.key, std::ptr::from_ref(mesh) as usize);
    let mut cache = CACHE.lock().unwrap();
    if let Some(c) = cache.get(&key).filter(|c| c.base.upgrade().is_some_and(|b| Arc::ptr_eq(&b, vb)) && c.version == vb.version() && c.weights == *weights) {
        return Some(c.morphed.clone());
    }
    let morphed = Arc::new(apply(vb, &s.morphs, weights)?);
    cache.insert(key, Cached { object: morphs.object.clone(), base: Arc::downgrade(vb), version: vb.version(), weights: weights.clone(), morphed: morphed.clone() });
    Some(morphed)
}

/// Forget copies of objects or buffers that are gone.
pub fn prune() {
    CACHE.lock().unwrap().retain(|_, c| c.object.strong_count() > 0 && c.base.strong_count() > 0);
}

#[cfg(test)]
#[path = "morph_tests.rs"]
mod tests;
