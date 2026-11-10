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
