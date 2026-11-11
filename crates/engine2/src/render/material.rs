//! Materials (`IMaterial2` handles, `g_pMaterialSystem2`): a shader plus its parameters and the
//! render attributes it binds. With the empty device nothing compiles or draws, but managed
//! still builds, reads back and edits materials.

use crate::render::{attributes, texture};
use crate::resource::{self, ByName, Resource};
use abi::slots::{g_p_material_system2 as system, glue_resources as glue, i_material2 as m};
use abi::structs::Vector4;
use abi::{Guid, NativeRef, Registry, StrRet, bind, class};
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
    shader: String,
    attributes: attributes::Ref,
    params: Mutex<Params>,
    error: bool,
    guid: Guid,
}

impl Resource for Material {
    fn is_error(&self) -> bool {
        self.error
    }
    fn guid(&self) -> Guid {
        self.guid
    }
}

impl Material {
    pub fn shader(&self) -> &str {
        &self.shader
    }

    /// Texture params by attribute (`g_tColor`) and vector params by name, for a draw's binding.
    pub fn shading_params(&self) -> (HashMap<String, texture::Ref>, HashMap<String, glam::Vec4>) {
        let p = self.params.lock().unwrap();
        let vectors = p.vectors.iter().map(|(k, v)| (k.clone(), glam::Vec4::new(v.vec.x, v.vec.y, v.vec.z, v.vec.w))).collect();
        (p.textures.clone(), vectors)
    }
}

/// A strong handle the caller owns to the material at a resource path.
pub fn load_path(path: &str) -> Ref {
    load(std::ffi::CString::new(path).ok().as_deref(), Guid::default())
}

pub fn resolve(m: Ref) -> Option<Arc<Material>> {
    resource::resolve::<_, Material>(m)
}

static LOADED: ByName<Material> = ByName::new();

/// The material at a resource path, from its `.vmat` source; native's error material without one.
fn load(path: Option<&CStr>, guid: Guid) -> Ref {
    let name = string(path);
    let material = LOADED.find_or_insert(&name, || match super::material_source::load(&name) {
        Some(src) => {
            let material = new(name.clone(), src.shader, false, guid);
            let mut p = material.params.lock().unwrap();
            p.vectors.extend(src.vectors);
            p.textures.extend(src.textures);
            drop(p);
            material
        }
        None => new(name.clone(), String::new(), true, guid),
    });
    resource::new_handle(material)
}

fn new(name: String, shader: String, error: bool, guid: Guid) -> Material {
    Material { name, shader, attributes: attributes::create(), params: Mutex::default(), error, guid }
}

/// Texture params hold handles the material owns: managed takes ownership of whatever a
/// getter returns (Texture.FromNative), so getters hand out copies and setters keep copies.
fn copy(t: Texture) -> Texture {
    resource::copy_handle::<_, texture::Texture>(t)
}

impl Drop for Material {
    fn drop(&mut self) {
        super::material_mode::forget(self);
        objects::destroy(self.attributes);
        for t in self.params.get_mut().unwrap().textures.drain().map(|(_, t)| t) {
            resource::destroy_handle(t);
        }
    }
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub(crate) fn create_raw(name: Option<&CStr>, shader: Option<&CStr>, _anonymous: bool) -> Ref {
    resource::new_handle(Arc::new(new(string(name), string(shader), false, Guid::default())))
}

fn on<R: Default>(h: Ref, f: impl FnOnce(&Material) -> R) -> R {
    resource::resolve::<_, Material>(h).map(|mat| f(&mat)).unwrap_or_default()
}

fn params<R: Default>(h: Ref, f: impl FnOnce(&mut Params) -> R) -> R {
    on(h, |mat| f(&mut mat.params.lock().unwrap()))
}

/// `IMaterial2::Set( name, texture )`: the material keeps its own copy of the handle.
pub(crate) fn set_texture(h: Ref, name: &str, t: Texture) {
    let replaced = params(h, |p| p.textures.insert(name.to_owned(), copy(t)));
    replaced.into_iter().for_each(resource::destroy_handle);
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::i_material2, Material);
    bind!(r, system::CREATE_RAW_MATERIAL, create_raw);
    bind!(r, glue::GET_MATERIAL, load);
    bind!(r, system::FIND_OR_CREATE_MATERIAL_FROM_RESOURCE, |path| load(path, Guid::default()));
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
        let n = string(n);
        // Native answers the material's shader path for "shader" (managed's Material.ShaderName).
        let shader = || (n == "shader").then(|| on(h, |mat| mat.shader.clone())).filter(|s| !s.is_empty());
        StrRet::from(params(h, |p| p.strings.get(&n).cloned()).or_else(shader).unwrap_or_else(|| string(d)))
    });
    bind!(r, m::SET_1, |h, n, v| params(h, |p| {
        p.vectors.insert(string(n), v);
    }));
    bind!(r, m::GET_VECTOR4, |h, n| params(h, |p| p.vectors.get(&string(n)).copied().unwrap_or_default()));
    bind!(r, m::SET_2, |h, n, t| set_texture(h, &string(n), t));
    bind!(r, m::GET_TEXTURE, |h, n| params(h, |p| p.textures.get(&string(n)).copied()).map_or(Texture::NULL, copy));
    bind!(r, m::GET_FIRST_TEXTURE_ATTRIBUTE, |h| params(h, |p| p.textures.values().next().copied()).map_or(Texture::NULL, copy));
    bind!(r, m::SET_EDITED, |h, e| params(h, |p| p.edited = e));
    bind!(r, m::IS_EDITED, |h| params(h, |p| p.edited));
}
