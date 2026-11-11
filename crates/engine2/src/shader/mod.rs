//! `CVfx` (HShader) from vfx source and its combo iterators, on crates/shaders. Combo ids are the
//! mixed-radix ids `shaders::ComboSet` documents, so `EnumerateCombos`, `DescribeCombo` and the
//! translation pipeline agree. Compiling and `.shader_c` loading are not bound yet.

use crate::bind_resource_handle;
use crate::resource::{self, ByName, Resource};
use abi::slots::{c_vfx as vfx, c_vfx_combo_iterator as it};
use abi::{NativeRef, Registry, StrRet, bind, class};
use shaders::{ComboSet, INVALID_INDEX, IncludePaths, Stage, VfxShader};
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

pub struct Vfx {
    name: String,
    /// Set for shaders loaded by resource path (`GetShader`): what `GetFilename` answers.
    resource: Option<String>,
    shader: RwLock<Option<Arc<VfxShader>>>,
}

static LOADED: ByName<Vfx> = ByName::new();

/// `shaders/x.shader` for `x`, `x.shader`, `x.shader_c` or `shaders/x.shader_c`.
fn resource_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim_start_matches('/');
    let p = p.strip_suffix("_c").unwrap_or(p);
    let p = p.strip_suffix(".shader").unwrap_or(p);
    let dir = if p.to_ascii_lowercase().starts_with("shaders/") { "" } else { "shaders/" };
    format!("{dir}{p}.shader")
}

/// The shader at a resource path, parsed once; `None` when its source isn't on disk (closed core
/// shaders such as `complex`) or doesn't parse.
pub fn find(path: &str) -> Option<Arc<VfxShader>> {
    by_path(path).shader.read().unwrap().clone()
}

/// The name of shader handle `h` (its resource path, or the name it was compiled under).
pub fn name_of(h: NativeRef<class::CVfx>) -> Option<String> {
    resource::resolve::<_, Vfx>(h).map(|v| v.resource.clone().unwrap_or_else(|| v.name.clone()))
}

fn by_path(path: &str) -> Arc<Vfx> {
    let path = resource_path(path);
    LOADED.find_or_insert(&path, || {
        let v = Vfx { name: path.clone(), resource: Some(path.clone()), shader: RwLock::new(None) };
        load(&v, &path);
        v
    })
}

impl Resource for Vfx {
    fn is_error(&self) -> bool {
        self.shader.read().unwrap().is_none()
    }
}

/// A program's combo spaces and the iterator's position in each.
struct Iter {
    statics: ComboSet,
    dynamics: ComboSet,
    s: u64,
    d: u64,
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn shader(h: NativeRef<class::CVfx>) -> Option<Arc<VfxShader>> {
    resource::resolve::<_, Vfx>(h).and_then(|v| v.shader.read().unwrap().clone())
}

/// `VfxProgram_t` → stage (`ShaderProgramType`: VS = 1 … CS = 6, RTX = 8).
fn stage(program: i64) -> Option<Stage> {
    Stage::ALL.into_iter().find(|s| *s as i64 == program)
}

/// An absolute path, else a path relative to the mounted roots (projects, cloud, core).
fn locate(path: &str) -> Option<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_absolute() && p.is_file() {
        return Some(p);
    }
    crate::fs::resolve(path, "GAME")
}

fn load(v: &Vfx, path: &str) -> bool {
    let includes = IncludePaths::new(crate::fs::search_roots());
    let parsed = locate(path).ok_or_else(|| format!("{path}: not found")).and_then(|p| VfxShader::parse_file(&p, &includes));
    match parsed {
        Ok(s) => {
            *v.shader.write().unwrap() = Some(Arc::new(s));
            true
        }
        Err(e) => {
            abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{}: {e}", v.name));
            false
        }
    }
}

