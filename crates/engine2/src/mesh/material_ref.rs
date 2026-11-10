//! A strong `IMaterial2` handle native keeps (a mesh's or material group's material).

use crate::render::material::{self, Material};
use crate::resource;
use std::sync::Arc;

pub struct MaterialRef(material::Ref);

impl MaterialRef {
    /// Our own handle to the material behind `m`; `None` for a null or dead handle.
    pub fn keep(m: material::Ref) -> Option<Arc<Self>> {
        let own = resource::copy_handle::<_, Material>(m);
        (!own.is_null()).then(|| Arc::new(Self(own)))
    }

    /// The material at a resource path (`.vmat` source); native's error material without one.
    pub fn load(path: &str) -> Option<Arc<Self>> {
        let h = material::load_path(path);
        let own = Self::keep(h);
        resource::destroy_handle(h);
        own
    }

    /// The material itself, while the handle is alive.
    pub fn material(&self) -> Option<Arc<Material>> {
        material::resolve(self.0)
    }

    /// A new handle for managed, which owns (and destroys) it.
    pub fn handle(&self) -> material::Ref {
        resource::copy_handle::<_, Material>(self.0)
    }
}

impl Drop for MaterialRef {
    fn drop(&mut self) {
        resource::destroy_handle(self.0);
    }
}

/// The handle managed gets for an optional material.
pub fn handle(m: Option<&Arc<MaterialRef>>) -> material::Ref {
    m.map_or(material::Ref::NULL, |m| m.handle())
}
