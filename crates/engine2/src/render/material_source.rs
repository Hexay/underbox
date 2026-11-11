//! A material's parameters from its `.vmat` source, textures bound to their shader attributes
//! (see `assets::material::bind_textures`).

use super::texture;
use crate::math::vec4;
use abi::Guid;
use abi::rt::log::{Level, log};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{LazyLock, Mutex};

pub struct Loaded {
    pub shader: String,
    pub vectors: Vec<(String, abi::structs::Vector4)>,
    /// (attribute, owned texture handle).
    pub textures: Vec<(String, texture::Ref)>,
}

/// A shader's texture inputs: (attribute, declared default).
type Declarations = Vec<(String, String)>;

/// Texture declarations per shader path; `None` for shaders not in source (closed ones).
static SHADERS: LazyLock<Mutex<HashMap<String, Option<Declarations>>>> = LazyLock::new(Default::default);

fn source_path(path: &str, ext: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    if path.ends_with(ext) { path.to_owned() } else { format!("{path}{ext}") }
}

fn shader_declarations(shader: &str) -> Option<Vec<(String, String)>> {
    let key = if shader.contains('/') { shader.to_owned() } else { format!("shaders/{shader}") };
    let mut cache = SHADERS.lock().unwrap();
    cache
        .entry(key.clone())
        .or_insert_with(|| {
            let file = crate::fs::resolve(&key, "GAME")?;
            Some(assets::material::texture_inputs(&std::fs::read_to_string(file).ok()?))
        })
        .clone()
}

/// `None` when the source is missing or unreadable (the caller makes the error material).
pub fn load(path: &str) -> Option<Loaded> {
    let Some(file) = crate::fs::resolve(&source_path(path, ".vmat"), "GAME") else {
        let c = crate::compiled::material::load(path)?;
        return Some(Loaded { shader: c.shader, vectors: c.vectors, textures: c.textures });
    };
    let material = assets::material::from_vmat(&std::fs::read_to_string(file).ok()?)
        .inspect_err(|e| log(Level::Warn, &format!("{path}: {e:?}")))
        .ok()?;
    let declared = shader_declarations(&material.shader);
    let mut textures = assets::material::bind_textures(&material, declared.as_deref());
    // Real's compiler fills complex's unset inputs (an unset colour is grey, not white).
    if std::path::Path::new(&material.shader).file_stem().is_some_and(|s| s == "complex") {
        textures = assets::compiled::material::with_default_textures(textures);
    }
    let textures = textures
        .into_iter()
        .map(|(attr, tex)| (attr, texture::load(CString::new(tex).ok().as_deref(), Guid::default())))
        .collect();
    // Features ride along as scalars: draws read F_TRANSLUCENT and friends (Material::blend).
    let features = material.features.iter().map(|(k, v)| (k.clone(), vec4(*v as f32, 0.0, 0.0, 0.0)));
    let vectors = material.values.iter().map(|(k, [x, y, z, w])| (k.clone(), vec4(*x, *y, *z, *w))).chain(features).collect();
    Some(Loaded { shader: material.shader, vectors, textures })
}