/// "S_FOO=1 D_BAR=2" for the combos not at their minimum, as native's inline `DescribeCombo`.
fn describe(s: &VfxShader, program: i64, static_id: u64, dynamic_id: u64) -> String {
    let Some(p) = stage(program).and_then(|st| s.program_for(st)) else { return String::new() };
    [(&p.statics, static_id), (&p.dynamics, dynamic_id)]
        .into_iter()
        .flat_map(|(set, id)| set.combos.iter().zip(set.decode(id)).filter(|(c, v)| *v != c.min).map(|(c, v)| format!("{}={v}", c.name)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn with_iter(h: NativeRef<class::CVfxComboIterator>, f: impl FnOnce(&mut Iter) -> u64) -> u64 {
    objects::get::<_, Mutex<Iter>>(h).map_or(INVALID_INDEX, |i| f(&mut i.lock().unwrap()))
}

#[cfg(test)]
mod tests;

pub fn register(r: &mut Registry) {
    bind_resource_handle!(r, abi::slots::c_vfx, Vfx);
    bind!(r, vfx::CREATE, |name| {
        resource::new_handle(Arc::new(Vfx { name: string(name), resource: None, shader: RwLock::new(None) }))
    });
    bind!(r, abi::slots::glue_resources::GET_SHADER, |path, _guid| resource::new_handle(by_path(&string(path))));
    bind!(r, vfx::CREATE_FROM_SHADER_FILE, |h, path, _target, _flags| {
        resource::resolve::<_, Vfx>(h).is_some_and(|v| load(&v, &string(path)))
    });
    bind!(r, vfx::GET_FILENAME, |h| {
        let v = resource::resolve::<_, Vfx>(h);
        let file = || shader(h).map(|s| s.path.to_string_lossy().into_owned());
        StrRet::from(v.as_ref().and_then(|v| v.resource.clone()).or_else(file).or_else(|| v.map(|v| v.name.clone())).unwrap_or_default())
    });
    bind!(r, vfx::HAS_SHADER_PROGRAM, |h, program| {
        shader(h).is_some_and(|s| stage(program).is_some_and(|st| s.program_for(st).is_some()))
    });
    bind!(r, vfx::DESCRIBE_COMBO, |h, program, s, d| StrRet::from(shader(h).map(|sh| describe(&sh, program, s, d)).unwrap_or_default()));
    bind!(r, vfx::GET_ITERATOR, |h, program| {
        let p = shader(h).and_then(|s| stage(program).and_then(|st| s.program_for(st).cloned()));
        let (statics, dynamics) = p.map(|p| (p.statics, p.dynamics)).unwrap_or_default();
        objects::create(Mutex::new(Iter { statics, dynamics, s: INVALID_INDEX, d: INVALID_INDEX }))
    });

    bind!(r, it::DELETE, |h| {
        objects::destroy(h);
    });
    bind!(r, it::INVALID_INDEX, |_| INVALID_INDEX);
    bind!(r, it::FIRST_STATIC_COMBO, |h| with_iter(h, |i| {
        i.s = i.statics.next_valid(0);
        i.s
    }));
    bind!(r, it::NEXT_STATIC_COMBO, |h| with_iter(h, |i| {
        i.s = if i.s == INVALID_INDEX { INVALID_INDEX } else { i.statics.next_valid(i.s + 1) };
        i.s
    }));
    bind!(r, it::SET_STATIC_COMBO, |h, c| with_iter(h, |i| {
        i.s = if i.statics.is_valid(c) { c } else { INVALID_INDEX };
        i.s
    }));
    bind!(r, it::FIRST_DYNAMIC_COMBO, |h| with_iter(h, |i| {
        i.d = i.dynamics.next_valid(0);
        i.d
    }));
    bind!(r, it::NEXT_DYNAMIC_COMBO, |h| with_iter(h, |i| {
        i.d = if i.d == INVALID_INDEX { INVALID_INDEX } else { i.dynamics.next_valid(i.d + 1) };
        i.d
    }));
    bind!(r, it::SET_DYNAMIC_COMBO, |h, c| with_iter(h, |i| {
        i.d = if i.dynamics.is_valid(c) { c } else { INVALID_INDEX };
        i.d
    }));
}
