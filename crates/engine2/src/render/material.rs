//! Materials (`IMaterial2` handles, `g_pMaterialSystem2`): a shader plus its parameters and the
//! render attributes it binds. With the empty device nothing compiles or draws, but managed
//! still builds, reads back and edits materials.

use crate::render::attributes;
use crate::resource::{self, Resource};
use abi::slots::{g_p_material_system2 as system, i_material2 as m};
use abi::structs::Vector4;
use abi::{NativeRef, Registry, StrRet, bind, class};
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::IMaterial2>;
type Texture = NativeRef<class::CTextureBase>;

#[derive(Default)]
struct Params {
    strings: HashMap<String, String>,
    vectors: HashMap<String, Vector4>,
    textures: HashMap<String, Texture>,
    edited: bool,
}

pub struct Material {
    name: String,
    #[allow(dead_code)]
    shader: String,
    attributes: attributes::Ref,
    params: Mutex<Params>,
}

impl Resource for Material {}

impl Drop for Material {
    fn drop(&mut self) {
        objects::destroy(self.attributes);
    }
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn create_raw(name: Option<&CStr>, shader: Option<&CStr>, _anonymous: bool) -> Ref {
    let material = Material {
        name: string(name),
        shader: string(shader),
        attributes: attributes::create(),
        params: Mutex::default(),
    };
    resource::new_handle(Arc::new(material))
}

fn on<R: Default>(h: Ref, f: impl FnOnce(&Material) -> R) -> R {
    resource::resolve::<_, Material>(h).map(|mat| f(&mat)).unwrap_or_default()
}

fn params<R: Default>(h: Ref, f: impl FnOnce(&mut Params) -> R) -> R {
    on(h, |mat| f(&mut mat.params.lock().unwrap()))
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::i_material2, Material);
    bind!(r, system::CREATE_RAW_MATERIAL, create_raw);
    bind!(r, system::CREATE_PROCEDURAL_MATERIAL_COPY, |src, _kind, _recreate| resource::copy_handle::<_, Material>(src));
    bind!(r, system::FRAME_UPDATE, || {});

    bind!(r, m::GET_NAME, |h| StrRet::from(on(h, |mat| mat.name.clone())));
    bind!(r, m::GET_NAME_WITH_MOD, |h| StrRet::from(on(h, |mat| mat.name.clone())));
    bind!(r, m::IS_LOADED, |h| resource::resolve::<_, Material>(h).is_some());
    bind!(r, m::GET_RENDER_ATTRIBUTES, |h| on(h, |mat| mat.attributes));
    bind!(r, m::RECREATE_ALL_STATIC_CONSTANT_AND_COMMAND_BUFFERS, |_| {});
    bind!(r, m::RELOAD_STATIC_COMBOS, |_| {});
    bind!(r, m::GET_PHYSICS_SURFACE_PROPERTIES, |_| -1);

    bind!(r, m::HAS_PARAM, |h, n| {
        let n = string(n);
        params(h, |p| p.strings.contains_key(&n) || p.vectors.contains_key(&n) || p.textures.contains_key(&n))
    });
    bind!(r, m::SET, |h, n, v| params(h, |p| {
        p.strings.insert(string(n), string(v));
    }));
    bind!(r, m::GET_STRING, |h, n, d| {
        StrRet::from(params(h, |p| p.strings.get(&string(n)).cloned()).unwrap_or_else(|| string(d)))
    });
    bind!(r, m::SET_1, |h, n, v| params(h, |p| {
        p.vectors.insert(string(n), v);
    }));
    bind!(r, m::GET_VECTOR4, |h, n| params(h, |p| p.vectors.get(&string(n)).copied().unwrap_or_default()));
    bind!(r, m::SET_2, |h, n, t| params(h, |p| {
        p.textures.insert(string(n), t);
    }));
    bind!(r, m::GET_TEXTURE, |h, n| params(h, |p| p.textures.get(&string(n)).copied().unwrap_or_default()));
    bind!(r, m::GET_FIRST_TEXTURE_ATTRIBUTE, |h| params(h, |p| p.textures.values().next().copied().unwrap_or_default()));
    bind!(r, m::SET_EDITED, |h, e| params(h, |p| p.edited = e));
    bind!(r, m::IS_EDITED, |h| params(h, |p| p.edited));
}
