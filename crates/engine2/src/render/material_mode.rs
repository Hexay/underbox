//! `IMaterialMode` (`IMaterial2.GetMode`): a material in a shader mode, what `SetRenderState`
//! and `DrawSkyBox` take. Native hands out a pointer the material owns, not a strong handle, so
//! there is one per material, dropped with it. Every mode draws the material's one program.

use super::material::{self, Material};
use abi::slots::i_material2 as m;
use abi::{NativeRef, Registry, bind, class};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

pub type Ref = NativeRef<class::IMaterialMode>;

struct Mode {
    material: Weak<Material>,
}

/// Mode objects by material address.
static MODES: LazyLock<Mutex<HashMap<usize, usize>>> = LazyLock::new(Default::default);

fn of(h: material::Ref) -> Ref {
    let Some(mat) = material::resolve(h) else { return Ref::NULL };
    let key = Arc::as_ptr(&mat) as usize;
    let addr = *MODES.lock().unwrap().entry(key).or_insert_with(|| objects::create::<class::IMaterialMode, _>(Mode { material: Arc::downgrade(&mat) }).addr());
    NativeRef::from_raw(addr as *mut _)
}

/// The material behind a mode, while it lives.
pub fn material(r: Ref) -> Option<Arc<Material>> {
    objects::get::<_, Mode>(r).and_then(|m| m.material.upgrade())
}

/// Called as a material drops: its mode goes with it.
pub fn forget(material: *const Material) {
    if let Some(addr) = MODES.lock().unwrap().remove(&(material as usize)) {
        objects::destroy(Ref::from_raw(addr as *mut _));
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, m::GET_MODE, |h, _mode| of(h));
    bind!(r, m::GET_MODE_1, of);
    bind!(r, m::GET_MODE_2, |h, _layer| of(h));
}
