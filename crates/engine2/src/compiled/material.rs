//! Compiled materials (`.vmat_c`) in the shape of `render::material_source::Loaded` (private to
//! render, hence the twin): the shader, int (except static `F_*` combos), float and vector params
//! as vectors (scalars in x, as `.vmat` source gives them), and textures made from their `.vtex_c`.

use super::texture;
use crate::math::vec4;
use crate::render::texture::Ref as TextureRef;
use abi::structs::Vector4;
use assets::compiled::material::{self as vmat, Material};
use std::ffi::CString;

pub struct Loaded {
    pub shader: String,
    pub vectors: Vec<(String, Vector4)>,
    /// (attribute, owned texture handle).
    pub textures: Vec<(String, TextureRef)>,
}

pub fn compiled_path(path: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    if path.ends_with(".vmat") { format!("{path}_c") } else { format!("{path}.vmat_c") }
}

/// Every numeric param as a vector, in the order the file lists them.
pub fn vectors(m: &Material) -> Vec<(String, [f32; 4])> {
    let ints = m.ints.iter().filter(|(n, _)| !n.starts_with("F_")).map(|(n, v)| (n.clone(), [*v as f32, 0.0, 0.0, 0.0]));
    let floats = m.floats.iter().map(|(n, v)| (n.clone(), [*v, 0.0, 0.0, 0.0]));
    ints.chain(floats).chain(m.vectors.iter().cloned()).collect()
}

/// A texture handle for a compiled texture path; the error texture when it won't decode.
fn texture_ref(path: &str) -> TextureRef {
    let name = CString::new(path).ok();
    match texture::load(path) {
        Some(mut t) => crate::render::texture::create(name.as_deref(), false, t.config, &mut t.data),
        None => crate::render::texture::load(name.as_deref(), abi::Guid::default()),
    }
}

/// The compiled material at `path`; `None` without a readable `.vmat_c`.
pub fn load(path: &str) -> Option<Loaded> {
    let file = crate::fs::read(&compiled_path(path), "GAME")?;
    let m = vmat::read(&file).inspect_err(|e| abi::rt::log::warn(&format!("{path}: {e:?}"))).ok()?;
    let vectors = vectors(&m).into_iter().map(|(n, [x, y, z, w])| (n, vec4(x, y, z, w))).collect();
    let textures = m.textures.iter().map(|(attr, t)| (attr.clone(), texture_ref(t))).collect();
    Some(Loaded { shader: m.shader, vectors, textures })
}
